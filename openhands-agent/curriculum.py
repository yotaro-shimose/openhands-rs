import asyncio
import os
import shutil
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import List, Literal

from agents.extensions.models.litellm_model import LitellmModel
from agents.tracing import add_trace_processor
from dotenv.main import load_dotenv
from oai_utils.agent import AgentWrapper
from oai_utils.conversion import contents2params
from oai_utils.tracing import AgentContentPrinter
from pydantic import BaseModel, Field

from openhands_agent.exam.repository import chmod_recursive
from openhands_agent.runtime.rust_env import RustCodingEnvironment

# --- PROMPTS ---

EXPLORER_PROMPT = """You are an expert Rust Researcher and Curriculum Architect.
Your goal is to explore the provided Rust library and generate a **Comprehensive Summary** for a specific topic.

<INPUT>
- **Task Instruction**: {instruction}
- **Scope**: The directory or concept you are assigned to.
- **Library Path**: /workspace/repos/library
- **Working Directory**: {work_dir}
</INPUT>

<INSTRUCTIONS>
1. **Explore**:
   - thoroughly read the relevant code in `/workspace/repos/library`.
   - Look for modules, structs, traits, and functions related to your assigned instruction.
   - You can look at `README.md`, `examples/`, and tests.
2. **Summarize**:
   - Create a detailed summary named `comprehensive_summary.md` in your Working Directory.
   - This summary must explicitly list the API surface, key concepts, and usage patterns relevant to your task.
   - It serves as the "knowledge base" for deciding how to teach this topic.
</INSTRUCTIONS>
"""

EVALUATOR_PROMPT = """You are a rigorous Curriculum Auditor.
Your job is to verify that the `comprehensive_summary.md` is truly comprehensive and accurate regarding the codebase.

<INPUT>
1. `comprehensive_summary.md`: The draft summary.
2. `/workspace/repos/library`: The actual source code.
</INPUT>

<INSTRUCTIONS>
1. **Verify**: Check if the summary misses any public structs, enums, or functions that are relevant to the topic.
2. **Evaluate**:
   - If significant concepts are missing or incorrect, set `is_comprehensive` to False and provide specific `feedback`.
   - If the summary is excellent (covers >95% of the relevant scope), set `is_comprehensive` to True.
</INSTRUCTIONS>
"""

DISPATCHER_PROMPT = """You are a Curriculum Manager.
Your goal is to decide how to process the current task based on the `comprehensive_summary.md`.

<INPUT>
- **Instruction**: {instruction}
- **Summary**: `comprehensive_summary.md`
</INPUT>

<DECISION_LOGIC>
Determine if the scope described in the summary is small enough to be a **Single Leaf File** (e.g., a single concept, struct, or small module) or if it is too complex and requires **Splitting** into sub-tasks.

- **Split (Delegate)**:
  - If the topic covers multiple distinct sub-modules or concepts that deserve their own chapters/sections.
  - Create a list of sub-tasks. Each sub-task needs a directory name and a specific instruction.
  
- **Write (Leaf)**:
  - If the topic is atomic enough to be explained in one cohesive markdown file (e.g., 200-1000 lines of text/code).
  - You will simply signal to write the content.
</DECISION_LOGIC>
"""

WRITER_PROMPT = """You are an expert Rust Technical Writer.
Your goal is to write a high-quality educational markdown file for the current topic.

<INPUT>
- **Instruction**: {instruction}
- **Summary**: `comprehensive_summary.md`
</INPUT>

<INSTRUCTIONS>
1. Write a file named `content.md` (or a more descriptive name if appropriate, e.g., `01_intro.md`) in the current directory.
2. The content should be a textbook-style explanation.
3. **Include**:
   - Explanations of concepts.
   - Runnable Rust code snippets (verify against the library).
   - Expected output.
   - Best practices.
</INSTRUCTIONS>
"""

# --- MODELS ---


class EvaluationResult(BaseModel):
    is_comprehensive: bool = Field(description="Is the summary comprehensive?")
    feedback: str = Field(description="Feedback if not comprehensive.")


class SubTask(BaseModel):
    directory_name: str = Field(
        description="Name of the subdirectory for this sub-task (e.g. '01_parser')"
    )
    instruction: str = Field(description="Specific instruction for the sub-agent.")


class DecisionResult(BaseModel):
    action: Literal["split", "write"] = Field(
        description="Action to take: 'split' or 'write'"
    )
    sub_tasks: List[SubTask] | None = Field(
        default=None, description="List of sub-tasks if action is 'split'"
    )
    final_file_name: str | None = Field(
        default=None, description="Name of the final file if action is 'write'"
    )


class CurriculumConfig(BaseModel):
    curriculum_id: str = Field(default="generated_textbook")
    model_name: str = Field(default="gemini/gemini-3-flash-preview")
    workspace_dir: Path = Field(default=Path("workspace_curriculum2"))
    repository_path: Path = Field(default=Path("repositories/numrs"))
    max_concurrency: int = Field(default=2)

    def get_workspace_dir(self) -> Path:
        return self.workspace_dir.resolve()


@dataclass
class TaskQueueItem:
    instruction: str
    work_dir: Path
    depth: int


# --- AGENT CLASSES ---


class CurriculumAgent:
    def __init__(
        self,
        config: CurriculumConfig,
        model: LitellmModel,
        runtime: RustCodingEnvironment,
        semaphore: asyncio.Semaphore,
    ):
        self.config = config
        self.model = model
        self.runtime = runtime
        self.semaphore = semaphore

    def _to_container_path(self, path: Path) -> Path:
        """Convert a host path to a path inside the container (assuming /workspace mount)."""
        try:
            rel = path.relative_to(self.config.get_workspace_dir())
            return Path("/workspace") / rel
        except ValueError:
            return path

    async def run_queue(self, root_task: TaskQueueItem):
        queue = deque([root_task])
        running_tasks = set()

        # Loop until no tasks are left in queue AND no tasks are currently running
        while queue or running_tasks:
            while queue and len(running_tasks) < self.config.max_concurrency:
                task = queue.pop()  # LIFO (Stack) -> DFS behavior

                # Create a task wrapper to handle the result
                fut = asyncio.create_task(self.process_single_task(task))
                running_tasks.add(fut)

            if not running_tasks:
                break

            # Wait for any task to complete
            done, pending = await asyncio.wait(
                running_tasks, return_when=asyncio.FIRST_COMPLETED
            )

            for fut in done:
                running_tasks.remove(fut)
                try:
                    # Result is a tuple: (DecisionResult, TaskQueueItem)
                    # or None if failed/leaf
                    decision, parent_task = await fut

                    if decision and decision.action == "split" and decision.sub_tasks:
                        print(
                            f"[{parent_task.depth}] Splitting into {len(decision.sub_tasks)} sub-tasks."
                        )
                        # Add subtasks to stack (reverse order to pop first one first? or just extend)
                        # ensure DFS: children of just-finished task are processed next.
                        # Since we use `pop()` (from right/end), we should `extend` (add to right/end).
                        # Order: subtask 0 .. N.
                        # If we extend([T0, T1]), queue is [...old, T0, T1].
                        # pop() gives T1, then T0. So we execute T1 first.
                        # Standard DFS visits children in order?
                        # If we want to process T0 first, we should extend([T1, T0]).
                        # But it doesn't strictly matter for curriculum generation.

                        new_items = []
                        for sub in decision.sub_tasks:
                            sub_dir = parent_task.work_dir / sub.directory_name
                            new_items.append(
                                TaskQueueItem(
                                    instruction=sub.instruction,
                                    work_dir=sub_dir,
                                    depth=parent_task.depth + 1,
                                )
                            )

                        # Add to queue (Stack)
                        queue.extend(new_items)

                except Exception as e:
                    print(f"Task failed with error: {e}")
                    # In a real system we might log this or retry.

    async def process_single_task(self, task: TaskQueueItem):
        """Processes a single node. Returns (DecisionResult, task) so the loop can enqueue children."""
        print(f"\n[{task.depth}] Processing Task in {task.work_dir.name}...")
        task.work_dir.mkdir(parents=True, exist_ok=True)
        chmod_recursive(task.work_dir)

        instruction_file = task.work_dir / "instruction.md"
        instruction_file.write_text(task.instruction)
        chmod_recursive(instruction_file)

        container_work_dir = self._to_container_path(task.work_dir)

        async with self.semaphore:
            # 1. Explore
            await self._phase_explore(
                task.instruction, task.work_dir, container_work_dir
            )

            # 2. Decide
            decision = await self._phase_decide(
                task.instruction, task.work_dir, container_work_dir
            )

            # 3. Act (Write Phase Only)
            if decision.action == "write":
                print(f"[{task.depth}] Writing leaf content.")
                await self._phase_write(
                    task.instruction,
                    task.work_dir,
                    container_work_dir,
                    decision.final_file_name or "content.md",
                )
                return (decision, task)

            elif decision.action == "split":
                # Don't recurse here. Just return the decision.
                return (decision, task)

        return (None, task)

    async def _phase_explore(
        self, instruction: str, work_dir: Path, container_work_dir: Path
    ):
        print("  > Exploring...")

        prompt = EXPLORER_PROMPT.format(
            instruction=instruction, work_dir=container_work_dir
        )

        # Generator - Evaluator Loop
        max_retries = 3
        current_feedback = ""

        explorer_history = []

        explorer = AgentWrapper.create(
            name=f"Explorer-{work_dir.name}",
            instructions=prompt,
            model=self.model,
            mcp_servers=[self.runtime.server],
        )

        for i in range(max_retries):
            # 1. Generate
            user_msg = f"Generate comprehensive_summary.md in {container_work_dir}. "
            if current_feedback:
                user_msg += f"Critique feedback: {current_feedback}"

            explorer_history.extend(contents2params("user", [user_msg]))
            resp = await explorer.run(explorer_history, max_turns=50)
            explorer_history.extend(resp.result.to_input_list())

            summary_file = work_dir / "comprehensive_summary.md"
            if not summary_file.exists():
                print(f"  ❌ Summary generation failed in {work_dir}")
                # Retry if failure
                continue

            # 2. Evaluate
            evaluator = AgentWrapper[EvaluationResult].create(
                name=f"Evaluator-{work_dir.name}",
                instructions=EVALUATOR_PROMPT,
                model=self.model,
                mcp_servers=[self.runtime.server],
                output_type=EvaluationResult,
            )

            summary_container_path = container_work_dir / "comprehensive_summary.md"

            eval_resp = await evaluator.run(
                f"Evaluate {summary_container_path} against /workspace/repos/library",
                max_turns=40,
            )
            result = eval_resp.result.final_output

            if result.is_comprehensive:
                print(f"  ✅ Summary verified for {work_dir.name}")
                break

            current_feedback = result.feedback
            print(f"  ⚠️ Summary critique: {current_feedback[:50]}...")

    async def _phase_decide(
        self, instruction: str, work_dir: Path, container_work_dir: Path
    ) -> DecisionResult:
        print("  > Deciding...")
        prompt = DISPATCHER_PROMPT.format(instruction=instruction)

        dispatcher = AgentWrapper[DecisionResult].create(
            name=f"Dispatcher-{work_dir.name}",
            instructions=prompt,
            model=self.model,
            mcp_servers=[self.runtime.server],
            output_type=DecisionResult,
        )

        summary_path = container_work_dir / "comprehensive_summary.md"

        resp = await dispatcher.run(
            f"Read {summary_path} and decide. Instruction: {instruction}", max_turns=20
        )
        return resp.result.final_output

    async def _phase_write(
        self, instruction: str, work_dir: Path, container_work_dir: Path, filename: str
    ):
        print("  > Writing Content...")
        prompt = WRITER_PROMPT.format(instruction=instruction)

        writer = AgentWrapper.create(
            name=f"Writer-{work_dir.name}",
            instructions=prompt,
            model=self.model,
            mcp_servers=[self.runtime.server],
        )

        output_path = container_work_dir / filename
        summary_path = container_work_dir / "comprehensive_summary.md"

        await writer.run(
            f"Write the final content to {output_path}. Use {summary_path} as source.",
            max_turns=20,
        )


# --- MAIN ---


async def main():
    load_dotenv()
    add_trace_processor(AgentContentPrinter())

    config = CurriculumConfig()

    # Configuration
    api_key = os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("Error: GOOGLE_API_KEY environment variable not set.")
        return

    # Paths
    library_path = config.repository_path.resolve()
    if not library_path.exists():
        # try relative to script
        library_path = Path(__file__).parent / config.repository_path

    workspace_dir = config.get_workspace_dir()

    # Initialize Model
    model = LitellmModel(model=config.model_name, api_key=api_key)

    print(f"Initializing Queue-based Curriculum Agent with model: {config.model_name}")
    print(f"Library: {library_path}")
    print(f"Workspace: {workspace_dir}")

    # Prepare Workspace (clean start)
    if workspace_dir.exists():
        shutil.rmtree(workspace_dir)
    workspace_dir.mkdir(parents=True, exist_ok=True)
    chmod_recursive(workspace_dir)

    lib_repo_dir = workspace_dir / "repos" / "library"
    lib_repo_dir.parent.mkdir(parents=True, exist_ok=True)
    chmod_recursive(lib_repo_dir.parent)

    # Copy library
    if not lib_repo_dir.exists():
        print("Cloning library...")
        shutil.copytree(library_path, lib_repo_dir, dirs_exist_ok=True)
        chmod_recursive(lib_repo_dir)

    # Start Environment
    # Mount workspace_dir to /workspace
    async with RustCodingEnvironment(
        workspace_dir=workspace_dir, image_name="coder-mcp"
    ) as runtime:
        semaphore = asyncio.Semaphore(config.max_concurrency)
        agent = CurriculumAgent(config, model, runtime, semaphore)

        # Root Task
        root_instruction = (
            "Create a comprehensive textbook for the 'numrs' library. "
            "Start by understanding the high-level architecture and public API surface."
        )

        root_task = TaskQueueItem(
            instruction=root_instruction, work_dir=workspace_dir / "curriculum", depth=0
        )

        await agent.run_queue(root_task)

    print("\n✅ Curriculum Generation Complete!")


if __name__ == "__main__":
    asyncio.run(main())

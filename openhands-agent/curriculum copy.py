import asyncio
import os
import shutil
from pathlib import Path

from agents import ModelSettings
from agents.extensions.models.litellm_model import LitellmModel
from agents.items import TResponseInputItem
from agents.tracing import add_trace_processor
from dotenv.main import load_dotenv
from oai_utils.agent import AgentWrapper
from oai_utils.conversion import contents2params
from oai_utils.tracing import AgentContentPrinter
from pydantic import BaseModel, Field

from openhands_agent.runtime.rust_env import RustCodingEnvironment

# --- PROMPTS ---

CURRICULUM_ARCHITECT_PROMPT = """You are an expert Technical Curriculum Architect and Rust Developer.
Your goal is to design a comprehensive educational curriculum (textbook) for the 'numrs' Rust library.

<ROLE>
You are responsible for analyzing the provided codebase (`repos/library`) and creating a high-level curriculum plan.
</ROLE>

<INSTRUCTIONS>
**Iterative Explorative Planning**:
Engage in a continuous cycle of exploration and planning. You are not expected to fully explore before planning, nor plan without exploration.
Instead, let the codebase reveal itself to you. Navigate through the files (source code, examples, documentation, etc.) to understand the library's full scope.

As you explore, iteratively construct and refine your curriculum plan. If you find a new module or concept, update your plan to ensure it is covered. Conversely, use your evolving plan to direct your exploration into specific areas of the codebase.

**Goal**:
Ensure your final plan is comprehensive, covering all major features, modules, and patterns found in the library (e.g., core data structures, algorithms, error handling, interoperability, etc.).

**Output**:
Create a file named `curriculum_plan.md` in the current directory.
   - The plan should be a hierarchical table of contents (Chapters and Sections).
   - For each chapter, briefly explain what existing code/modules it covers.
   - Group related topics logically.
</INSTRUCTIONS>
"""

PLAN_PARSER_PROMPT = """You are a helper agent. 
Read the file `curriculum_plan.md`. 
Extract the list of chapters defined in the plan.
Output a JSON object with a list of chapters, where each chapter has:
- `chapter_number`: int
- `title`: str (e.g., "Core Concepts")
- `filename`: str (e.g., "01_core_concepts.md")
- `description`: str (A summary of what to cover based on the plan)

Example Output:
{
  "chapters": [
    {"chapter_number": 1, "title": "Introduction", "filename": "01_introduction.md", "description": "Cover installation and basics..."},
    ...
  ]
}

<IMPORTANT>
Output ONLY the structured data.
</IMPORTANT>
"""

CURRICULUM_EVALUATOR_PROMPT = """You are a rigorous Curriculum Evaluator.
Your goal is to ensure the generated 'curriculum_plan.md' is truly comprehensive.

<INPUT>
1. `curriculum_plan.md`: The proposed textbook plan.
2. `repos/library`: The actual source code of the library.
</INPUT>

<INSTRUCTIONS>
1. **Analyze the Codebase**: Explore the `repos/library/src` directory. Look for modules, structs, and significant features.
2. **Cross-Reference**: Check if each significant feature found in the code is covered by a Chapter or Section in the plan.
3. **Identify Gaps**: specifically look for:
    - Missing top-level modules (e.g. `src/financial` vs "Financial Math" chapter).
    - Missing advanced features (e.g. `src/cluster.rs` vs "Clustering").
    - Missing interoperability features (e.g. `python` feature flags).
4. **Evaluate**:
    - If SIGNIFICANT items are missing, set `is_comprehensive` to False and provide detailed `feedback` listing the specific missing files/modules and where they should logically fit.
    - If the plan is good and covers >95% of the codebase, set `is_comprehensive` to True and `feedback` to "Plan looks great."

<CRITICAL>
</CRITICAL>
"""

CONTENT_WRITER_PROMPT_TEMPLATE = """You are an expert Rust Technical Writer.
Your task is to write **Chapter {chapter_number}: {title}** for the NumRS2 curriculum.

<CONTEXT>
We are writing a comprehensive textbook for the `numrs` library.
You have access to the full source code in `repos/library`.
</CONTEXT>

<GOAL>
Write a high-quality, detailed markdown file named `{filename}`.
Referece the following scope from the curriculum plan:
{description}
</GOAL>

<GUIDELINES>
1. **Accuracy**: You MUST verify your code snippets. Check the actual source code in `repos/library` to ensure function signatures and module paths are correct.
2. **Examples**: Include runnable code snippets. Use `assert_eq!` or `println!` to show results.
3. **Style**: Use clear, educational language. Explain *why* things work the way they do (e.g., memory layout, broadcasting rules).
4. **Formatting**: Use standard Markdown. key terms in bold. Code blocks with `rust`.
</GUIDELINES>
"""


class Chapter(BaseModel):
    chapter_number: int = Field(description="The chapter number")
    title: str = Field(description="Title of the chapter")
    filename: str = Field(description="Filename for the chapter (e.g., '01_intro.md')")
    description: str = Field(
        description="Detailed description of what to cover in this chapter"
    )


class CurriculumPlan(BaseModel):
    chapters: list[Chapter] = Field(description="List of chapters in the curriculum")


class EvaluationResult(BaseModel):
    is_comprehensive: bool = Field(
        description="True if the plan covers all major library modules, False if items are missing"
    )
    feedback: str = Field(
        description="Detailed feedback on what is missing. Empty if comprehensive."
    )


class CurriculumConfig(BaseModel):
    curriculum_id: str = Field(
        default="generated_curriculum", description="Unique ID for this curriculum run"
    )
    model_name: str = Field(
        default="gemini/gemini-3-flash-preview", description="LLM model name"
    )
    workspace_dir: Path = Field(
        default=Path("workspace_curriculum2"),
        description="Working directory for curriculum generation",
    )
    repository_path: Path = Field(
        default=Path("repositories/numrs"),
        description="Local path to the source repository",
    )
    image_name: str = Field(
        default=os.getenv("OPENHANDS_IMAGE_NAME", "coder-mcp"),
        description="Docker image to use for the MCP server",
    )

    def get_workspace_dir(self) -> Path:
        """Resolve config workspace path relative to this script's location if not absolute."""
        return self.workspace_dir.resolve()

    def get_repository_path(self) -> Path:
        return self.repository_path.resolve()

    def save(self):
        """Save the curriculum config to a JSON file."""
        output_dir = Path("curriculums")
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / f"{self.curriculum_id}.json"
        output_path.write_text(self.model_dump_json(indent=2))
        print(f"✅ Curriculum config saved to {output_path}")

    @classmethod
    def load(cls, curriculum_id: str) -> "CurriculumConfig":
        """Load a CurriculumConfig by ID."""
        input_path = Path("curriculums") / f"{curriculum_id}.json"
        if not input_path.exists():
            raise FileNotFoundError(f"Config not found at {input_path}")
        return cls.model_validate_json(input_path.read_text())


async def generate_curriculum_plan(
    workspace_dir: Path, model: LitellmModel, runtime: RustCodingEnvironment
) -> Path:
    plan_path = workspace_dir / "curriculum_plan.md"

    if plan_path.exists():
        print(f"\n--- Phase 1: Plan found at {plan_path}, skipping generation ---")
        return plan_path

    print("\n--- Phase 1: Generating Curriculum Plan (Iterative Loop) ---")

    # 1. Generator Agent
    architect_agent = AgentWrapper.create(
        name="CurriculumArchitect",
        instructions=CURRICULUM_ARCHITECT_PROMPT,
        model=model,
        mcp_servers=[runtime.server],
        model_settings=ModelSettings(tool_choice="auto", parallel_tool_calls=True),
    )

    # 2. Evaluator Agent
    evaluator_agent = AgentWrapper[EvaluationResult].create(
        name="CurriculumEvaluator",
        instructions=CURRICULUM_EVALUATOR_PROMPT,
        model=model,
        mcp_servers=[runtime.server],
        output_type=EvaluationResult,
        model_settings=ModelSettings(tool_choice="auto", parallel_tool_calls=True),
    )

    max_iterations = 3
    current_feedback = ""
    chat_history: list[TResponseInputItem] = []

    for i in range(max_iterations):
        print(f"\n[Iteration {i + 1}/{max_iterations}] Generating Plan...")

        # Run Generator
        if current_feedback:
            prompt = (
                "The previous plan was critiqued. Please update `curriculum_plan.md` "
                f"addressing this feedback:\n\n{current_feedback}\n\n"
                "Ensure you actually cover these missing modules."
            )
        else:
            prompt = "Generate `curriculum_plan.md` by exploring the `repos/library`."
        chat_history.extend(contents2params("user", [prompt]))
        ret = await architect_agent.run(chat_history, max_turns=30)
        chat_history.extend(ret.result.to_input_list())

        if not plan_path.exists():
            print("❌ Generator failed to create curriculum_plan.md")
            continue

        # Run Evaluator
        print(f"\n[Iteration {i + 1}/{max_iterations}] Evaluating Plan...")
        eval_result = await evaluator_agent.run(
            "Compare `curriculum_plan.md` against `repos/library` and evaluate comprehensiveness.",
            max_turns=50,
        )

        result = eval_result.result.final_output
        if result.is_comprehensive:
            print("✅ Plan deemed comprehensive by Evaluator.")
            break

        print(f"⚠️ Plan gap detected: {result.feedback}")
        current_feedback = result.feedback

    return plan_path


async def main():
    load_dotenv()
    add_trace_processor(AgentContentPrinter())

    config = CurriculumConfig()
    config.save()

    # Configuration
    api_key = os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("Error: GOOGLE_API_KEY environment variable not set.")
        return

    # Paths
    # To keep backward compatibility with how the path was found relative to the script:
    script_path = Path(__file__).resolve()
    openhands_agent_dir = script_path.parent

    # Logic to find the repo if the default relative path doesn't work
    library_path = config.get_repository_path()
    if not library_path.exists():
        # Fallback to checking relative to openhands_agent directory as in original code
        fallback_path = openhands_agent_dir / config.repository_path
        if fallback_path.exists():
            library_path = fallback_path

    workspace_dir = config.get_workspace_dir()
    curriculum_out_dir = workspace_dir / "curriculum"

    # Initialize Model
    model = LitellmModel(model=config.model_name, api_key=api_key)

    print(f"Initializing Curriculum Agent with model: {config.model_name}")
    print(f"Library Path: {library_path}")
    print(f"Workspace: {workspace_dir}")

    # Prepare Workspace (Clone library into it)
    workspace_dir.mkdir(parents=True, exist_ok=True)
    curriculum_out_dir.mkdir(parents=True, exist_ok=True)

    lib_repo_dir = workspace_dir / "repos" / "library"
    if not lib_repo_dir.exists():
        print("Cloning/Copying library to workspace...")

        shutil.copytree(library_path, lib_repo_dir, dirs_exist_ok=True)

    # Start Environment
    async with RustCodingEnvironment(
        workspace_dir=workspace_dir, image_name=config.image_name
    ) as runtime:
        # --- PHASE 1: PLANNING ---
        plan_path = await generate_curriculum_plan(workspace_dir, model, runtime)

        # --- PHASE 1.5: PARSING PLAN ---
        print("\n--- Phase 1.5: Parsing Plan ---")
        parser_agent = AgentWrapper[CurriculumPlan].create(
            name="PlanParser",
            instructions=PLAN_PARSER_PROMPT,
            model=model,
            mcp_servers=[runtime.server],
            output_type=CurriculumPlan,
        )

        parse_result = await parser_agent.run(
            f"Read {plan_path.name} and output the JSON list of chapters.", max_turns=5
        )

        try:
            plan_obj = parse_result.final_output()
            chapters = plan_obj.chapters
            print(f"Parsed {len(chapters)} chapters.")
        except Exception as e:
            print(f"Failed to parse plan JSON: {e}")
            print("Raw output:", parse_result.final_output())
            return

        # --- PHASE 2: WRITING CONTENT ---
        print("\n--- Phase 2: Writing Content ---")

        for chapter in chapters:
            ch_num = chapter.chapter_number
            title = chapter.title
            filename = chapter.filename
            desc = chapter.description

            target_file = curriculum_out_dir / filename
            if target_file.exists():
                print(f"Skipping Chapter {ch_num}: {filename} (Already exists)")
                continue

            print(f"Writing Chapter {ch_num}: {title} -> {filename}...")

            # Create a fresh writer agent for each chapter to keep context clean
            writer_instruction = CONTENT_WRITER_PROMPT_TEMPLATE.format(
                chapter_number=ch_num,
                title=title,
                filename=f"curriculum/{filename}",  # Relative to workspace
                description=desc,
            )

            writer_agent = AgentWrapper.create(
                name=f"Writer_Ch{ch_num}",
                instructions=writer_instruction,
                model=model,
                mcp_servers=[runtime.server],
                # Give enough turns to explore specific files if needed
                model_settings=ModelSettings(
                    tool_choice="auto", parallel_tool_calls=True
                ),
            )

            # Trigger the writing
            await writer_agent.run(
                f"Please write the file `curriculum/{filename}`. "
                "Make sure to read the relevant source files in `repos/library` first to be accurate.",
                max_turns=20,
            )

            print(f"✅ Finished Chapter {ch_num}")


if __name__ == "__main__":
    asyncio.run(main())

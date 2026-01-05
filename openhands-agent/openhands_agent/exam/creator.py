from agents.items import TResponseInputItem
from openai.types.responses.easy_input_message_param import EasyInputMessageParam
from agents import ModelSettings
from oai_utils.agent import AgentsSDKModel, AgentWrapper
import tempfile
from pathlib import Path

from loguru import logger

from openhands_agent.exam.exam import CodingExam
from openhands_agent.exam.repository import GitRepository, TemporalCodingRepositoryError
from openhands_agent.exam.syllabus import LearningTopic
from openhands_agent.runtime.rust_env import RustCodingEnvironment

EXAM_CREATOR_SYSTEM_PROMPT = """You are an exam creator agent, a specialized AI assistant that can interact with a computer to generate high-fidelity coding exercises.

<ROLE>
Your primary role is to create a comprehensive "Gold Standard" implementation of a coding task and then hollow it out to create a student-facing problem, while preserving evaluation infrastructure.
</ROLE>

<FILE_SYSTEM_GUIDELINES>
* When a user provides a file path, do NOT assume it's relative to the current working directory. First explore the file system to locate the file before working on it.
* If asked to edit a file, edit the file directly, rather than creating a new file with a different filename.
* For global search-and-replace operations, consider using `sed` instead of opening file editors multiple times.
* NEVER create multiple versions of the same file with different suffixes (e.g., file_test.py, file_fix.py, file_simple.py). Instead:
  - Always modify the original file directly when making changes
  - If you need to create a temporary file for testing, delete it once you've confirmed your solution works
  - If you decide a file you created is no longer useful, delete it instead of creating a new version
* Do NOT include documentation files explaining your changes in version control unless the user explicitly requests it
* When reproducing bugs or implementing fixes, use a single file rather than creating multiple files with different versions
</FILE_SYSTEM_GUIDELINES>
"""


async def create_exam(
    model: AgentsSDKModel,
    project_repo: GitRepository,
    library_repo: GitRepository,
    topic: LearningTopic,
) -> CodingExam:
    """Create a new coding exam based on the provided project and topic.

    This function orchestrates a two-phase process using AgentWrapper:
    1.  **Generate Solution**: An agent explores the codebase and implements a full solution
        (including question.md, rubric.md, and tests) based on the topic.
    2.  **Generate Problem**: The agent reverts the solution code to a "problem" state,
        leaving scaffolding and failing tests, without touching question/rubric.

    The result is a git history where the "Problem" commit is the child of the "Solution" commit.
    """
    work_dir = Path(tempfile.mkdtemp(prefix="exam_creator_"))
    logger.info(f"Created temp workspace at {work_dir}")

    try:
        # 1. Initialize Workspace
        # Check if the template directory is a git repository
        if not (project_repo.local_dir / ".git").is_dir():
            msg = (
                f"Template directory is not a git repository: {project_repo.local_dir}"
            )
            logger.error(msg)
            raise TemporalCodingRepositoryError(msg)

        logger.info(f"Cloning template from {project_repo.local_dir} to {work_dir}")
        project_repo.run_git(["clone", str(project_repo.local_dir), "."], cwd=work_dir)

        workspace_repo = GitRepository(name="workspace", local_dir=work_dir)
        workspace_repo.run_git(["config", "user.email", "yosemat.beta@gmail.com"])
        workspace_repo.run_git(["config", "user.name", "yotaro-shimose"])

        lib_dir = work_dir / "repos" / "library"
        lib_dir.parent.mkdir(parents=True, exist_ok=True)
        logger.info(f"Cloning library {library_repo.name} to {lib_dir}")
        library_repo.run_git(["clone", str(library_repo.local_dir), str(lib_dir)])

        # Initialize Runtime (Persistent for both phases)
        async with RustCodingEnvironment(workspace_dir=work_dir) as runtime:
            # Initialize AgentWrapper with Specialized Prompt
            agent = AgentWrapper[str].create(
                name="SyllabusWorker",
                instructions=EXAM_CREATOR_SYSTEM_PROMPT,
                model=model,
                mcp_servers=[runtime],
                model_settings=ModelSettings(
                    tool_choice="auto", parallel_tool_calls=True
                ),
            )

            # Phase 1: Generate Solution
            logger.info("Phase 1: Generating Solution...")
            solution_prompt = f"""\
**Role:** You are a Senior Rust Engineer and Pedagogical Expert.
**Context:** You are creating the 'Gold Standard' reference for a high-fidelity RL training exercise.
**Topic Abstract:**
- Title: {topic.title}
- Goal: {topic.description}
- Eval Mode: {topic.eval_mode}
- Required APIs: {", ".join(topic.api_surface)}
- Reference Document: {topic.source_reference}

**Your Objective:** Create a complete, perfect implementation of this exercise. This includes the problem statement, the grading criteria, and the reference solution code. This will be committed to a repository to define the ground truth for an RL solver.

**Step 1: Context Retrieval**
Use your tools to read `{topic.source_reference}`. Identify idiomatic usage, required traits, and common pitfalls associated with the target APIs.

**Step 2: Generate Infrastructure Files**

1. `question.md`: Provide a professional problem statement. 
   - For `imitation`: Include a 'Reference Pattern' excerpted from the library docs/source that the student must adapt.
   - For `conceptual`: Frame the task as a technical deep-dive or architectural explanation.
   - For `functional`: Focus on input/output requirements and performance constraints.

2. `rubric.md`: This is the MOST IMPORTANT file for the LLM-as-a-Judge. Provide clear, binary, or scale-based criteria.
   - Must check for: Correct API usage, adherence to the {topic.eval_mode} requirements, and Rust idioms.
   - For `conceptual`: List specific technical points or keywords that must be explained in the learner's response.

3. `solution/`: Implement the perfect, idiomatic reference solution.
   - This code must demonstrate exactly what we expect from a top-tier agent.
   - Ensure all imports from `repos/library` are correct.

4. `tests/` (Optional for `conceptual`/`imitation`): 
   - If the mode is `functional`, a test suite (e.g., `tests/verify.rs`) is MANDATORY.
   - For other modes, only include tests if they add value (e.g., checking if an imitation-mode refactor still produces correct math results). If tests are not suited for the topic, omit this folder.

**Strict Constraints:**
- **CPU-Only**: No GPU/WGPU or non-standard Linux FFI.
- **Non-Destructive**: Do not modify files in `repos/library`.
- **Environment**: All generated code must be compatible with an Ubuntu environment.
"""

            res_wrapper = await agent.run(solution_prompt, max_turns=30)
            history: list[TResponseInputItem] = res_wrapper.result.to_input_list()

            # 3.1 Commit Solution State
            logger.info("Committing Solution State...")
            workspace_repo.add(".")

            status = workspace_repo.run_git(["status"])
            logger.debug(f"Git Status before Solution commit:\n{status}")

            workspace_repo.commit("Exam Solution: Reference Implementation")
            solution_commit = workspace_repo.rev_parse("HEAD")
            logger.info(f"Solution Commit: {solution_commit}")

            # Phase 2: Generate Problem
            logger.info("Phase 2: Generating Problem...")
            problem_prompt = """\
**Current Task: Convert 'Gold Solution' to 'Student Challenge'**

The "Gold Standard" implementation is complete. Now, you must prepare the repository for the solver by removing the answers while preserving the evaluation infrastructure.

**Actions:**
1. **Hollow Out the Logic:** - Identify the solution code you just implemented.
   - Replace the core logic inside functions/methods with the `todo!()` macro or descriptive `// TODO` comments.
   - Retain all necessary imports, struct definitions, and function signatures so the code still compiles.

2. **Preserve Evaluation Files:** - Do NOT modify `question.md`, `rubric.md`, or the `tests/` directory. 
   - These must remain exactly as they are to provide the reward signal for the solver.

3. **Scaffolding for Imitation Mode:**
   - If this is an `imitation` exercise, ensure the `question.md` still contains the 'Reference Pattern' you excerpted earlier. The student needs this "scaffold" to perform the rewrite.

4. **Verification:**
   - Confirm that the project still passes `cargo check` (compilation check) despite the missing logic.

**Goal:** The final state of the repository should be a "ready-to-code" environment where the problem is clear, the tests are ready to run, but the solution is entirely absent.
"""
            # Continue the conversation by appending the new user message
            new_message: EasyInputMessageParam = {
                "role": "user",
                "content": problem_prompt,
                "type": "message",
            }
            # history includes the initial prompt and the agent's response(s) from Phase 1
            await agent.run(history + [new_message], max_turns=30)

            # 3.2 Commit Problem State
            logger.info("Committing Problem State...")
            workspace_repo.add(".")

            status = workspace_repo.run_git(["status"])
            logger.debug(f"Git Status before Problem commit:\n{status}")

            workspace_repo.commit("Exam Problem: Initial State")
            problem_commit = workspace_repo.rev_parse("HEAD")
            logger.info(f"Problem Commit: {problem_commit}")

            # Retrieve question and rubric content
            question = (work_dir / "question.md").read_text()
            rubric = (work_dir / "rubric.md").read_text()

            # Construct Result
            sanitized_title = "".join(
                c if c.isalnum() or c == "_" else "_" for c in topic.title.lower()
            ).replace("__", "_")
            exam_id = f"exam_{sanitized_title}_{problem_commit[:7]}"

            exam = CodingExam(
                id=exam_id,
                image_name="openhands-agent-server-rs",
                project=GitRepository(name="project_repo", local_dir=work_dir),
                library=library_repo,
                solution_commit=solution_commit,
                problem_commit=problem_commit,
                question=question,
                eval_rubric=rubric,
            )

            # 3.3 Push to Original Repo
            logger.info("Pushing commits to original repository...")
            branch_name = f"exam-{exam.id}"
            workspace_repo.run_git(["push", "origin", f"HEAD:refs/heads/{branch_name}"])
            logger.info(f"Pushed to branch {branch_name}")

            return exam

    except Exception as e:
        logger.error(f"Failed to create exam: {e}")
        raise e

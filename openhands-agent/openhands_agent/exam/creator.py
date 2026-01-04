import shutil
import tempfile
from pathlib import Path

from loguru import logger

from openhands_agent.agent import OpenHandsAgent
from openhands_agent.exam.exam import CodingExam
from openhands_agent.exam.repository import GitRepository
from openhands_agent.exam.topic import Topic
from openhands_agent.runtime.rust_env import RustCodingEnvironment


async def create_exam(
    project_repo: GitRepository,
    library_repo: GitRepository,
    topic: Topic,
) -> CodingExam:
    """Create a new coding exam based on the provided project and topic.

    This function orchestrates a two-phase process:
    1.  **Generate Solution**: An agent explores the codebase and implements a full solution
        (including question.md, rubric.md, and tests) based on the topic.
    2.  **Generate Problem**: The agent reverts the solution code to a "problem" state,
        leaving scaffolding and failing tests, without touching question/rubric.

    The result is a git history where the "Problem" commit is the parent of the "Solution" commit.
    """
    # Create a temporary directory for the workspace
    # We use a persistent temp dir so it survives the function call if needed,
    # but practically we might want to cleanup or move it.
    # For now, let's create a new one.
    work_dir = Path(tempfile.mkdtemp(prefix="exam_creator_"))
    logger.info(f"Created temp workspace at {work_dir}")

    try:
        # Phase 0: Setup
        # Clone project to workspace root
        logger.info(f"Cloning project {project_repo.name} to {work_dir}")
        project_repo.run_git(["clone", str(project_repo.local_dir), "."], cwd=work_dir)
        workspace_repo = GitRepository(name="exam_workspace", local_dir=work_dir)

        # Configure git user for commits
        workspace_repo.run_git(["config", "user.name", "OpenHands Exam Creator"])
        workspace_repo.run_git(["config", "user.email", "creator@openhands.ai"])

        # Clone library to repos/library
        lib_dir = work_dir / "repos" / "library"
        lib_dir.parent.mkdir(parents=True, exist_ok=True)
        logger.info(f"Cloning library {library_repo.name} to {lib_dir}")
        library_repo.run_git(["clone", str(library_repo.local_dir), str(lib_dir)])

        # Initialize Runtime
        async with RustCodingEnvironment(workspace_dir=work_dir) as runtime:
            agent = OpenHandsAgent(mcp_server=runtime)

            # Phase 1: Generate Solution
            logger.info("Phase 1: Generating Solution...")
            solution_prompt = (
                f"You are an expert Rust developer and exam creator.\n"
                f"Your Task: Create a coding exam based on the topic: '{topic.title}'\n"
                f"Description: {topic.description}\n\n"
                f"Instructions:\n"
                f"1. Explore the codebase to understand the context.\n"
                f"2. Create a new feature or fix a bug related to the topic.\n"
                f"3. Create a `question.md` file describing the problem clearly to a student.\n"
                f"4. Create a `rubric.md` file with evaluation criteria.\n"
                f"5. Implement the FULL solution code.\n"
                f"6. Add a test file (e.g., `tests/exam_test.rs`) that verifies the solution. "
                "The tests MUST PASS with your solution.\n\n"
                "IMPORTANT: The library code is available in `repos/library`.\n"
                "You MUST use this library for your solution (e.g. via `include!` or by adding it to Cargo.toml if you can).\n"
                "You can refer to it but DO NOT modify any files in `repos/library`."
            )

            # We enforce a high max_turns to allow for exploration and implementation
            result = await agent.run(solution_prompt, max_turns=30)
            history = result.to_input_list()

            # Verify tests pass (optional but good sanity check)
            # In a real impl, we might check result.final_output or run a validation step.

        # Snapshot Solution State
        # Copy everything to a backup
        solution_backup = Path(tempfile.mkdtemp(prefix="exam_solution_backup_"))
        shutil.copytree(work_dir, solution_backup, dirs_exist_ok=True)
        logger.info(f"Backed up solution state to {solution_backup}")

        # Phase 2: Generate Problem
        # Re-initialize runtime (fresh agent state recommended for clean context)
        async with RustCodingEnvironment(workspace_dir=work_dir) as runtime:
            agent = OpenHandsAgent(mcp_server=runtime)
            logger.info("Phase 2: Generating Problem...")
            problem_prompt = (
                "You are now preparing the problem state for the student.\n"
                "Your Task: Revert the solution code to a starting state.\n\n"
                "Instructions:\n"
                "1. Remove the implementation details of the feature/fix you just created, "
                "leaving only function signatures/struct definitions (stubs).\n"
                "2. Ensure the test file (`tests/exam_test.rs`) REMAINS but fails (compiles but asserts fail, or 'todo!()').\n"
                "3. DO NOT modify `question.md` or `rubric.md`. They must stay as is.\n"
                "4. Remove any other temporary files if you created them."
            )

            # Continue the conversation by appending the new user message
            new_message = {
                "role": "user",
                "content": problem_prompt,
                "type": "message",
            }
            # history includes the initial prompt and the agent's response(s) from Phase 1
            await agent.run(history + [new_message])

        # Phase 3: Finalize Git History

        # 3.1 Commit Problem State
        logger.info("Committing Problem State...")
        workspace_repo.add(".")

        # DEBUG: Check status before commit
        status = workspace_repo.run_git(["status"])
        logger.debug(f"Git Status before Problem commit:\n{status}")

        workspace_repo.commit("Exam Problem: Initial State")
        problem_commit = workspace_repo.rev_parse("HEAD")
        logger.info(f"Problem Commit: {problem_commit}")

        # 3.2 Restore Solution State on top
        # We delete current files and restore from backup to ensure we have the EXACT solution state
        # (excluding .git to preserve the problem commit history)
        logger.info("Restoring Solution State...")
        for item in work_dir.iterdir():
            if item.name != ".git":
                if item.is_dir():
                    shutil.rmtree(item)
                else:
                    item.unlink()

        for item in solution_backup.iterdir():
            if item.name != ".git":
                dest = work_dir / item.name
                if item.is_dir():
                    shutil.copytree(item, dest)
                else:
                    shutil.copy2(item, dest)

        # 3.3 Commit Solution State
        logger.info("Committing Solution State...")
        workspace_repo.add(".")

        # DEBUG: Check status before commit
        status = workspace_repo.run_git(["status"])
        logger.debug(f"Git Status before Solution commit:\n{status}")

        workspace_repo.commit("Exam Solution: Reference Implementation")
        solution_commit = workspace_repo.rev_parse("HEAD")
        logger.info(f"Solution Commit: {solution_commit}")

        # Cleanup backup
        shutil.rmtree(solution_backup)

        # Retrieve question and rubric content
        question = (work_dir / "question.md").read_text()
        rubric = (work_dir / "rubric.md").read_text()

        # Construct result (temporarily to get the ID for the branch name)
        # The full exam object will be constructed again later with the same ID.
        exam_id = f"exam_{topic.title.lower().replace(' ', '_')}_{problem_commit[:7]}"

        # Construct result
        exam = CodingExam(
            id=exam_id,  # Use the pre-calculated ID
            image_name="openhands-agent-server-rs",  # Default for now
            project=GitRepository(
                name="project_repo", local_dir=work_dir
            ),  # The workspace IS the new repo
            library=library_repo,  # Original library ref
            solution_commit=solution_commit,
            problem_commit=problem_commit,
            question=question,
            eval_rubric=rubric,
        )

        # 3.4 Push to Original Repo
        logger.info("Pushing commits to original repository...")
        branch_name = f"exam-{exam.id}"
        # Push with force to ensure we create/update the branch
        workspace_repo.run_git(["push", "origin", f"HEAD:refs/heads/{branch_name}"])
        logger.info(f"Pushed to branch {branch_name}")

        # Note: solution_commit is HEAD, problem_commit is its ancestor.
        # Both are now available in the remote repo under that branch.

        # NOTE: The caller is responsible for moving `work_dir` (the new repo)
        # to a permanent location if desired, as `exam.project` points to it.

        return exam

    except Exception as e:
        logger.error(f"Failed to create exam: {e}")
        # Cleanup on failure?
        # shutil.rmtree(work_dir)
        raise e

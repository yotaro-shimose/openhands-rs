import subprocess
import tempfile
from pathlib import Path
from typing import Optional

from loguru import logger

from openhands_agent.agent import AgentConfig, OpenHandsAgent
from openhands_agent.exam.exam import CodingExam
from openhands_agent.exam.repository import GitRepository
from openhands_agent.runtime.rust_env import RustCodingEnvironment


async def solve_exam(exam: CodingExam, config: Optional[AgentConfig] = None) -> Path:
    """
    Solves the given exam by running an agent in a temporary environment.
    Returns the path to the temporary workspace containing the solution.
    """
    # Create temp workspace
    work_dir = Path(tempfile.mkdtemp(prefix="exam_solve_"))
    logger.info(f"Created temp workspace for solution at {work_dir}")

    try:
        # Clone project repo
        logger.info(f"Cloning exam project to {work_dir}")
        subprocess.run(
            ["git", "clone", str(exam.project.local_dir), str(work_dir)],
            check=True,
            capture_output=True,
        )

        # Initialize GitRepository for the workspace
        workspace_repo = GitRepository(name="solve_workspace", local_dir=work_dir)

        # Config User
        workspace_repo.run_git(["config", "user.name", "OpenHands Exam Solver"])
        workspace_repo.run_git(["config", "user.email", "solver@openhands.ai"])

        # Checkout problem commit
        logger.info(f"Checking out problem commit: {exam.problem_commit}")
        workspace_repo.run_git(["checkout", exam.problem_commit])

        # Initialize Runtime
        async with RustCodingEnvironment(workspace_dir=work_dir) as runtime:
            # Use provided config or default
            agent = OpenHandsAgent(mcp_server=runtime, config=config)

            # Construct Prompt
            prompt = (
                f"You are taking a coding exam.\n\n"
                f"Question:\n{exam.question}\n\n"
                f"Please solve the problem by editing the files in the current directory.\n"
                f"Your solution must pass all provided tests (e.g. `cargo test`).\n"
            )

            logger.info("Starting agent to solve exam...")
            # We assume a reasonable max_turns if not specified in config
            if config and config.max_iterations:
                max_turns = config.max_iterations
            else:
                max_turns = 30

            await agent.run(prompt, max_turns=max_turns)

        return work_dir

    except Exception as e:
        logger.error(f"Failed to solve exam: {e}")
        raise e


async def evaluate_exam(
    exam: CodingExam, workspace_path: Path, config: Optional[AgentConfig] = None
) -> str:
    """
    Evaluates a solution in the given workspace against the exam rubric.
    Returns the evaluation report as a string.
    """
    logger.info(f"Evaluating exam solution at {workspace_path}")

    try:
        # Initialize Runtime on the existing solution workspace
        async with RustCodingEnvironment(workspace_dir=workspace_path) as runtime:
            # Use provided config or default
            agent = OpenHandsAgent(mcp_server=runtime, config=config)

            # Construct Prompt
            prompt = (
                f"You are a strict exam grader.\n\n"
                f"Your Task: Evaluate the student's solution in the current directory against the provided rubric.\n\n"
                f"Question:\n{exam.question}\n\n"
                f"Rubric:\n{exam.eval_rubric}\n\n"
                f"Instructions:\n"
                f"1. Run the tests (e.g. `cargo test`) to ensure correctness.\n"
                f"2. Inspect the code to check for specific requirements, code style, and potential cheating.\n"
                f"3. Provide a detailed report with points awarded for each rubric item.\n"
                f"4. Conclude with a 'TOTAL USER SCORE: <score>/<total>' line.\n"
            )

            logger.info("Starting agent to evaluate exam...")
            # Evaluation might not take as many turns as solving
            result = await agent.run(prompt, max_turns=15)

            return result.final_output or "No evaluation report generated."

    except Exception as e:
        logger.error(f"Failed to evaluate exam: {e}")
        raise e

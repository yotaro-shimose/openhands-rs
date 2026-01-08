from pydantic import BaseModel
from oai_utils.agent import AgentWrapper
from oai_utils.agent import AgentsSDKModel
import subprocess
import tempfile
from pathlib import Path

from loguru import logger

from openhands_agent.agent import OpenHandsAgent
from openhands_agent.exam.exam import CodingExam
from openhands_agent.exam.repository import GitRepository
from openhands_agent.runtime.rust_env import RustCodingEnvironment


async def solve_exam(model: AgentsSDKModel, exam: CodingExam) -> Path:
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
        workspace_repo = GitRepository(local_dir=work_dir)

        # Config User
        workspace_repo.run_git(["config", "user.name", "OpenHands Exam Solver"])
        workspace_repo.run_git(["config", "user.email", "solver@openhands.ai"])

        # Checkout problem commit
        logger.info(f"Checking out problem commit: {exam.problem_commit}")
        workspace_repo.run_git(["checkout", exam.problem_commit])

        # Initialize Runtime
        async with RustCodingEnvironment(workspace_dir=work_dir) as runtime:
            # Use provided config or default
            agent = OpenHandsAgent.create(model=model, mcp_server=runtime)

            # Construct Prompt
            prompt = (
                f"You are taking a coding exam.\n\n"
                f"Question:\n{exam.question}\n\n"
                f"Please solve the problem by editing the files in the current directory.\n"
                f"Your solution must pass all provided tests (e.g. `cargo test`).\n"
            )

            logger.info("Starting agent to solve exam...")
            await agent.run(prompt, max_turns=30)

        return work_dir

    except Exception as e:
        logger.error(f"Failed to solve exam: {e}")
        raise e


class EvaluationResponse(BaseModel):
    description: str
    score: int  # out of 100


async def evaluate_exam(
    model: AgentsSDKModel, exam: CodingExam, workspace_path: Path
) -> EvaluationResponse:
    """
    Evaluates a solution in the given workspace against the exam rubric.
    Returns the evaluation report as an EvaluationResponse.
    """
    logger.info(f"Evaluating exam solution at {workspace_path}")
    prompt = f"""\
**Role:** You are a Principal Software Engineer and Technical Auditor.
**Context:** You are evaluating a student's solution for a coding exam within a sandboxed Rust environment.

**Evaluation Materials:**
1. **The Question:**
---
{exam.question}
---

2. **The Grading Rubric:**
---
{exam.eval_rubric}
---

**Your Task:**
Perform a rigorous, multi-stage audit of the current workspace to determine the student's final score.

**Step 1: Functional Verification**
- Attempt to compile the project (`cargo check`).
- Execute all unit tests (`cargo test`). 
- Note: If the question is purely `conceptual` (documentation-based), skip code execution and focus on text analysis.

**Step 2: Library Integrity Check**
- The student was provided the library as a dependency.
- Verify that the student correctly utilized the library APIs as requested in the question.
- Check for "workarounds" (e.g., using `std` vectors when `numrs2::Array` was required).

**Step 3: Rubric Analysis**
- Grade the submission point-by-point against the provided **Grading Rubric**.
- Be objective: Award partial credit only where the rubric explicitly allows it.
- Identify any "Negative Criteria" (e.g., performance bottlenecks or non-idiomatic Rust).

**Step 4: Report Generation**
Your final output must contain:
1. **Summary:** A brief overview of the student's performance.
2. **Audit Log:** Results of compilation and test runs.
3. **Rubric Scoring:** A list showing points earned for each rubric item.
4. **Final Verdict:** You must end your response with exactly one line in this format:
   `TOTAL USER SCORE: <score>/100`

**Constraint:** Do not be lenient. This evaluation is used for Reinforcement Learning training; a precise and consistent reward signal is mandatory.
"""
    try:
        # Initialize Runtime on the existing solution workspace
        async with RustCodingEnvironment(workspace_dir=workspace_path) as runtime:
            agent = AgentWrapper[EvaluationResponse].create(
                name="exam_evaluator",
                instructions=prompt,
                model=model,
                mcp_servers=[runtime],
                output_type=EvaluationResponse,
            )

            logger.info("Starting agent to evaluate exam...")
            result = await agent.run("Now start evaluation", max_turns=60)
            return result.final_output()

    except Exception as e:
        logger.error(f"Failed to evaluate exam: {e}")
        raise e


4

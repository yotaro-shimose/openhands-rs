import asyncio
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from agents.extensions.models.litellm_model import LitellmModel
from agents.tracing import add_trace_processor
from dotenv.main import load_dotenv
from loguru import logger
from oai_utils.tracing import AgentContentPrinter

from openhands_agent.exam.exam import CodingExam
from openhands_agent.exam.runner import evaluate_exam
from openhands_agent.exam.repository import GitRepository
from benchmark import BenchmarkConfig


async def main():
    load_dotenv()

    # Configure Model
    model_name = "gemini/gemini-3-flash-preview"
    model = LitellmModel(model=model_name, api_key=os.environ["GOOGLE_API_KEY"])

    # Enable tracing
    add_trace_processor(AgentContentPrinter())

    # Load benchmark config
    try:
        config = BenchmarkConfig.load("test_numrs_syllabus2")
    except FileNotFoundError:
        logger.error("Benchmark config not found. Please run benchmark.py first.")
        return

    # Load exam from checkpoint
    project_dir = config.get_project_dir()
    exams_path = project_dir / "exams.json"

    exams_data = json.loads(exams_path.read_text())
    if not exams_data:
        logger.error("No exams found in exams.json")
        return

    # Take the first exam for evaluation
    exam_dict = exams_data[0]
    exam = CodingExam.model_validate(exam_dict)

    logger.info(f"Evaluating gold solution for exam: {exam.id}")

    # Create a random temporary directory for evaluation
    with tempfile.TemporaryDirectory(prefix="gold_eval_") as temp_dir:
        temp_path = Path(temp_dir)
        logger.info(f"Using temporary directory: {temp_path}")

        # Clone the template directory to the temp directory
        # exam.project.local_dir points to the original template
        logger.info(f"Cloning {exam.project.local_dir} to {temp_path}")

        subprocess.run(
            ["git", "clone", str(exam.project.local_dir), str(temp_path)],
            check=True,
            capture_output=True,
        )

        # Clean up any existing bash_events from the template to avoid permission issues
        bash_events_dir = temp_path / "bash_events"
        if bash_events_dir.exists():
            logger.info(f"Removing existing bash_events directory at {bash_events_dir}")
            shutil.rmtree(bash_events_dir)

        # Initialize GitRepository for the cloned workspace
        workspace_repo = GitRepository(name="gold_workspace", local_dir=temp_path)

        # Checkout the gold solution commit
        logger.info(f"Checking out gold solution commit: {exam.solution_commit}")
        workspace_repo.run_git(["checkout", exam.solution_commit])

        # The template now contains a pre-configured Cargo.toml with registry dependencies.
        # No need to copy library or create Cargo.toml manually.
        logger.info(
            "Relying on template boilerplate (Cargo.toml + registry dependencies)"
        )

        # Evaluate the exam
        evaluation = await evaluate_exam(
            model=model,
            exam=exam,
            workspace_path=temp_path,
        )

        print("\n=== Evaluation Result ===")
        print(f"Score: {evaluation.score}/100")
        print(f"Description:\n{evaluation.description}")

        if evaluation.score == 100:
            print("\nSUCCESS: Observed perfect score for gold standard solution.")
        else:
            print(f"\nWARNING: Score is {evaluation.score}, expected 100.")


if __name__ == "__main__":
    asyncio.run(main())

import asyncio
import os
import sys
import json
from pathlib import Path

from dotenv.main import load_dotenv
from agents.tracing import add_trace_processor
from openhands_agent.tracing import AgentContentPrinter
from agents.extensions.models.litellm_model import LitellmModel
from openhands_agent.exam.repository import GitRepository
from openhands_agent.exam.pipeline import generate_exercises, generate_exams

# Enable tracing to see agent activity
add_trace_processor(AgentContentPrinter())


async def main():
    load_dotenv()

    # Configure Model
    model = LitellmModel(
        model="gemini/gemini-3-flash-preview", api_key=os.environ["GOOGLE_API_KEY"]
    )

    # Work Dir / Output Project
    project_dir = Path("projects/test_numrs_syllabus2").resolve()
    project_dir.mkdir(parents=True, exist_ok=True)

    # Template for EXAM creation
    exam_template = GitRepository(
        name="exam_template", local_dir=Path("../../templates/rust_exam").resolve()
    )

    # Source Library definition
    numrs_path = Path("repositories/numrs").resolve()
    if not numrs_path.exists():
        print(f"Error: {numrs_path} does not exist.")
        return

    numrs = GitRepository(name="numrs", local_dir=numrs_path)

    # Phase 1: Generate Exercises
    print("=== Phase 1: Generating Exercises ===")

    # Check if exercises.json already exists to avoid re-running Phase 1 if not needed
    exercises_path = project_dir / "exercises.json"
    exercises = []

    if exercises_path.exists():
        print(f"Found existing exercises in {exercises_path}. Loading...")
        try:
            from openhands_agent.exam.syllabus import Exercise

            with open(exercises_path, "r") as f:
                data = json.load(f)
                exercises = [Exercise(**item) for item in data]
            print(f"Loaded {len(exercises)} exercises.")
        except Exception as e:
            print(f"Failed to load existing exercises: {e}")
            exercises = []

    if not exercises:
        # Generate new if not present (increased limit to batch more)
        exercises = await generate_exercises(model, numrs, project_dir, limit=5)

        if not exercises:
            print("No exercises generated. Exiting.")
            return

        # Save exercises list as JSON
        with open(exercises_path, "w") as f:
            f.write(json.dumps([ex.model_dump() for ex in exercises], indent=2))
        print(f"Saved {len(exercises)} exercises to {exercises_path}")

    # Phase 2: Generate Exams
    print("\n=== Phase 2: Generating Exams ===")

    # Limit to 10 as requested
    target_exercises = exercises[:10]
    print(f"Targeting {len(target_exercises)} exercises for exam generation.")

    exams = await generate_exams(
        model=model,
        exercises=target_exercises,
        library_repository=numrs,
        exam_template=exam_template,
        work_dir=project_dir,
    )

    # Save exams list as JSON
    if exams:
        exams_path = project_dir / "exams.json"
        with open(exams_path, "w") as f:
            f.write(json.dumps([ex.model_dump(mode="json") for ex in exams], indent=2))
        print(f"Saved {len(exams)} exams to {exams_path}")

    print(f"\nPipeline Complete. Generated {len(exams)} exams.")
    if exams:
        sys.exit(0)


if __name__ == "__main__":
    asyncio.run(main())

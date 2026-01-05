import asyncio
import json
import traceback
from pathlib import Path
from typing import Optional

from oai_utils.agent import AgentsSDKModel, AgentWrapper
from openhands_agent.runtime.docker_runtime import DockerRuntime
from openhands_agent.exam.repository import GitRepository
from openhands_agent.exam.creator import create_exam
from openhands_agent.exam.topic import Topic
from openhands_agent.exam.syllabus import (
    CurriculumAbstract,
    Exercise,
    SYLLABUS_WORKER_PROMPT,
)
from openhands_agent.exam.exam import CodingExam


# --- Helpers ---
def list_rust_files(repo_path: Path) -> list[Path]:
    """Recursively list all .rs files in src/ directory."""
    src_dir = repo_path / "src"
    if not src_dir.exists():
        return []
    return list(src_dir.rglob("*.rs"))


def save_abstract_as_markdown(
    abstract: CurriculumAbstract, output_dir: Path, original_file: Path
):
    """Save the abstract as multiple Markdown files, one per exercise."""
    for ex in abstract.exercises:
        # Use filename derived from original file and exercise topic ID
        filename = f"{original_file.stem}_{ex.id}.md"
        # Basic sanitization
        filename = "".join(c for c in filename if c.isalnum() or c in ("_", "-", "."))

        output_path = output_dir / filename

        content = f"""---
title: "{ex.topic}"
description: "Exercise for {original_file.name}: {ex.concept}"
---

# {ex.topic}

## Exercise Details
- **ID:** {ex.id}
- **Concept:** {ex.concept}
- **Rationale:** {ex.rationale}
- **Complexity:** {ex.complexity}
- **API Surface:** {", ".join(ex.api_surface)}
- **Source Reference:** {ex.source_reference}

"""
        output_path.write_text(content)
        print(f"Saved exercise to {output_path}")


def save_exam_metadata(exam: CodingExam, output_dir: Path):
    """Save CodingExam metadata to JSON."""
    output_path = output_dir / f"{exam.id}.json"
    with open(output_path, "w") as f:
        f.write(exam.model_dump_json(indent=2))
    print(f"Saved exam metadata to {output_path}")


def exercise_to_topic(ex: Exercise) -> Topic:
    """Convert a generated Exercise into a Topic for exam creation."""
    description = (
        f"Concept: {ex.concept}\n\n"
        f"Rationale: {ex.rationale}\n\n"
        f"Complexity: {ex.complexity}\n"
        f"Target API: {', '.join(ex.api_surface)}\n"
        f"Reference: {ex.source_reference}"
    )
    return Topic(title=ex.topic, description=description)


# --- Phases ---
async def generate_exercises(
    model: AgentsSDKModel,
    library_repository: GitRepository,
    work_dir: Path,
    limit: int = 3,
) -> list[Exercise]:
    """
    Phase 1: Discover Rust files, generate curriculum abstracts and exercises.
    """
    work_dir.mkdir(parents=True, exist_ok=True)

    # Clone library into repositories/numrs (needed for agent context)
    lib_dir = work_dir / "repositories" / "numrs"
    if not lib_dir.exists() and library_repository.exists:
        lib_dir.parent.mkdir(parents=True, exist_ok=True)
        try:
            # We assume git is available in the environment
            import subprocess

            subprocess.run(
                ["git", "clone", str(library_repository.local_dir), str(lib_dir)],
                check=True,
            )
        except Exception as e:
            print(f"Clone failed (might already exist or error): {e}")

    # Output directory for abstracts
    abstracts_dir = work_dir / "curriculum" / "abstracts"
    abstracts_dir.mkdir(parents=True, exist_ok=True)

    # 1. Discovery
    all_files = list_rust_files(library_repository.local_dir)
    print(f"Discovered {len(all_files)} Rust files.")

    # Filter/Limit for experiment
    target_files = [
        f for f in all_files if "mod.rs" not in f.name and "lib.rs" not in f.name
    ]
    if not target_files:
        target_files = all_files

    # Take 'limit' for experiment
    experiment_batch = target_files[:limit]
    print(
        f"Processing batch of {len(experiment_batch)} files: {[f.name for f in experiment_batch]}"
    )

    collected_exercises = []

    async with DockerRuntime(workspace_dir=str(work_dir)) as mcp_server:
        for rust_file in experiment_batch:
            print(f"--- Analyzing {rust_file.name} ---")

            # Read content
            try:
                code_content = rust_file.read_text()
            except Exception as e:
                print(f"Skipping {rust_file.name}: {e}")
                continue

            # Prepare Input
            user_msg = f"File Path: {rust_file.relative_to(library_repository.local_dir)}\n\nCode:\n```rust\n{code_content}\n```"

            # Create Ephemeral Agent
            agent = AgentWrapper[CurriculumAbstract].create(
                name=f"SyllabusWorker-{rust_file.stem}",
                instructions=SYLLABUS_WORKER_PROMPT,
                model=model,
                mcp_servers=[mcp_server],
                output_type=CurriculumAbstract,
            )

            # Run
            try:
                result_wrapper = await agent.run(user_msg, max_turns=5)
                abstract = result_wrapper.result.final_output

                if isinstance(abstract, CurriculumAbstract):
                    save_abstract_as_markdown(abstract, abstracts_dir, rust_file)
                    collected_exercises.extend(abstract.exercises)
                else:
                    print(
                        f"Agent failed to return structured output for {rust_file.name}"
                    )

            except Exception as e:
                print(f"Error processing {rust_file.name}: {e}")

    print(f"Generated {len(collected_exercises)} exercises.")
    return collected_exercises


async def generate_exams(
    model: AgentsSDKModel,
    exercises: list[Exercise],
    library_repository: GitRepository,
    exam_template: GitRepository,
    work_dir: Path,
    push_to_origin: bool = True,
) -> list[CodingExam]:
    """
    Phase 2: Generate CodingExams for the provided exercises.
    """
    specs_dir = work_dir / "exams"
    specs_dir.mkdir(parents=True, exist_ok=True)

    generated_exams = []

    # Iterate over exercises
    for ex in exercises:
        print(f"Creating exam for exercise: {ex.id} ({ex.topic})")
        topic = exercise_to_topic(ex)

        try:
            # Use exam_template as the base project_repo for exams
            exam = await create_exam(
                model=model,
                project_repo=exam_template,
                library_repo=library_repository,
                topic=topic,
            )
            print(f"Exam created successfully: {exam.id}")

            # Save Metadata
            save_exam_metadata(exam, specs_dir)
            generated_exams.append(exam)

            if push_to_origin:
                # Push to Original Repo (origin)
                branch_name = f"exam/{ex.id}"
                print(f"Pushing result to branch '{branch_name}' in template repo...")

                # exam.project is the temp repo. Its 'origin' is the exam_template.
                try:
                    exam.project.run_git(
                        ["push", "origin", f"HEAD:refs/heads/{branch_name}"]
                    )
                    print(f"✅ Successfully pushed exam to branch: {branch_name}")

                except Exception as push_err:
                    print(f"❌ Failed to push to origin: {push_err}")

        except Exception as exam_error:
            # Log error including traceback, then continue to the next exercise
            print(f"Failed to create exam for {ex.id}: {exam_error}")
            traceback.print_exc()
            continue

    return generated_exams

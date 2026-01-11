import asyncio
import os
import uuid
from pathlib import Path

from agents.extensions.models.litellm_model import LitellmModel
from oai_utils.agent import AgentWrapper
from pydantic import BaseModel, Field

from openhands_agent.async_util import gather_with_semaphore
from openhands_agent.runtime.rust_env import RustCodingEnvironment
from openhands_agent.runtime.temp_workspace import TempWorkspace

# --- PYDANTIC MODELS ---


class HindsightConfig(BaseModel):
    experiment_id: str = Field(
        default="experiment_generic", description="ID for the experiment/output folder"
    )
    model_name: str = Field(
        default="gemini/gemini-3-flash-preview", description="LLM model name"
    )
    # Using relative paths assuming execution from project root
    library_path: Path = Field(
        default=Path("repositories/numrs"),
        description="Path to the source library to learn from",
    )
    curriculum_dir: Path = Field(
        default=Path("workspace_curriculum/curriculum"),
        description="Path to the generated curriculum markdown files",
    )
    boilerplate_dir: Path = Field(
        default=Path("templates/rust_boilerplate"),
        description="Path to the rust boilerplate template",
    )
    output_base_dir: Path = Field(
        default=Path("data/qra"),
        description="Base directory for outputting generated QRA data",
    )
    image_name: str = Field(
        default=os.getenv("OPENHANDS_IMAGE_NAME", "coder-mcp"),
        description="Docker image to use",
    )
    max_concurrent_tasks: int = Field(
        default=5, description="Number of concurrent generation agents"
    )

    def get_library_path(self) -> Path:
        return self.library_path.resolve()

    def get_curriculum_dir(self) -> Path:
        return self.curriculum_dir.resolve()

    def get_boilerplate_dir(self) -> Path:
        return self.boilerplate_dir.resolve()

    def get_output_dir(self) -> Path:
        return (self.output_base_dir / self.experiment_id).resolve()


class Teachable(BaseModel):
    slug: str = Field(
        description="A short, url-friendly identifier (e.g., 'broadcast_ops')."
    )
    description: str = Field(
        description="A sentence describing what the user should learn."
    )


class TeachablesList(BaseModel):
    items: list[Teachable] = Field(description="List of extracted teachables.")


class QRAContent(BaseModel):
    question: str = Field(description="The Markdown question text.")
    reasoning: str = Field(
        description="Internal monologue explaining the verification and thought process."
    )
    answer: str = Field(description="The final natural language answer for the user.")


class HindsightOutput(BaseModel):
    id: str
    slug: str
    chapter: str
    concept: str
    question: str
    reasoning: str
    answer: str


# --- PROMPTS ---

TOPIC_EXTRACTOR_PROMPT = """You are an expert Technical Curriculum Architect.
Your goal is to extract "Teachables" from a given curriculum chapter for the `numrs` Rust library.

<INSTRUCTIONS>
1. Read the provided chapter content.
2. Identify ALL distinctive concepts or practical skills taught in this chapter.
3. **Mix Types**: Include both:
    - **Coding Challenges**: "How to reshape an array?"
    - **Conceptual Questions**: "How does broadcasting work with different shapes?" or "What is the difference between a View and a Copy?"
4. **Comprehensive List**: Do not limit yourself to a small number. Extract a comprehensive list of tasks that covers the chapter thoroughly.
</INSTRUCTIONS>
"""

QRA_GENERATOR_PROMPT = """You are an expert Rust Developer and Technical Writer who has internalized the `numrs` library.
Your task is to create a verified QRA (Question, Reasoning, Answer) triplet for a specific concept.

<CONTEXT>
Library: `numrs`
Concept: {description}
</CONTEXT>

<GOAL>
1. **Design a Question**: A practical coding challenge OR a conceptual question.
2. **Verify Solution (Agentic Loop)**:
    - **Verification Rule**: If your intended Answer includes ANY Rust code snippet (even for conceptual explanations), you MUST verify it.
        - You MUST write a verification script (e.g. `src/bin/verify_x.rs`) using the `write_to_file` tool.
        - You MUST run it using `run_command` (e.g. `cargo run --bin verify_x`).
        - You MUST fix any compilation/runtime errors until it passes.
    - If the answer is purely text, verification is mental/conceptual.
3. **Draft Output**:
    - **Reasoning**: Write as an expert recalling knowledge ("To solve this, we use..."). Mention you wrote a script to check your memory if verification was performed.
    - **Answer**: The final natural answer.
</GOAL>

<IMPORTANT>
Your final response MUST be a pure JSON object corresponding to the QRAContent schema.
Do NOT include any conversational filler like "Here is the JSON:", "I have verified...", or "thought:".
Do NOT use markdown fencing for the JSON (no ```json ... ```), just the raw JSON object.
</IMPORTANT>
"""


async def generate_qra_task(
    teachable: Teachable,
    chapter_slug: str,
    output_dir: Path,
    model_name: str,
    api_key: str,
    boilerplate_dir: Path,
    library_path: Path,
    curriculum_src: Path,
    image_name: str,
):
    # Deterministic UUID for the task
    task_uuid = uuid.uuid5(uuid.NAMESPACE_DNS, f"{chapter_slug}_{teachable.slug}")
    output_file = output_dir / f"{task_uuid}.json"

    if output_file.exists():
        print(f"Skipping {teachable.slug} (already exists at {output_file.name})")
        return

    print(f"Starting generation for: {teachable.slug} ({task_uuid})")

    injections = {
        library_path: "repos/library",
        curriculum_src: "curriculum",
    }

    try:
        # Use TempWorkspace context manager
        with TempWorkspace(
            template_dir=boilerplate_dir,
            injections=injections,
            prefix=f"hindsight_{teachable.slug}_",
        ) as sandbox_dir:
            # Initialize Model per task (lightweight wrapper)
            model = LitellmModel(model=model_name, api_key=api_key)

            async with RustCodingEnvironment(
                workspace_dir=sandbox_dir, image_name=image_name
            ) as runtime:
                # The workspace inside the container is /workspace
                playground_path = "/workspace"

                generator_agent = AgentWrapper[QRAContent].create(
                    name=f"QRA_Architect_{teachable.slug}",
                    instructions=QRA_GENERATOR_PROMPT.format(
                        description=teachable.description
                    )
                    + f"\n\nIMPORTANT: Working Directory for commands is `{playground_path}`.",
                    model=model,
                    mcp_servers=[runtime.server],
                    output_type=QRAContent,
                )

                result = await generator_agent.run(
                    "Create the verified QRA. Use tools to verify code if needed.",
                    max_turns=30,
                )

                final_obj = result.final_output()

                # Save as structured JSON
                output_model = HindsightOutput(
                    id=str(task_uuid),
                    slug=teachable.slug,
                    chapter=chapter_slug,
                    concept=teachable.description,
                    question=final_obj.question,
                    reasoning=final_obj.reasoning,
                    answer=final_obj.answer,
                )

                output_file.write_text(output_model.model_dump_json(indent=2))
                print(f"Completed {teachable.slug} -> {output_file.name}")

    except Exception as e:
        print(f"Error generating {teachable.slug}: {e}")
        # Optional: write error log


async def extract_from_chapter(
    chapter_file: Path, model: LitellmModel
) -> list[tuple[Teachable, str]]:
    if chapter_file.name.startswith("00_"):
        return []

    print(f"Processing Chapter: {chapter_file.name}")
    content = chapter_file.read_text()

    # Extract Teachables
    extractor_agent = AgentWrapper[TeachablesList].create(
        name="TopicExtractor",
        instructions=TOPIC_EXTRACTOR_PROMPT,
        model=model,
        output_type=TeachablesList,
    )

    try:
        extract_result = await extractor_agent.run(
            f"Extract teachables from this chapter:\n\n{content}", max_turns=5
        )
        teachables = extract_result.result.final_output.items
        print(f"  Found {len(teachables)} teachables in {chapter_file.name}.")

        return [(t, chapter_file.stem) for t in teachables]

    except Exception as e:
        print(f"Failed to extract info from {chapter_file.name}: {e}")
        return []


async def main():
    config = HindsightConfig()

    api_key = os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("Error: GOOGLE_API_KEY environment variable not set.")
        return

    # Resolve paths
    library_path = config.get_library_path()
    curriculum_src = config.get_curriculum_dir()
    boilerplate_dir = config.get_boilerplate_dir()
    output_dir = config.get_output_dir()

    # Path validation
    if not library_path.exists():
        print(f"Error: Library not found at {library_path}")
        return

    if not curriculum_src.exists():
        print(f"Error: Curriculum not found at {curriculum_src}")
        return

    if not boilerplate_dir.exists():
        print(f"Error: Boilerplate not found at {boilerplate_dir}")
        return

    output_dir.mkdir(parents=True, exist_ok=True)

    # Initialize Model for Topic Extraction
    model = LitellmModel(model=config.model_name, api_key=api_key)

    print(f"Initializing Hindsight Generator (Config: {config.experiment_id})...")

    # List curriculum files
    chapters = sorted(list(curriculum_src.glob("*.md")))
    all_tasks = []

    # Create extraction tasks
    extraction_tasks = [
        extract_from_chapter(chapter_file, model) for chapter_file in chapters
    ]

    print(f"Extracting teachables from {len(chapters)} chapters concurrently...")
    results = await gather_with_semaphore(
        extraction_tasks, max_concurrent=config.max_concurrent_tasks
    )

    # Flatten results
    for res in results:
        all_tasks.extend(res)

    tasks = [
        generate_qra_task(
            teachable=t,
            chapter_slug=c_slug,
            output_dir=output_dir,
            model_name=config.model_name,
            api_key=api_key,
            boilerplate_dir=boilerplate_dir,
            library_path=library_path,
            curriculum_src=curriculum_src,
            image_name=config.image_name,
        )
        for t, c_slug in all_tasks
    ]

    # Run in parallel with semaphore
    await gather_with_semaphore(tasks, max_concurrent=config.max_concurrent_tasks)
    print("All tasks completed.")


if __name__ == "__main__":
    asyncio.run(main())

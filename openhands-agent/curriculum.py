import asyncio
import json
import os
from pathlib import Path

from agents import ModelSettings
from agents.extensions.models.litellm_model import LitellmModel
from dotenv.main import load_dotenv
from oai_utils.agent import AgentWrapper
from pydantic import BaseModel, Field

from openhands_agent.runtime.rust_env import RustCodingEnvironment

# --- PROMPTS ---

CURRICULUM_ARCHITECT_PROMPT = """You are an expert Technical Curriculum Architect and Rust Developer.
Your goal is to design a comprehensive educational curriculum (textbook) for the 'numrs' Rust library.

<ROLE>
You are responsible for analyzing the provided codebase (`repos/library`) and creating a high-level curriculum plan.
</ROLE>

<INSTRUCTIONS>
1. **Explore**: Use your tools to explore the `repos/library` directory.
   - identifying key entry points (e.g., `src/lib.rs` if available) and usage examples (e.g., `examples/`).
   - Inspect individual modules to understand the API surface.
   - **Autonomous Discovery**: You are expected to find *any* other relevant files or directories that define the library's capabilities.

2. **Iterative Planning**:
   - Draft an initial list of chapters based on your exploration.
   - **CRITICAL STEP**: Stop and review your draft. Check the codebase again.
   - Ask yourself: "Did I miss any modules? Is 'interoperability' covered? Are 'macros' covered? Is 'error handling' covered?"
   - If you find missing pieces, add new chapters or sections.

3. **Plan**: Create a file named `curriculum_plan.md` in the current directory.
   - The plan should be a hierarchical table of contents (Chapters and Sections).
   - For each chapter, briefly explain what existing code/modules it covers.
   - Group related topics logically (e.g., "Linear Algebra", "Statistics", "Optimization", "Interoperability").
   - Ensure NO major feature of the library is left behind.
</INSTRUCTIONS>
"""

# Prompt to parse the markdown plan into a machine-readable JSON list of chapters
PLAN_PARSER_PROMPT = """You are a helper agent. 
Read the file `curriculum_plan.md`. 
Extract the list of chapters defined in the plan.
Output a pure JSON list of objects, where each object has:
- `chapter_number`: int
- `title`: str (e.g., "Core Concepts")
- `filename`: str (e.g., "01_core_concepts.md")
- `description`: str (A summary of what to cover based on the plan)

Example Output:
[
  {"chapter_number": 1, "title": "Introduction", "filename": "01_introduction.md", "description": "Cover installation and basics..."},
  ...
]

<IMPORTANT>
Output ONLY the JSON. No markdown formatting, no backticks.
</IMPORTANT>
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


async def main():
    load_dotenv()
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
        import shutil

        shutil.copytree(library_path, lib_repo_dir, dirs_exist_ok=True)

    # Start Environment
    async with RustCodingEnvironment(
        workspace_dir=workspace_dir, image_name=config.image_name
    ) as runtime:
        # --- PHASE 1: PLANNING ---
        plan_path = workspace_dir / "curriculum_plan.md"

        if not plan_path.exists():
            print("\n--- Phase 1: Generating Curriculum Plan ---")
            architect_agent = AgentWrapper.create(
                name="CurriculumArchitect",
                instructions=CURRICULUM_ARCHITECT_PROMPT,
                model=model,
                mcp_servers=[runtime.server],
                model_settings=ModelSettings(
                    tool_choice="auto", parallel_tool_calls=True
                ),
            )
            await architect_agent.run("Generate curriculum_plan.md", max_turns=30)
        else:
            print("\n--- Phase 1: Plan already exists, skipping generation ---")

        # --- PHASE 1.5: PARSING PLAN ---
        print("\n--- Phase 1.5: Parsing Plan ---")
        parser_agent = AgentWrapper.create(
            name="PlanParser",
            instructions=PLAN_PARSER_PROMPT,
            model=model,
            mcp_servers=[runtime.server],
        )

        parse_result = await parser_agent.run(
            f"Read {plan_path.name} and output the JSON list of chapters.", max_turns=5
        )

        try:
            raw_json = parse_result.result.final_output
            # Cleanup potential markdown ticks if the model ignored instructions
            clean_json = raw_json.replace("```json", "").replace("```", "").strip()
            chapters = json.loads(clean_json)
            print(f"Parsed {len(chapters)} chapters.")
        except Exception as e:
            print(f"Failed to parse plan JSON: {e}")
            print("Raw output:", parse_result.result.final_output)
            return

        # --- PHASE 2: WRITING CONTENT ---
        print("\n--- Phase 2: Writing Content ---")

        for chapter in chapters:
            ch_num = chapter["chapter_number"]
            title = chapter["title"]
            filename = chapter["filename"]
            desc = chapter["description"]

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

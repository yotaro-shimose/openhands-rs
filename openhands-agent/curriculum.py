import asyncio
import json
import os
from pathlib import Path


from agents import ModelSettings
from agents.extensions.models.litellm_model import LitellmModel
from oai_utils.agent import AgentWrapper

from openhands_agent.runtime.rust_env import RustCodingEnvironment

# --- PROMPTS ---

CURRICULUM_ARCHITECT_PROMPT = """You are an expert Technical Curriculum Architect and Rust Developer.
Your goal is to design a comprehensive educational curriculum (textbook) for the 'numrs' Rust library.

<ROLE>
You are responsible for analyzing the provided codebase (`repos/library`) and creating a high-level curriculum plan.
</ROLE>

<INSTRUCTIONS>
1. **Explore**: Use your tools to explore the `repos/library` directory.
   - Look at `src/lib.rs` to see exported modules.
   - Look at `examples/` to see how the library is intended to be used.
   - Inspect individual modules (e.g., `src/linalg/`, `src/stats/`) to understand the API surface.

2. **Plan**: Create a file named `curriculum_plan.md` in the current directory.
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


async def main():
    # Configuration
    model_name = "gemini/gemini-3-flash-preview"
    api_key = os.environ.get("GOOGLE_API_KEY")
    if not api_key:
        print("Error: GOOGLE_API_KEY environment variable not set.")
        return

    # Paths
    script_path = Path(__file__).resolve()
    openhands_agent_dir = script_path.parent
    project_root = openhands_agent_dir.parent
    library_path = project_root / "repositories" / "numrs"
    if not library_path.exists():
        library_path = openhands_agent_dir / "repositories" / "numrs"

    workspace_dir = openhands_agent_dir / "workspace_curriculum"
    curriculum_out_dir = workspace_dir / "curriculum"

    # Initialize Model
    model = LitellmModel(model=model_name, api_key=api_key)

    print(f"Initializing Curriculum Agent with model: {model_name}")
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

    image_name = os.getenv("OPENHANDS_IMAGE_NAME", "coder-mcp")

    # Start Environment
    async with RustCodingEnvironment(
        workspace_dir=workspace_dir, image_name=image_name
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
            # Force text output mostly, tool use not strictly needed but good for reading file
            model_settings=ModelSettings(tool_choice="auto"),
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

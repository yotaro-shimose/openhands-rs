import asyncio
from pathlib import Path
from dotenv import load_dotenv

from agents import Agent, Runner
from agents.model_settings import ModelSettings
from rust_env import RustCodingEnvironment

load_dotenv()


async def main():
    current_dir = Path.cwd()
    cache_dir = current_dir / ".sccache_debug"
    workspace_dir = current_dir / "rust_debug_workspace"

    # We DON'T clear cache_dir here to test persistence if it works at all
    workspace_dir.mkdir(parents=True, exist_ok=True)

    async with RustCodingEnvironment(
        image_name="openhands-agent-server-rs",
        cache_dir=cache_dir,
        workspace_dir=workspace_dir,
    ) as server:
        agent = Agent(
            name="Sccache Debugger",
            instructions="""You are a system debugger.
            1. Check environment: 'env | grep SCCACHE', 'which sccache', 'sccache --version'.
            2. Check directory permissions: 'ls -ld /var/cache/sccache' and 'touch /var/cache/sccache/test && rm /var/cache/sccache/test'.
            3. Setup project: 'cargo new debug_proj && cd debug_proj'.
            4. Build 1: 'sccache --zero-stats', 'cargo build', 'sccache --show-stats'.
            5. Build 2 (incremental check): 'cargo build' (should be fast/nothing to do), 'sccache --show-stats'.
            6. Build 3 (clean build check): 'cargo clean', 'cargo build', 'sccache --show-stats'.
            7. If still 0 hits, run a manual compilation: 'sccache rustc --version' and 'sccache rustc src/main.rs'.
            8. Final check: 'ls -R /var/cache/sccache' to see if files were actually written.
            """,
            mcp_servers=[server],
            model_settings=ModelSettings(tool_choice="auto"),
        )

        print(f"🤖 Starting deep debug turn...")
        result = await Runner.run(
            agent,
            "Analyze why sccache isn't caching. Perform the steps and share the outputs.",
        )
        print("\n--- DEBUG OUTPUT ---")
        print(result.final_output)


if __name__ == "__main__":
    asyncio.run(main())

import asyncio
from pathlib import Path
from dotenv import load_dotenv

from agents import Agent, Runner
from agents.model_settings import ModelSettings
from rust_env import RustCodingEnvironment

# Load environment variables
load_dotenv()


async def run_compilation_test(iteration: int):
    print(f"\n--- Starting Compiliation Test (Iteration {iteration}) ---")

    # We use explicit host paths for persistence check
    current_dir = Path.cwd()
    cache_dir = current_dir / ".sccache_test"
    cargo_cache_dir = current_dir / ".cargo_cache_test"
    workspace_dir = current_dir / "rust_test_workspace"

    if iteration == 1 and cache_dir.exists():
        import shutil

        shutil.rmtree(cache_dir)

    workspace_dir.mkdir(parents=True, exist_ok=True)

    async with RustCodingEnvironment(
        image_name="openhands-agent-server-rs",
        cache_dir=cache_dir,
        cargo_cache_dir=cargo_cache_dir,
        workspace_dir=workspace_dir,
    ) as server:
        agent = Agent(
            name="Rust Specialist",
            instructions="""You are a Rust expert.
            1. Verify environment: run 'sccache --version', 'whoami', and 'env | grep SCCACHE'.
            2. Check permissions: run 'ls -ld /var/cache/sccache /usr/local/cargo' and try 'touch /var/cache/sccache/test && rm /var/cache/sccache/test'.
            3. Create a new Rust project named 'hello_rl' using 'cargo new hello_rl' (if it doesn't exist).
            4. Change to the 'hello_rl' directory.
            5. Add a simple loop in 'src/main.rs' that prints numbers 1 to 5.
            6. Add a dependency to 'Cargo.toml' (e.g., 'serde = "1.0"') to test dependency caching.
            7. Run 'cargo clean' to ensure we are testing sccache (not local 'target/' artifacts).
            8. Compile the project using 'cargo build'.
            9. Run 'sccache --show-stats' and report 'Cache hits' and 'Cache misses'.
            """,
            mcp_servers=[server],
            model_settings=ModelSettings(tool_choice="auto"),
        )

        print(f"🤖 Agent turn for Iteration {iteration}...")
        result = await Runner.run(
            agent,
            "Please build the Rust project and show sccache stats. Ensure you clean first.",
            max_turns=30,
        )
        print(f"\nFinal Output Iteration {iteration}:")
        print(result.final_output)


async def main():
    try:
        # First run: should have misses
        await run_compilation_test(1)

        print("\n" + "=" * 50)
        print("Waiting a bit before second run to ensure container cleanup...")
        await asyncio.sleep(5)

        # Second run: should have hits (if persistence works)
        await run_compilation_test(2)

    except Exception as e:
        print(f"\n❌ Test failed: {e}")
        import traceback

        traceback.print_exc()


if __name__ == "__main__":
    asyncio.run(main())

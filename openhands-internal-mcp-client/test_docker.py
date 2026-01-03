import asyncio
import os
from dotenv import load_dotenv

from agents import Agent, Runner
from agents.model_settings import ModelSettings
from docker_runtime import DockerRuntime

# Load environment variables (for OPENAI_API_KEY)
load_dotenv()


async def main() -> None:
    image_name = "openhands-agent-server-rs"

    print(f"--- Starting Docker Runtime Test with image: {image_name} ---")

    # Define some environment variables and a volume mount for testing
    env_vars = {"TEST_VAR": "HelloFromDocker"}
    # Mount the current directory to /workspace/test_mount inside the container
    current_dir = os.getcwd()
    volumes = {current_dir: "/workspace/test_mount"}

    try:
        async with DockerRuntime(
            image_name=image_name, env_vars=env_vars, volumes=volumes
        ) as server:
            agent = Agent(
                name="Docker Test Assistant",
                instructions="""You are testing the MCP server running inside Docker.
                1. Check if the environment variable 'TEST_VAR' is set correctly using bash.
                2. Check if the directory '/workspace/test_mount' is mounted correctly and list its files.
                3. Create a small file in '/workspace/test_mount/docker_test.txt'.
                """,
                mcp_servers=[server],
                model_settings=ModelSettings(tool_choice="auto"),
            )

            print("\n🤖 Running Agent turn...")
            result = await Runner.run(
                agent, "Please verify the Docker environment (ENV and MOUNT)."
            )
            print("\nFinal Output Summary:")
            print(result.final_output)

            # Verify file creation on host
            test_file = os.path.join(current_dir, "docker_test.txt")
            if os.path.exists(test_file):
                print(
                    f"✅ Verified: 'docker_test.txt' was created on the host via volume mount."
                )
                os.remove(test_file)
            else:
                print(f"❌ Error: 'docker_test.txt' was NOT found on the host.")

    except Exception as e:
        print(f"\n❌ Test failed: {e}")
        import traceback

        traceback.print_exc()


if __name__ == "__main__":
    asyncio.run(main())

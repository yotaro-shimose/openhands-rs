import asyncio
import os
from dotenv import load_dotenv

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp
from agents.model_settings import ModelSettings

# Load environment variables (for OPENAI_API_KEY)
load_dotenv()


async def main() -> None:
    # My server is running on port 3000
    mcp_url = "http://localhost:3000/mcp"

    print(f"Connecting to {mcp_url}...")

    async with MCPServerStreamableHttp(
        name="OpenHands Server",
        params={
            "url": mcp_url,
            "timeout": 15,
        },
        cache_tools_list=False,
    ) as server:
        agent = Agent(
            name="OpenHands Assistant",
            instructions="""You are a helpful assistant with technical expertise.
            Test the following tools carefully:
            1. Create a directory named 'test_dir' using bash.
            2. Inside 'test_dir', create a file named 'hello.txt' with content 'Hello from OpenHands!'.
            3. Use the 'list_files' tool to verify the contents of 'test_dir'.
            4. Read 'test_dir/hello.txt' using 'read_file' to confirm content.
            5. Use the 'file_editor' tool with command 'str_replace' to change 'OpenHands' to 'Rust' in 'test_dir/hello.txt'.
            6. Read 'test_dir/hello.txt' again to verify the change.
            7. Delete 'test_dir/hello.txt' using the 'delete_file' tool.
            8. Use 'list_files' on 'test_dir' again to confirm it's gone.
            """,
            mcp_servers=[server],
            model_settings=ModelSettings(tool_choice="auto"),
        )

        try:
            print("\n--- Starting Comprehensive Tool Test ---")
            result = await Runner.run(agent, "Please run the tool test sequence.")
            print("\nFinal Output:")
            print(result.final_output)
        except Exception as e:
            print(f"\nError details: {e}")
            import traceback

            traceback.print_exc()


if __name__ == "__main__":
    asyncio.run(main())

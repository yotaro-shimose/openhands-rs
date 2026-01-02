import asyncio
import os
import sys
from dotenv import load_dotenv

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp
from agents.model_settings import ModelSettings
from agents.tracing import add_trace_processor, TracingProcessor
from agents.tracing.span_data import (
    FunctionSpanData,
    ResponseSpanData,
    AgentSpanData,
    GenerationSpanData,
)


class AgentContentPrinter(TracingProcessor):
    def on_trace_start(self, trace):
        print(f"\n🚀 TRACE START: {trace.name}")

    def on_trace_end(self, trace):
        pass

    def on_span_start(self, span):
        pass

    def on_span_end(self, span):
        data = span.span_data

        # 1. Capture Agent identity
        if isinstance(data, AgentSpanData):
            print(f"\n👤 AGENT: {data.name}")

        # 2. Capture Tool Calls
        elif isinstance(data, FunctionSpanData):
            print(f"\n🛠️  TOOL CALL: {data.name}")
            print(f"   Input: {data.input}")
            if data.output:
                outcome = str(data.output)
                if len(outcome) > 500:
                    outcome = outcome[:500] + "..."
                print(f"   Outcome: {outcome}")

        # 3. Capture LLM Generation / Responses
        elif isinstance(data, GenerationSpanData):
            # Often data.output contains the generated text
            if data.output:
                print(f"\n🤖 LLM GENERATION:")
                print(f"{data.output}")

        elif isinstance(data, ResponseSpanData):
            response = getattr(data, "response", None)
            if response:
                # Try to find messages in output_items (common in OpenAI Response)
                items = getattr(response, "output_items", [])
                for item in items:
                    if hasattr(item, "type") and "message" in item.type:
                        content = None
                        if hasattr(item, "message") and hasattr(
                            item.message, "content"
                        ):
                            content = item.message.content
                        elif hasattr(item, "content"):
                            content = item.content
                        if content:
                            print(f"\n🤖 AGENT MESSAGE:\n{content}")

    def force_flush(self):
        pass

    def shutdown(self):
        pass


# Add our custom processor to the global tracer
add_trace_processor(AgentContentPrinter())

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
            print("\nFinal Output Summary:")
            print(result.final_output)
        except Exception as e:
            print(f"\nError: {e}")


if __name__ == "__main__":
    asyncio.run(main())

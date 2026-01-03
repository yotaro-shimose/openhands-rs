"""Test script for the OpenHands Agent with DockerRuntime.

This demonstrates running the agent inside a Docker container for isolated execution.
"""

import asyncio

from dotenv import load_dotenv

from agents.tracing import add_trace_processor, TracingProcessor
from agents.tracing.span_data import (
    FunctionSpanData,
    ResponseSpanData,
    AgentSpanData,
)

from docker_runtime import DockerRuntime
from openhands_agent import OpenHandsAgent, AgentConfig


class AgentContentPrinter(TracingProcessor):
    """Prints agent activity for observability."""

    def on_trace_start(self, trace):
        print(f"\n🚀 TRACE START: {trace.name}")

    def on_trace_end(self, trace):
        pass

    def on_span_start(self, span):
        pass

    def on_span_end(self, span):
        data = span.span_data

        if isinstance(data, AgentSpanData):
            print(f"\n👤 AGENT: {data.name}")

        elif isinstance(data, FunctionSpanData):
            print(f"\n🛠️  TOOL CALL: {data.name}")
            print(f"   Input: {data.input}")
            if data.output:
                outcome = str(data.output)
                if len(outcome) > 500:
                    outcome = outcome[:500] + "..."
                print(f"   Outcome: {outcome}")

        elif isinstance(data, ResponseSpanData):
            response = getattr(data, "response", None)
            if response:
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


add_trace_processor(AgentContentPrinter())
load_dotenv()


async def main() -> None:
    """Run a test task with the OpenHands agent in DockerRuntime."""
    config = AgentConfig.from_env()
    print(f"Using model: {config.model}")
    print("Starting DockerRuntime...")

    # Start DockerRuntime with the MCP server
    async with DockerRuntime(image_name="openhands-agent-server-rs") as runtime:
        print("DockerRuntime started, connecting agent...")

        # Create agent with the Docker runtime
        async with OpenHandsAgent(runtime=runtime, config=config) as agent:
            # Example task - create a simple Python script
            task = """
            Please do the following:
            1. Create a Python file called 'hello.py' that prints "Hello from Docker!"
            2. Run the script to verify it works
            """

            print(f"\n📋 Task: {task.strip()}")
            print("\n" + "=" * 60)

            result = await agent.run(task)

            print("\n" + "=" * 60)
            print("\n✅ Final Output:")
            print(result.final_output)


if __name__ == "__main__":
    asyncio.run(main())

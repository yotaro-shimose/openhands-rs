"""OpenHands Agent implementation using openai-agents-sdk."""

from typing import Any

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp
from agents.run import RunResult
from agents.model_settings import ModelSettings

from openhands_agent.config import AgentConfig
from openhands_agent.prompts import SYSTEM_PROMPT


class OpenHandsAgent:
    """OpenHands-compatible agent using openai-agents-sdk with MCP tools.

    This agent connects to a runtime to access file operations, bash execution,
    and other tools, using OpenHands-aligned system prompts for consistent behavior.

    Example with LocalRuntime (requires running MCP server):
        from openhands_agent import OpenHandsAgent, LocalRuntime

        async with LocalRuntime() as runtime:
            async with OpenHandsAgent(runtime=runtime) as agent:
                result = await agent.run("Create a hello world script")
                print(result.final_output)

    Example with DockerRuntime (self-contained):
        from docker_runtime import DockerRuntime
        from openhands_agent import OpenHandsAgent

        async with DockerRuntime(image_name="openhands-agent-server-rs") as runtime:
            async with OpenHandsAgent(runtime=runtime) as agent:
                result = await agent.run("Create a hello world script")
                print(result.final_output)
    """

    def __init__(
        self,
        runtime: MCPServerStreamableHttp,
        config: AgentConfig | None = None,
    ):
        """Initialize the agent with a runtime.

        Args:
            runtime: MCP server from a Runtime (LocalRuntime, DockerRuntime, etc.)
            config: Agent configuration. If None, loads from environment.
        """
        self.config = config or AgentConfig.from_env()
        self._mcp_server = runtime
        self._agent: Agent | None = None

    async def __aenter__(self) -> "OpenHandsAgent":
        """Enter async context and initialize agent."""
        self._agent = Agent(
            name="OpenHands Agent",
            instructions=SYSTEM_PROMPT,
            mcp_servers=[self._mcp_server],
            model=self.config.model,
            model_settings=ModelSettings(tool_choice="auto"),
        )
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Exit async context."""
        # Runtime handles MCP server cleanup, we just cleanup agent state
        self._agent = None

    async def run(self, task: str) -> RunResult:
        """Run the agent with a task.

        Args:
            task: The task to execute (natural language description)

        Returns:
            RunResult containing the agent's output and execution details

        Raises:
            RuntimeError: If agent not initialized (not in async context)
        """
        if not self._agent:
            raise RuntimeError("Agent not initialized. Use 'async with' context.")

        return await Runner.run(self._agent, task)


async def run_agent(
    task: str,
    runtime: MCPServerStreamableHttp | None = None,
    config: AgentConfig | None = None,
) -> RunResult:
    """Convenience function to run a task with the OpenHands agent.

    Args:
        task: The task to execute
        runtime: MCP server from a Runtime. If None, uses LocalRuntime.
        config: Optional agent configuration

    Returns:
        RunResult containing the agent's output

    Example with explicit runtime:
        async with DockerRuntime(image_name="openhands-agent-server-rs") as runtime:
            result = await run_agent("Fix the bug", runtime=runtime)
            print(result.final_output)

    Example with default LocalRuntime:
        # Requires MCP server running on localhost:3000
        async with LocalRuntime() as runtime:
            result = await run_agent("Fix the bug", runtime=runtime)
            print(result.final_output)
    """
    if runtime is None:
        raise ValueError("runtime is required. Use LocalRuntime() or DockerRuntime().")

    async with OpenHandsAgent(runtime=runtime, config=config) as agent:
        return await agent.run(task)

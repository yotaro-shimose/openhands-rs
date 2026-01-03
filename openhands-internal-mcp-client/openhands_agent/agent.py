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

    This agent connects to an MCP server to access file operations, bash execution,
    and other tools, using OpenHands-aligned system prompts for consistent behavior.

    Example with local MCP server:
        async with OpenHandsAgent() as agent:
            result = await agent.run("Create a hello world script")
            print(result.final_output)

    Example with DockerRuntime:
        from docker_runtime import DockerRuntime

        async with DockerRuntime(image_name="openhands-agent-server-rs") as mcp_server:
            async with OpenHandsAgent(mcp_server=mcp_server) as agent:
                result = await agent.run("Create a hello world script")
                print(result.final_output)
    """

    def __init__(
        self,
        config: AgentConfig | None = None,
        mcp_server: MCPServerStreamableHttp | None = None,
    ):
        """Initialize the agent with configuration.

        Args:
            config: Agent configuration. If None, loads from environment.
            mcp_server: Optional pre-configured MCP server (e.g., from DockerRuntime).
                       If provided, skips creating a new MCP connection.
        """
        self.config = config or AgentConfig.from_env()
        self._external_mcp_server = mcp_server
        self._mcp_server: MCPServerStreamableHttp | None = None
        self._owns_mcp_server = False
        self._agent: Agent | None = None

    async def __aenter__(self) -> "OpenHandsAgent":
        """Enter async context and connect to MCP server."""
        if self._external_mcp_server:
            # Use externally provided MCP server (e.g., from DockerRuntime)
            self._mcp_server = self._external_mcp_server
            self._owns_mcp_server = False
        else:
            # Create our own MCP connection
            self._mcp_server = MCPServerStreamableHttp(
                name="OpenHands MCP Server",
                params={
                    "url": self.config.mcp_url,
                    "timeout": self.config.timeout,
                },
                cache_tools_list=False,
            )
            await self._mcp_server.__aenter__()
            self._owns_mcp_server = True

        self._agent = Agent(
            name="OpenHands Agent",
            instructions=SYSTEM_PROMPT,
            mcp_servers=[self._mcp_server],
            model=self.config.model,
            model_settings=ModelSettings(tool_choice="auto"),
        )
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Exit async context and cleanup MCP connection."""
        # Only cleanup MCP server if we own it (didn't receive it externally)
        if self._mcp_server and self._owns_mcp_server:
            await self._mcp_server.__aexit__(exc_type, exc_val, exc_tb)

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
    config: AgentConfig | None = None,
    mcp_server: MCPServerStreamableHttp | None = None,
) -> RunResult:
    """Convenience function to run a task with the OpenHands agent.

    This is a simple wrapper that creates an agent, runs the task, and returns
    the result. For multiple tasks, use OpenHandsAgent context manager directly.

    Args:
        task: The task to execute
        config: Optional agent configuration
        mcp_server: Optional pre-configured MCP server (e.g., from DockerRuntime)

    Returns:
        RunResult containing the agent's output

    Example:
        result = await run_agent("Fix the bug in main.py")
        print(result.final_output)
    """
    async with OpenHandsAgent(config, mcp_server) as agent:
        return await agent.run(task)

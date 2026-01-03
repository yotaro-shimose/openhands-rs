"""Runtime abstraction for OpenHands agent.

Provides base class and implementations for different execution environments.
"""

from abc import ABC, abstractmethod
from typing import Any

from agents.mcp import MCPServerStreamableHttp


class Runtime(ABC):
    """Base class for runtime environments.

    A Runtime provides an MCP server connection for the agent to use.
    Implementations handle setup/teardown of the execution environment.
    """

    @abstractmethod
    async def __aenter__(self) -> MCPServerStreamableHttp:
        """Enter runtime context and return MCP server."""
        pass

    @abstractmethod
    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Exit runtime context and cleanup."""
        pass


class LocalRuntime(Runtime):
    """Runtime that connects to a local MCP server.

    Use this when you have an MCP server already running locally.

    Example:
        # Start server: cargo run -p openhands-agent-server-rs
        async with LocalRuntime() as mcp_server:
            async with OpenHandsAgent(runtime=mcp_server) as agent:
                result = await agent.run("Create hello.py")
    """

    def __init__(
        self,
        url: str = "http://localhost:3000/mcp",
        timeout: int = 30,
    ):
        """Initialize LocalRuntime.

        Args:
            url: URL of the MCP server endpoint
            timeout: Connection timeout in seconds
        """
        self.url = url
        self.timeout = timeout
        self._mcp_server: MCPServerStreamableHttp | None = None

    async def __aenter__(self) -> MCPServerStreamableHttp:
        """Connect to local MCP server."""
        self._mcp_server = MCPServerStreamableHttp(
            name="Local MCP Server",
            params={
                "url": self.url,
                "timeout": self.timeout,
            },
            cache_tools_list=False,
        )
        return await self._mcp_server.__aenter__()

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Disconnect from MCP server."""
        if self._mcp_server:
            await self._mcp_server.__aexit__(exc_type, exc_val, exc_tb)

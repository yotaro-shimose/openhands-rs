"""Configuration management for OpenHands Agent."""

import os
from dataclasses import dataclass


@dataclass
class AgentConfig:
    """Configuration for the OpenHands agent.

    Attributes:
        mcp_url: URL of the MCP server endpoint
        model: LLM model to use
        timeout: Connection timeout in seconds
    """

    mcp_url: str = "http://localhost:3000/mcp"
    model: str = "gpt-4o"
    timeout: int = 30

    @classmethod
    def from_env(cls) -> "AgentConfig":
        """Load configuration from environment variables."""
        return cls(
            mcp_url=os.getenv("MCP_URL", "http://localhost:3000/mcp"),
            model=os.getenv("MODEL", "gpt-4o"),
            timeout=int(os.getenv("TIMEOUT", "30")),
        )

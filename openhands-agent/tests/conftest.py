"""Shared pytest fixtures for OpenHands agent tests.

Provides reusable fixtures for runtimes and configurations.
Uses LLM-as-a-judge pattern for testing (no mocks).
"""

import os
import pytest
import pytest_asyncio
from pathlib import Path

from dotenv import load_dotenv
from agents.mcp import MCPServerStreamableHttp


from openhands_agent import OpenHandsAgent, AgentConfig
from openhands_agent.runtime import LocalRuntime
from docker_runtime import DockerRuntime

# Load environment variables (OPENAI_API_KEY, etc.)
load_dotenv()


@pytest.fixture
def agent_config() -> AgentConfig:
    """Load agent configuration from environment."""
    return AgentConfig.from_env()


@pytest.fixture
def temp_workspace(tmp_path: Path) -> Path:
    """Create a temporary workspace directory."""
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    return workspace


@pytest_asyncio.fixture
async def docker_runtime(temp_workspace: Path):
    """Create a DockerRuntime with temporary workspace."""
    async with DockerRuntime(workspace_dir=str(temp_workspace)) as runtime:
        yield runtime


@pytest_asyncio.fixture
async def local_runtime(agent_config: AgentConfig):
    """Create a LocalRuntime (requires running MCP server)."""
    async with LocalRuntime(
        url=agent_config.mcp_url,
        timeout=agent_config.timeout,
    ) as runtime:
        yield runtime


async def llm_judge(
    mcp_server: MCPServerStreamableHttp,
    task_description: str,
    agent_output: str,
    criteria: list[str],
    config: AgentConfig | None = None,
) -> tuple[bool, str]:
    """Use another OpenHandsAgent as judge to evaluate agent output.

    The judge agent has access to the same workspace via MCP server,
    allowing it to inspect files and verify task completion.

    Args:
        mcp_server: MCP server for workspace access
        task_description: What the agent was asked to do
        agent_output: The agent's final output
        criteria: List of success criteria to check
        config: Agent config (defaults to env-based config with gpt-4o-mini)

    Returns:
        Tuple of (passed: bool, explanation: str)
    """
    judge_config = config or AgentConfig(
        mcp_url="",  # Not used, we pass mcp_server directly
        model="gpt-4o-mini",
        timeout=30,
    )

    criteria_text = "\n".join(f"- {c}" for c in criteria)

    judge_task = f"""You are a test evaluator. Evaluate if the agent successfully completed its task.

TASK GIVEN TO AGENT:
{task_description}

SUCCESS CRITERIA:
{criteria_text}

AGENT'S OUTPUT:
{agent_output}

INSTRUCTIONS:
1. Use the file tools (read_file, list_files) to verify the workspace state if needed
2. Check if the criteria are met based on the agent output and workspace state
3. Respond with your evaluation in this exact format:

PASSED: yes/no
EXPLANATION: <brief explanation of your findings>"""

    async with OpenHandsAgent(mcp_server=mcp_server, config=judge_config) as judge:
        result = await judge.run(judge_task)

    output = result.final_output or ""
    passed = "passed: yes" in output.lower()

    return passed, output

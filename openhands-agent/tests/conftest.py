"""Shared pytest fixtures for OpenHands agent tests.

Provides reusable fixtures for runtimes and configurations.
Uses LLM-as-a-judge pattern for testing (no mocks).
"""

import os
import pytest
import pytest_asyncio
import tempfile
from pathlib import Path

from dotenv import load_dotenv

# Load environment variables (OPENAI_API_KEY, etc.)
load_dotenv()

from agents import Agent, Runner

from openhands_agent import OpenHandsAgent, AgentConfig
from openhands_agent.runtime import LocalRuntime
from docker_runtime import DockerRuntime


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
    task_description: str,
    agent_output: str,
    criteria: list[str],
    model: str = "gpt-4o-mini",
) -> tuple[bool, str]:
    """Use LLM-as-a-judge to evaluate agent output.

    Args:
        task_description: What the agent was asked to do
        agent_output: The agent's final output
        criteria: List of success criteria to check
        model: Model to use for judging

    Returns:
        Tuple of (passed: bool, explanation: str)
    """
    from openai import OpenAI

    client = OpenAI()

    criteria_text = "\n".join(f"- {c}" for c in criteria)

    response = client.chat.completions.create(
        model=model,
        messages=[
            {
                "role": "system",
                "content": """You are a test evaluator. Given a task and agent output,
determine if the agent successfully completed the task based on the criteria.

Respond in this format:
PASSED: yes/no
EXPLANATION: <brief explanation>""",
            },
            {
                "role": "user",
                "content": f"""Task: {task_description}

Success Criteria:
{criteria_text}

Agent Output:
{agent_output}

Did the agent successfully complete the task?""",
            },
        ],
        temperature=0,
    )

    result = response.choices[0].message.content or ""
    passed = "PASSED: yes" in result.lower() or "passed: yes" in result.lower()

    return passed, result

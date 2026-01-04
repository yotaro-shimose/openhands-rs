"""OpenHands Agent implementation using openai-agents-sdk."""

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
        mcp_server: MCPServerStreamableHttp,
        config: AgentConfig | None = None,
    ):
        """Initialize the agent with a runtime.

        Args:
            runtime: MCP server from a Runtime (LocalRuntime, DockerRuntime, etc.)
            config: Agent configuration. If None, loads from environment.
        """
        self.config = config or AgentConfig.from_env()
        self._mcp_server = mcp_server
        self._agent = Agent(
            name="OpenHands Agent",
            instructions=SYSTEM_PROMPT,
            mcp_servers=[self._mcp_server],
            model=self.config.model,
            model_settings=ModelSettings(tool_choice="auto"),
        )

    async def run(
        self,
        input: str | list,
        *,
        context=None,
        max_turns: int | None = None,
        hooks=None,
        run_config=None,
        previous_response_id: str | None = None,
        auto_previous_response_id: bool = False,
        conversation_id: str | None = None,
        session=None,
    ) -> RunResult:
        """Run the agent with a task.

        Args:
            input: The initial input to the agent.
            context: The context to run the agent with.
            max_turns: The maximum number of turns to run the agent for.
            hooks: An object that receives callbacks on various lifecycle events.
            run_config: Global settings for the entire agent run.
            previous_response_id: The ID of the previous response.
            auto_previous_response_id: Whether to automatically use the previous response ID.
            conversation_id: The conversation ID.
            session: A session for automatic conversation history management.

        Returns:
            RunResult containing the agent's output and execution details
        """
        if not self._agent:
            raise RuntimeError("Agent not initialized.")

        # Use configured max_iterations if max_turns is not provided
        if max_turns is None and self.config.max_iterations:
            max_turns = self.config.max_iterations
        elif max_turns is None:
            max_turns = 30

        return await Runner.run(
            self._agent,
            input,
            context=context,
            max_turns=max_turns,
            hooks=hooks,
            run_config=run_config,
            previous_response_id=previous_response_id,
            auto_previous_response_id=auto_previous_response_id,
            conversation_id=conversation_id,
            session=session,
        )


async def run_agent(
    task: str,
    mcp_server: MCPServerStreamableHttp | None = None,
    config: AgentConfig | None = None,
) -> RunResult:
    """Convenience function to run a task with the OpenHands agent."""
    if mcp_server is None:
        raise ValueError(
            "mcp_server is required. Use LocalRuntime() or DockerRuntime()."
        )

    agent = OpenHandsAgent(mcp_server=mcp_server, config=config)
    return await agent.run(task)

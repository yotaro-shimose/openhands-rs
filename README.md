# Coder MCP & OpenHands Agent

This repository contains the high-performance Rust implementation of the Coder MCP server and the OpenHands Agent.

## Overview

The project consists of two primary components:

1.  **`coder-mcp`**: A standalone Rust HTTP server implementing the Model Context Protocol (MCP). It provides tools for agents to interact with the file system and execute bash commands.
2.  **`openhands-agent`**: A Python-based agent system that utilities the Coder MCP server to perform coding tasks.

## Architecture

### Agent-Runtime Decoupling

The core design philosophy is the separation of the **Agent** (decision making) from the **Runtime** (execution).

-   **Agent**: Resides in `openhands-agent/`, using LLMs to decide actions.
-   **Runtime (Coder MCP)**: Resides in `coder-mcp/`, providing the toolbox (bash, file editing, grep, etc.) via a standard MCP interface.

### Workspace Pattern

We support a "Workspace" architecture where the agent interacts with a sandboxed environment.

-   **DockerRuntime**: Spins up a disposable Docker container for each session. This container runs an instance of `coder-mcp` inside it. The host agent proxies commands to this inner server via HTTP, ensuring complete isolation and a clean development environment for every task.

## Getting Started

### Prerequisites

-   Rust 1.83+
-   Python 3.10+
-   Docker (for containerized execution)
-   `OPENAI_API_KEY` (or compatible LLM key)

### Running the Server Locally

If you want to run the Coder MCP server directly:

```bash
cd coder-mcp
cargo run
```

### Building the Docker Image

Required for `DockerRuntime`:

```bash
docker build -t coder-mcp:latest .
```

## Project Structure

-   `coder-mcp/`: Standalone Rust MCP server.
    -   `src/main.rs`: Server entry point and routing.
    -   `src/service.rs`: MCP tool implementation logic.
    -   `src/runtime/`: Internal services (Bash execution).
    -   `src/tools/`: Tool-specific handlers (File editor, Task tracker).
-   `openhands-agent/`: Python agent implementation.
    -   `openhands_agent/runtime/`: Docker and local runtime managers.
    -   `openhands_agent/agent.py`: Core agent loop.

## Contributing

Contributions are welcome! Please ensure you run tests before submitting:

```bash
cd coder-mcp
cargo test
```

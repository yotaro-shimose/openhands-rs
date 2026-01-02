# OpenHands Rust SDK & Agent Server

This repository contains the high-performance Rust implementation of the OpenHands SDK and Agent Server.

## Overview

The Rust implementation targets performance, type safety, and scalability. It consists of two main crates:

1.  **`openhands-sdk-rs`**: The core library for building agents.
2.  **`openhands-agent-server-rs`**: The HTTP server that hosts agents and manages conversations.

## Architecture

### Agent-Runtime Decoupling

The core design philosophy is the separation of the **Agent** (decision making) from the **Runtime** (execution).

-   **Agent**: Uses an LLM (via `genai` crate) to decide the next step based on the conversation history. It does not know *how* tools are executed.
-   **Runtime**: Implements the `Runtime` trait to execute tools. This allows the same agent to run in different environments without code changes.

### Workspace Pattern (Server-Inside-Docker)

We support a "Workspace" architecture where the agent interacts with a sandboxed environment.

-   **LocalRuntime**: Executes tools (shell commands, file ops) directly on the host machine.
-   **DockerRuntime**: Spins up a disposable Docker container for each session. This container runs an instance of `openhands-agent-server-rs` inside it. The host agent proxies commands to this inner server via HTTP, ensuring complete isolation.

## Getting Started

### Prerequisites

-   Rust 1.83+
-   Docker (optional, for DockerRuntime)
-   `OPENAI_API_KEY` (or compatible LLM key)

### Installation

Clone the repository:

```bash
git clone https://github.com/OpenHands/rust-sdk.git
cd rust-sdk
```

### Running the Server

1.  Set your API key:
    ```bash
    export OPENAI_API_KEY="sk-..."
    ```

2.  Run the server locally:
    ```bash
    cargo run -p openhands-agent-server-rs
    ```

3.  Or run with Docker Runtime enabled (requires Docker):
    ```bash
    export RUNTIME_ENV="docker"
    cargo run -p openhands-agent-server-rs
    ```

### Running the Example Agent

We provide a CLI demo that uses the SDK directly:

```bash
cargo run -p openhands-sdk-rs --example agent_demo
```

## Docker Deployment

To build the agent server image (required for `DockerRuntime`):

```bash
docker build -t openhands-agent-server-rs:latest .
```

## Project Structure

-   `openhands-sdk-rs/`: Core SDK library.
    -   `src/agent.rs`: ReAct agent loop.
    -   `src/runtime.rs`: Runtime trait and LocalRuntime.
    -   `src/runtime/docker.rs`: DockerRuntime implementation.
    -   `src/tools.rs`: Tool definitions (Cmd, FileRead, FileWrite).
-   `openhands-agent-server-rs/`: Axum web server.
    -   `src/conversation_api.rs`: Manages conversation state and runtime selection.
    -   `src/bash_service.rs`: Persistent bash session management.

## Contributing

Contributions are welcome! Please ensure you run tests before submitting:

```bash
cargo test --workspace
```

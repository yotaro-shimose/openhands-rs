use crate::runtime::Runtime;
use crate::tools::Tool;
use async_trait::async_trait;
use serde_json::Value;
use std::process::Command;
use uuid::Uuid;

/// A runtime that runs the agent within a Docker container.
///
/// This implements the "Workspace" pattern where the agent functions inside an isolated environment.
/// In this specific implementation ("Server-Inside-Docker"), a Docker container is started
/// running the `openhands-agent-server-rs`. The `DockerRuntime` then acts as a client,
/// sending tool execution requests (e.g., "cmd") to the API server running inside that container.
///
/// This allows for:
/// - Sandboxing: The agent cannot accidentally harm the host system.
/// - Reproducibility: Every session starts with a clean state.
pub struct DockerRuntime {
    /// The Docker container ID (name) used for lifecycle management (stop/rm).
    pub container_id: String,
    pub image_name: String,
    /// The internal RemoteRuntime used for communication with the agent server.
    remote: crate::runtime::RemoteRuntime,
}

impl DockerRuntime {
    /// Starts a new Docker container with the specified image and waits for it to be ready.
    ///
    /// This function:
    /// 1. Generates a unique container name.
    /// 2. Assigns a random host port (3000-4000) to map to the container's port 3000.
    /// 3. execute `docker run` to start the container in detached mode.
    /// 4. Waits for the container to initialize (currently a simple sleep).
    ///
    /// # Arguments
    /// * `image` - The Docker image to run (must contain `openhands-agent-server-rs`).
    /// * `tools` - The tools available to this runtime.
    pub fn new(image: &str, tools: Vec<Box<dyn Tool>>) -> Self {
        // Start the container
        let container_name = format!("openhands-agent-{}", Uuid::new_v4());
        let port = 3000 + (rand::random::<u16>() % 1000); // Simple random port for now

        let status = Command::new("docker")
            .args(&[
                "run",
                "-d",
                "-p",
                &format!("{}:3000", port),
                "--name",
                &container_name,
                image,
            ])
            .status()
            .expect("Failed to start docker container");

        if !status.success() {
            panic!("Docker run failed");
        }

        // Wait for health check (simplified for now, ideally retry loop)
        std::thread::sleep(std::time::Duration::from_secs(5));

        Self {
            container_id: container_name,
            image_name: image.to_string(),
            remote: crate::runtime::RemoteRuntime::new(format!("http://localhost:{}", port), tools),
        }
    }

    /// Stops and removes the Docker container.
    pub fn stop(&self) {
        let _ = Command::new("docker")
            .args(&["stop", &self.container_id])
            .output();
        let _ = Command::new("docker")
            .args(&["rm", &self.container_id])
            .output();
    }
}

impl Drop for DockerRuntime {
    /// Ensures the container is cleaned up when the Runtime is dropped.
    fn drop(&mut self) {
        self.stop();
    }
}

#[async_trait]
impl Runtime for DockerRuntime {
    fn tools(&self) -> &[Box<dyn Tool>] {
        self.remote.tools()
    }

    /// Executes an action by delegating to the internal RemoteRuntime.
    async fn execute(&self, action: &str, args: Value) -> Result<String, String> {
        self.remote.execute(action, args).await
    }
}

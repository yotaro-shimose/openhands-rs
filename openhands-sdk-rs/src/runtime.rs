use crate::tools::Tool;
use async_trait::async_trait;
pub mod docker;
pub use docker::DockerRuntime;
use serde_json::Value;

#[async_trait]
/// Defines the runtime environment where the agent executes tools.
///
/// The Runtime is responsible for the actual execution of actions (like running shell commands
/// or reading files) and exposing the available/allowed tools to the agent.
/// This decouples the Agent's decision-making logic from the execution environment (host, container, etc.).
pub trait Runtime: Send + Sync {
    /// List available tools that the agent can call in this runtime.
    fn tools(&self) -> &[Box<dyn Tool>];

    /// Execute a tool action with the given arguments.
    ///
    /// # Arguments
    /// * `action` - The name of the tool to execute (e.g., "cmd", "file_read").
    /// * `args` - The arguments for the tool as a JSON Value.
    ///
    /// # Returns
    /// * `Ok(String)` - The output of the tool execution.
    /// * `Err(String)` - An error message if execution fails.
    async fn execute(&self, action: &str, args: Value) -> Result<String, String>;
}

pub mod local;
pub use local::LocalRuntime;

pub mod remote;
pub use remote::RemoteRuntime;

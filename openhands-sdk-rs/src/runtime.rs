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

/// A local runtime implementation that executes tools directly on the host machine
/// (or within the same container if the agent itself is containerized).
///
/// This runtime uses the provided tool implementations directly.
pub struct LocalRuntime {
    tools: Vec<Box<dyn Tool>>,
}

impl LocalRuntime {
    /// Create a new LocalRuntime with the given set of tools.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl Runtime for LocalRuntime {
    fn tools(&self) -> &[Box<dyn Tool>] {
        &self.tools
    }

    async fn execute(&self, action: &str, args: Value) -> Result<String, String> {
        if let Some(tool) = self.tools.iter().find(|t| t.name() == action) {
            tool.call(args).await
        } else {
            Err(format!("Tool {} not found", action))
        }
    }
}

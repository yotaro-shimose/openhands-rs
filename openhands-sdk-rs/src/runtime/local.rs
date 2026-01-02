use crate::runtime::Runtime;
use crate::tools::Tool;
use async_trait::async_trait;
use serde_json::Value;

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

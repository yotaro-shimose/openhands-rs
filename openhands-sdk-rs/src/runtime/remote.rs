use crate::agent::tools::Tool;
use crate::models::{
    BashOutput, ExecuteBashRequest, FileReadRequest, FileResponse, FileWriteRequest,
};
use crate::runtime::Runtime;
use async_trait::async_trait;
use serde_json::Value;

/// A runtime that executes tools by sending requests to a remote agent server.
pub struct RemoteRuntime {
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
}

impl RemoteRuntime {
    pub fn new(base_url: String, tools: Vec<Box<dyn Tool>>) -> Self {
        Self { base_url, tools }
    }
}

#[async_trait]
impl Runtime for RemoteRuntime {
    fn tools(&self) -> &[Box<dyn Tool>] {
        &self.tools
    }

    async fn execute(&self, action: &str, args: Value) -> Result<String, String> {
        let client = reqwest::Client::new();

        if action == "cmd" {
            let command = args["command"].as_str().ok_or("Missing command")?;
            let req = ExecuteBashRequest {
                command: command.to_string(),
                cwd: None,
                timeout: None,
            };
            let res = client
                .post(format!("{}/bash/execute_bash_command", self.base_url))
                .json(&req)
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !res.status().is_success() {
                let status = res.status();
                let error_text = res.text().await.unwrap_or_default();
                return Err(format!("Server returned error {}: {}", status, error_text));
            }

            let output: BashOutput = res.json().await.map_err(|e| e.to_string())?;
            let mut combined = String::new();
            if let Some(stdout_str) = output.stdout {
                combined.push_str(&stdout_str);
            }
            if let Some(stderr_str) = output.stderr {
                if !combined.is_empty() {
                    combined.push_str("\n");
                }
                combined.push_str("Error output:\n");
                combined.push_str(&stderr_str);
            }
            return Ok(combined);
        }

        if action == "read_file" {
            let path = args["path"].as_str().ok_or("Missing path")?;
            let req = FileReadRequest {
                path: path.to_string(),
            };
            let res = client
                .post(format!("{}/file/read", self.base_url))
                .json(&req)
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !res.status().is_success() {
                return Err(format!("Server returned error: {}", res.status()));
            }

            let output: FileResponse = res.json().await.map_err(|e| e.to_string())?;
            if output.success {
                return Ok(output.content.unwrap_or_default());
            } else {
                return Err(output.error.unwrap_or_else(|| "Unknown error".to_string()));
            }
        }

        if action == "write_file" {
            let path = args["path"].as_str().ok_or("Missing path")?;
            let content = args["content"].as_str().ok_or("Missing content")?;
            let req = FileWriteRequest {
                path: path.to_string(),
                content: content.to_string(),
            };
            let res = client
                .post(format!("{}/file/write", self.base_url))
                .json(&req)
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !res.status().is_success() {
                return Err(format!("Server returned error: {}", res.status()));
            }

            let output: FileResponse = res.json().await.map_err(|e| e.to_string())?;
            if output.success {
                return Ok(format!("File written to {}", path));
            } else {
                return Err(output.error.unwrap_or_else(|| "Unknown error".to_string()));
            }
        }

        Err(format!(
            "Tool {} not yet supported via RemoteRuntime API",
            action
        ))
    }
}

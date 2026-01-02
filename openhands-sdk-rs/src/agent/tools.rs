mod apply_patch;
mod file_editor;
mod glob;
mod grep;

pub use apply_patch::ApplyPatchTool;
pub use file_editor::FileEditorTool;
pub use glob::GlobTool;
pub use grep::GrepTool;

use async_trait::async_trait;
use serde_json::Value;
use std::process::Command;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parameters(&self) -> Value; // JSON Schema
    async fn call(&self, args: Value) -> Result<String, String>;
}

pub struct CmdTool;

#[async_trait]
impl Tool for CmdTool {
    fn name(&self) -> String {
        "cmd".to_string()
    }

    fn description(&self) -> String {
        "Execute a shell command (bash)".to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'command' argument")?;

        // Simple std::process implementation for now.
        // In real agent this might call BashEventService or unsafe shell.
        let output = Command::new("bash")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stderr.is_empty() {
            Ok(format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr))
        } else {
            Ok(stdout.to_string())
        }
    }
}

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> String {
        "read_file".to_string()
    }

    fn description(&self) -> String {
        "Read the contents of a file".to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute path to the file"
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'path' argument")?;

        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| e.to_string())
    }
}

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> String {
        "write_file".to_string()
    }

    fn description(&self) -> String {
        "Write content to a file".to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'path' argument")?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'content' argument")?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("Successfully wrote to {}", path))
    }
}

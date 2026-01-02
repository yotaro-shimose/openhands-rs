use rmcp::model::ErrorCode;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FileEditorArgs {
    pub command: String, // view, create, str_replace, insert, undo_edit
    pub path: String,
    pub file_text: Option<String>,
    pub view_range: Option<Vec<u64>>,
    pub old_str: Option<String>,
    pub new_str: Option<String>,
    pub insert_line: Option<u64>,
}

pub async fn run_file_editor(
    args: &FileEditorArgs,
    workspace_dir: &Path,
    editor_history: &Mutex<HashMap<PathBuf, Vec<String>>>,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    match args.command.as_str() {
        "view" => {
            if !path.exists() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!("File not found: {}", path.display()).into(),
                    data: None,
                });
            }
            match fs::read_to_string(&path) {
                Ok(content) => {
                    // Support view_range
                    let lines: Vec<&str> = content.lines().collect();
                    let output = if let Some(range) = &args.view_range {
                        if range.len() >= 2 {
                            let start = (range[0] as usize).saturating_sub(1);
                            let end = range[1] as usize;
                            lines
                                .iter()
                                .skip(start)
                                .take(end - start)
                                .cloned()
                                .collect::<Vec<&str>>()
                                .join("\n")
                        } else {
                            content
                        }
                    } else {
                        content
                    };
                    Ok(output)
                }
                Err(e) => Err(McpError {
                    code: ErrorCode(-32603),
                    message: format!("Failed to read file: {}", e).into(),
                    data: None,
                }),
            }
        }
        "create" => {
            if path.exists() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: "File already exists".to_string().into(),
                    data: None,
                });
            }
            let content = args.file_text.clone().unwrap_or_default();
            fs::write(&path, &content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to create file: {}", e).into(),
                data: None,
            })?;
            Ok("File created successfully".to_string())
        }
        "str_replace" => {
            if !path.exists() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: "File not found".to_string().into(),
                    data: None,
                });
            }
            let old_str = args.old_str.clone().ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing old_str".to_string().into(),
                data: None,
            })?;
            let new_str = args.new_str.clone().unwrap_or_default();

            let content = fs::read_to_string(&path).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to read file: {}", e).into(),
                data: None,
            })?;

            // Save history
            {
                let mut history = editor_history.lock().await;
                history
                    .entry(path.clone())
                    .or_default()
                    .push(content.clone());
            }

            let new_content = content.replace(&old_str, &new_str);
            // Check if anything changed
            if content == new_content {
                return Ok("No occurrences of old_str found".to_string());
            }

            fs::write(&path, &new_content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write file: {}", e).into(),
                data: None,
            })?;
            Ok("File updated successfully".to_string())
        }
        "insert" => {
            let insert_line = args.insert_line.ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing insert_line".to_string().into(),
                data: None,
            })?;
            let text = args.file_text.clone().ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing file_text".to_string().into(),
                data: None,
            })?;

            let content = fs::read_to_string(&path).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to read file: {}", e).into(),
                data: None,
            })?;

            // Save history
            {
                let mut history = editor_history.lock().await;
                history
                    .entry(path.clone())
                    .or_default()
                    .push(content.clone());
            }

            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let idx = (insert_line as usize).saturating_sub(1);
            if idx <= lines.len() {
                lines.insert(idx, text);
            } else {
                lines.push(text);
            }

            let new_content = lines.join("\n");
            fs::write(&path, &new_content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write file: {}", e).into(),
                data: None,
            })?;
            Ok("Text inserted successfully".to_string())
        }
        "undo_edit" => {
            let mut history = editor_history.lock().await;
            if let Some(versions) = history.get_mut(&path) {
                if let Some(prev_content) = versions.pop() {
                    fs::write(&path, &prev_content).map_err(|e| McpError {
                        code: ErrorCode(-32603),
                        message: format!("Failed to restore file: {}", e).into(),
                        data: None,
                    })?;
                    return Ok("Undo successful".to_string());
                }
            }
            Err(McpError {
                code: ErrorCode(-32602),
                message: "No edit history found for this file".to_string().into(),
                data: None,
            })
        }
        _ => Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Unknown command: {}", args.command).into(),
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_file_editor_create_and_view() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());

        let args_create = FileEditorArgs {
            command: "create".to_string(),
            path: "test.txt".to_string(),
            file_text: Some("hello world".to_string()),
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        let result = run_file_editor(&args_create, dir.path(), &history)
            .await
            .unwrap();
        assert!(result.contains("created successfully"));

        let args_view = FileEditorArgs {
            command: "view".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        let content = run_file_editor(&args_view, dir.path(), &history)
            .await
            .unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_file_editor_replace_and_undo() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        // Replave
        let args_replace = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: Some("world".to_string()),
            new_str: Some("rust".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };

        run_file_editor(&args_replace, dir.path(), &history)
            .await
            .unwrap();

        // Verify replace
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello rust");

        // Undo
        let args_undo = FileEditorArgs {
            command: "undo_edit".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        run_file_editor(&args_undo, dir.path(), &history)
            .await
            .unwrap();

        // Verify undo
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }
}

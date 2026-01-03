use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::Tool;

#[derive(Clone)]
struct FileState {
    content: String,
    history: Vec<String>,
}

pub struct FileEditorTool {
    working_dir: PathBuf,
    file_states: Arc<Mutex<HashMap<String, FileState>>>,
}

impl FileEditorTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            working_dir,
            file_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_or_load_file(&self, path: &str) -> Result<FileState, String> {
        let mut states = self.file_states.lock().unwrap();

        if let Some(state) = states.get(path) {
            Ok(state.clone())
        } else {
            // Load file from disk
            let full_path = self.working_dir.join(path);
            let content = std::fs::read_to_string(&full_path)
                .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

            let state = FileState {
                content: content.clone(),
                history: vec![content],
            };
            states.insert(path.to_string(), state.clone());
            Ok(state)
        }
    }

    fn save_file_state(&self, path: &str, new_content: String) -> Result<(), String> {
        let mut states = self.file_states.lock().unwrap();

        if let Some(state) = states.get_mut(path) {
            state.history.push(state.content.clone());
            state.content = new_content.clone();
        } else {
            let state = FileState {
                content: new_content.clone(),
                history: vec![],
            };
            states.insert(path.to_string(), state);
        }

        // Write to disk
        let full_path = self.working_dir.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
        std::fs::write(&full_path, new_content)
            .map_err(|e| format!("Failed to write file '{}': {}", path, e))?;

        Ok(())
    }

    fn view_operation(&self, path: &str, start_line: Option<usize>, end_line: Option<usize>) -> Result<String, String> {
        let state = self.get_or_load_file(path)?;
        let lines: Vec<&str> = state.content.lines().collect();

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(lines.len()).min(lines.len());

        if start >= lines.len() {
            return Err(format!("Start line {} is beyond file length {}", start + 1, lines.len()));
        }

        let view_lines: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4} | {}", start + i + 1, line))
            .collect();

        Ok(format!(
            "Viewing '{}' (lines {}-{}):\n{}",
            path,
            start + 1,
            end,
            view_lines.join("\n")
        ))
    }

    fn insert_operation(&self, path: &str, line: usize, content: &str) -> Result<String, String> {
        let state = self.get_or_load_file(path)?;
        let mut lines: Vec<String> = state.content.lines().map(|s| s.to_string()).collect();

        let insert_pos = line.saturating_sub(1).min(lines.len());
        let new_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        for (i, new_line) in new_lines.iter().enumerate() {
            lines.insert(insert_pos + i, new_line.clone());
        }

        let new_content = lines.join("\n");
        if !state.content.is_empty() && !new_content.ends_with('\n') {
            self.save_file_state(path, format!("{}\n", new_content))?;
        } else {
            self.save_file_state(path, new_content)?;
        }

        Ok(format!(
            "Inserted {} line(s) at line {} in '{}'",
            new_lines.len(),
            line,
            path
        ))
    }

    fn replace_operation(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
        content: &str,
    ) -> Result<String, String> {
        let state = self.get_or_load_file(path)?;
        let mut lines: Vec<String> = state.content.lines().map(|s| s.to_string()).collect();

        let start = start_line.saturating_sub(1);
        let end = end_line.min(lines.len());

        if start >= lines.len() {
            return Err(format!("Start line {} is beyond file length {}", start_line, lines.len()));
        }

        // Remove old lines
        lines.drain(start..end);

        // Insert new lines
        let new_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        for (i, new_line) in new_lines.iter().enumerate() {
            lines.insert(start + i, new_line.clone());
        }

        let new_content = lines.join("\n");
        if !state.content.is_empty() && !new_content.ends_with('\n') {
            self.save_file_state(path, format!("{}\n", new_content))?;
        } else {
            self.save_file_state(path, new_content)?;
        }

        Ok(format!(
            "Replaced lines {}-{} with {} line(s) in '{}'",
            start_line,
            end_line,
            new_lines.len(),
            path
        ))
    }

    fn delete_operation(&self, path: &str, start_line: usize, end_line: usize) -> Result<String, String> {
        let state = self.get_or_load_file(path)?;
        let mut lines: Vec<String> = state.content.lines().map(|s| s.to_string()).collect();

        let start = start_line.saturating_sub(1);
        let end = end_line.min(lines.len());

        if start >= lines.len() {
            return Err(format!("Start line {} is beyond file length {}", start_line, lines.len()));
        }

        let deleted_count = end - start;
        lines.drain(start..end);

        let new_content = lines.join("\n");
        if !state.content.is_empty() && !new_content.ends_with('\n') {
            self.save_file_state(path, format!("{}\n", new_content))?;
        } else {
            self.save_file_state(path, new_content)?;
        }

        Ok(format!(
            "Deleted {} line(s) ({}-{}) from '{}'",
            deleted_count, start_line, end_line, path
        ))
    }

    fn undo_operation(&self, path: &str) -> Result<String, String> {
        let mut states = self.file_states.lock().unwrap();

        if let Some(state) = states.get_mut(path) {
            if let Some(previous_content) = state.history.pop() {
                state.content = previous_content.clone();

                // Write to disk
                let full_path = self.working_dir.join(path);
                std::fs::write(&full_path, &previous_content)
                    .map_err(|e| format!("Failed to write file '{}': {}", path, e))?;

                Ok(format!("Undid last change to '{}'", path))
            } else {
                Err(format!("No history available for '{}'", path))
            }
        } else {
            Err(format!("File '{}' not in edit session", path))
        }
    }
}

#[async_trait]
impl Tool for FileEditorTool {
    fn name(&self) -> String {
        "file_editor".to_string()
    }

    fn description(&self) -> String {
        format!(
            "Structured file editing tool. Supports view, insert, replace, delete, and undo operations. \
            Your current working directory is: {}",
            self.working_dir.display()
        )
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["view", "insert", "replace", "delete", "undo"],
                    "description": "The operation to perform"
                },
                "path": {
                    "type": "string",
                    "description": "Relative path to the file"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Starting line number (1-indexed, for view/replace/delete)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Ending line number (1-indexed, for view/replace/delete)"
                },
                "line": {
                    "type": "integer",
                    "description": "Line number for insert operation (1-indexed)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to insert or replace"
                }
            },
            "required": ["operation", "path"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'operation' argument")?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'path' argument")?;

        match operation {
            "view" => {
                let start_line = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
                let end_line = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);
                self.view_operation(path, start_line, end_line)
            }
            "insert" => {
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'line' argument for insert")? as usize;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'content' argument for insert")?;
                self.insert_operation(path, line, content)
            }
            "replace" => {
                let start_line = args
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'start_line' argument for replace")? as usize;
                let end_line = args
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'end_line' argument for replace")? as usize;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'content' argument for replace")?;
                self.replace_operation(path, start_line, end_line, content)
            }
            "delete" => {
                let start_line = args
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'start_line' argument for delete")? as usize;
                let end_line = args
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing 'end_line' argument for delete")? as usize;
                self.delete_operation(path, start_line, end_line)
            }
            "undo" => self.undo_operation(path),
            _ => Err(format!("Unknown operation: {}", operation)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_editor_view() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "line1\nline2\nline3\n").unwrap();

        let tool = FileEditorTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "operation": "view",
            "path": "test.txt",
            "start_line": 1,
            "end_line": 2
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(!result.contains("line3"));
    }

    #[tokio::test]
    async fn test_file_editor_insert() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "line1\nline3\n").unwrap();

        let tool = FileEditorTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "operation": "insert",
            "path": "test.txt",
            "line": 2,
            "content": "line2"
        });

        tool.call(args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
    }

    #[tokio::test]
    async fn test_file_editor_replace() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "line1\nline2\nline3\n").unwrap();

        let tool = FileEditorTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "operation": "replace",
            "path": "test.txt",
            "start_line": 2,
            "end_line": 2,
            "content": "new_line2"
        });

        tool.call(args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert_eq!(content, "line1\nnew_line2\nline3\n");
    }

    #[tokio::test]
    async fn test_file_editor_delete() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "line1\nline2\nline3\n").unwrap();

        let tool = FileEditorTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "operation": "delete",
            "path": "test.txt",
            "start_line": 2,
            "end_line": 2
        });

        tool.call(args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert_eq!(content, "line1\nline3\n");
    }

    #[tokio::test]
    async fn test_file_editor_undo() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "original\n").unwrap();

        let tool = FileEditorTool::new(temp_path.to_path_buf());

        // Make a change
        let args = serde_json::json!({
            "operation": "replace",
            "path": "test.txt",
            "start_line": 1,
            "end_line": 1,
            "content": "modified"
        });
        tool.call(args).await.unwrap();

        // Undo
        let undo_args = serde_json::json!({
            "operation": "undo",
            "path": "test.txt"
        });
        tool.call(undo_args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert_eq!(content, "original\n");
    }
}

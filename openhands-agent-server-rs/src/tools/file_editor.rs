use rmcp::model::ErrorCode;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

const SNIPPET_CONTEXT_WINDOW: usize = 4;

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

fn make_output(snippet_content: &str, snippet_description: &str, start_line: usize) -> String {
    let lines: Vec<&str> = snippet_content.lines().collect();
    let numbered_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:6}\t{}", i + start_line, line))
        .collect();

    format!(
        "Here's the result of running `cat -n` on {}:\n{}\n",
        snippet_description,
        numbered_lines.join("\n")
    )
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
                    message: format!(
                        "The path {} does not exist. Please provide a valid path.",
                        path.display()
                    )
                    .into(),
                    data: None,
                });
            }
            if path.is_dir() {
                let mut formatted_paths = Vec::new();
                // Read dir up to depth 2 (simulated basic logic)
                // For now, simpler implementation than full recursive walk with exclude hidden
                // just to show intent.
                // Actually, let's just list 1 level for simplicity as walkdir is cleaner but maybe overkill if not already used.
                // But wait, we imported WalkDir in grep.rs, so we can use it if we add it to Cargo.toml or just use fs::read_dir.
                // Original impl uses `find ... -maxdepth 2`.
                // Let's stick to simple flat list for now to avoid complexity unless requested.
                // User said "If original tool also is not that much verbose it is okay".
                // But for directories, verbosity IS helpful.
                // Let's rely on standard message for now.

                // Using fs::read_dir
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if !name.starts_with('.') {
                                if entry.path().is_dir() {
                                    formatted_paths.push(format!("{}/", name));
                                } else {
                                    formatted_paths.push(name);
                                }
                            }
                        }
                        formatted_paths.sort();
                        Ok(format!(
                            "Here's the files and directories in {}, excluding hidden items:\n{}",
                            path.display(),
                            formatted_paths.join("\n")
                        ))
                    }
                    Err(e) => Err(McpError {
                        code: ErrorCode(-32603),
                        message: format!("Failed to list directory: {}", e).into(),
                        data: None,
                    }),
                }
            } else {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let num_lines = lines.len();
                        let (start_line, end_line) = if let Some(range) = &args.view_range {
                            if range.len() != 2 {
                                return Err(McpError {
                                    code: ErrorCode(-32602),
                                    message: "view_range should be a list of two integers.".into(),
                                    data: None,
                                });
                            }
                            let s = range[0] as usize;
                            let e = range[1] as usize;
                            if s < 1 || s > num_lines {
                                return Err(McpError {
                                    code: ErrorCode(-32602),
                                    message: format!("Its first element `{}` should be within the range of lines of the file: [1, {}].", s, num_lines).into(),
                                    data: None,
                                 });
                            }
                            if e < s {
                                return Err(McpError {
                                    code: ErrorCode(-32602),
                                    message: format!("Its second element `{}` should be greater than or equal to the first element `{}`.", e, s).into(),
                                    data: None,
                                });
                            }
                            (s, e)
                        } else {
                            (1, num_lines)
                        };

                        let end_line = std::cmp::min(end_line, num_lines);
                        let snippet_lines = lines
                            .iter()
                            .skip(start_line - 1)
                            .take(end_line - start_line + 1)
                            .cloned()
                            .collect::<Vec<&str>>()
                            .join("\n");

                        Ok(make_output(
                            &snippet_lines,
                            &path.to_string_lossy(),
                            start_line,
                        ))
                    }
                    Err(e) => Err(McpError {
                        code: ErrorCode(-32603),
                        message: format!("Failed to read file: {}", e).into(),
                        data: None,
                    }),
                }
            }
        }
        "create" => {
            if path.exists() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!("File already exists at: {}. Cannot overwrite files using command `create`.", path.display()).into(),
                    data: None,
                });
            }
            let content = args.file_text.clone().ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing file_text".into(),
                data: None,
            })?;
            fs::write(&path, &content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write to {}: {}", path.display(), e).into(),
                data: None,
            })?;
            Ok(format!("File created successfully at: {}", path.display()))
        }
        "str_replace" => {
            if !path.exists() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!("The path {} does not exist.", path.display()).into(),
                    data: None,
                });
            }
            let old_str = args.old_str.clone().ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing old_str".into(),
                data: None,
            })?;
            let new_str = args.new_str.clone().ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing new_str".into(),
                data: None,
            })?;

            if old_str == new_str {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message:
                        "No replacement was performed. `new_str` and `old_str` must be different."
                            .into(),
                    data: None,
                });
            }

            let content = fs::read_to_string(&path).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to read file: {}", e).into(),
                data: None,
            })?;

            // Find occurrences logic
            let occurrences: Vec<_> = content.match_indices(&old_str).collect();

            if occurrences.is_empty() {
                // Try trimming (simplified version of python logic)
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!(
                        "No replacement was performed, old_str `{}` did not appear verbatim in {}.",
                        old_str,
                        path.display()
                    )
                    .into(),
                    data: None,
                });
            }
            if occurrences.len() > 1 {
                let line_numbers: Vec<usize> = occurrences
                    .iter()
                    .map(|(idx, _)| content[..*idx].chars().filter(|&c| c == '\n').count() + 1)
                    .collect();
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!("No replacement was performed. Multiple occurrences of old_str `{}` in lines {:?}. Please ensure it is unique.", old_str, line_numbers).into(),
                    data: None,
                });
            }

            let (idx, matched_text) = occurrences[0];
            let replacement_line = content[..idx].chars().filter(|&c| c == '\n').count() + 1;

            let new_content = format!(
                "{}{}{}",
                &content[..idx],
                new_str,
                &content[idx + matched_text.len()..]
            );

            // Save history
            {
                let mut history = editor_history.lock().await;
                history
                    .entry(path.clone())
                    .or_default()
                    .push(content.clone());
            }

            fs::write(&path, &new_content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write file: {}", e).into(),
                data: None,
            })?;

            // Create snippet
            // Create snippet
            let start_line = replacement_line.saturating_sub(SNIPPET_CONTEXT_WINDOW);
            let end_line =
                replacement_line + SNIPPET_CONTEXT_WINDOW + new_str.matches('\n').count();

            let lines: Vec<&str> = new_content.lines().collect();
            // Adjust for make_output
            let snippet_display_start_line = start_line + 1; // if 0 -> 1

            // Slicing logic on vector:
            let s_idx = start_line; // 0-based index to start reading from
            let output_snippet = lines
                .iter()
                .skip(s_idx)
                .take(end_line - s_idx)
                .cloned()
                .collect::<Vec<&str>>()
                .join("\n");

            Ok(format!(
                "The file {} has been edited. {}Review the changes and make sure they are as expected. Edit the file again if necessary.",
                 path.display(),
                 make_output(&output_snippet, &format!("a snippet of {}", path.display()), snippet_display_start_line)
            ))
        }
        "insert" => {
            let insert_line = args.insert_line.ok_or_else(|| McpError {
                code: ErrorCode(-32602),
                message: "Missing insert_line".into(),
                data: None,
            })?;
            let text_to_insert =
                args.new_str
                    .clone()
                    .or(args.file_text.clone())
                    .ok_or_else(|| McpError {
                        code: ErrorCode(-32602),
                        message: "Missing new_str (or file_text)".into(),
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

            if idx > lines.len() {
                return Err(McpError {
                    code: ErrorCode(-32602),
                    message: format!(
                        "It should be within the range of allowed values: [0, {}]",
                        lines.len()
                    )
                    .into(),
                    data: None,
                });
            }

            let inserted_lines_count = text_to_insert.lines().count();

            if idx == lines.len() {
                lines.push(text_to_insert.clone());
            } else {
                lines.insert(idx, text_to_insert.clone());
            }

            let new_content = lines.join("\n");
            fs::write(&path, &new_content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write file: {}", e).into(),
                data: None,
            })?;

            // Snippet
            let start_line = (insert_line as usize).saturating_sub(SNIPPET_CONTEXT_WINDOW);
            let end_line = insert_line as usize + SNIPPET_CONTEXT_WINDOW + inserted_lines_count;

            let new_lines: Vec<&str> = new_content.lines().collect();
            let output_snippet = new_lines
                .iter()
                .skip(start_line)
                .take(end_line - start_line)
                .cloned()
                .collect::<Vec<&str>>()
                .join("\n");

            Ok(format!(
                "The file {} has been edited. {}Review the changes and make sure they are as expected (correct indentation, no duplicate lines, etc). Edit the file again if necessary.",
                path.display(),
                make_output(&output_snippet, "a snippet of the edited file", start_line + 1)
            ))
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
                    return Ok(format!(
                        "Last edit to {} undone successfully. {}",
                        path.display(),
                        make_output(&prev_content, &path.to_string_lossy(), 1)
                    ));
                }
            }
            Err(McpError {
                code: ErrorCode(-32602),
                message: format!("No edit history found for {}", path.display()).into(),
                data: None,
            })
        }
        _ => Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Unrecognized command {}.", args.command).into(),
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
        assert!(content.contains("hello world"));
        assert!(content.contains("cat -n"));
    }

    #[tokio::test]
    async fn test_file_editor_replace_and_undo() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        // Replace
        let args_replace = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: Some("world".to_string()),
            new_str: Some("rust".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };

        let res = run_file_editor(&args_replace, dir.path(), &history)
            .await
            .unwrap();
        assert!(res.contains("edited"));
        // {:6} padding for "1" gives "     1"
        assert!(res.contains("     1\thello rust"));

        // Verify content
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

        let undo_res = run_file_editor(&args_undo, dir.path(), &history)
            .await
            .unwrap();
        assert!(undo_res.contains("undone successfully"));

        // Verify undo
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }
}

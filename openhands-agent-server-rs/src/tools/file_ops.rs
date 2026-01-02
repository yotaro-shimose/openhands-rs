use rmcp::model::ErrorCode;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ReadFileArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ListFilesArgs {
    pub path: String,
    pub recursive: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
}

const MAX_LINES_PER_READ: usize = 1000;

fn make_numbered_output(content: &str, start_line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let numbered_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:6}\t{}", i + start_line, line))
        .collect();
    numbered_lines.join("\n")
}

pub fn run_read_file(args: &ReadFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: File not found: {}", path.display()).into(),
            data: None,
        });
    }
    if path.is_dir() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: Path is a directory, not a file: {}", path.display()).into(),
            data: None,
        });
    }

    let content = fs::read_to_string(&path).map_err(|e| McpError {
        code: ErrorCode(-32603),
        message: format!("Error reading file: {}", e).into(),
        data: None,
    })?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let offset = args.offset.unwrap_or(0);

    if offset >= total_lines && total_lines > 0 {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!(
                "Error: Offset {} is beyond file length ({} lines)",
                offset, total_lines
            )
            .into(),
            data: None,
        });
    }

    let limit = args.limit.unwrap_or(MAX_LINES_PER_READ);
    let end = std::cmp::min(offset + limit, total_lines);

    let lines_to_show = &lines[offset..end];
    let content_to_show = lines_to_show.join("\n");
    let numbered_content = make_numbered_output(&content_to_show, offset + 1);

    let is_truncated = end < total_lines;
    let mut header = format!("Read file: {}", path.display());
    if is_truncated {
        header.push_str(&format!(
            " (showing lines {}-{} of {})",
            offset + 1,
            end,
            total_lines
        ));
        header.push_str(&format!(
            "\nTo read more, use: read_file(path='{}', offset={}, limit={})",
            args.path, end, limit
        ));
    }

    Ok(format!("{}\n\n{}", header, numbered_content))
}

pub fn run_write_file(args: &WriteFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if path.exists() && path.is_dir() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: Path is a directory, not a file: {}", path.display()).into(),
            data: None,
        });
    }

    let is_new_file = !path.exists();

    // Create parent dirs
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Failed to create parent directory: {}", e).into(),
            data: None,
        })?;
    }

    fs::write(&path, &args.content).map_err(|e| McpError {
        code: ErrorCode(-32603),
        message: format!("Error writing file: {}", e).into(),
        data: None,
    })?;

    let action_verb = if is_new_file { "Created" } else { "Updated" };
    Ok(format!("{} file: {}", action_verb, path.display()))
}

pub fn run_list_files(args: &ListFilesArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: Directory not found: {}", path.display()).into(),
            data: None,
        });
    }
    if !path.is_dir() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: Path is not a directory: {}", path.display()).into(),
            data: None,
        });
    }

    let mut entries = Vec::new();
    let recursive = args.recursive.unwrap_or(false);

    if recursive {
        // Simple recursive implementation (limit to 2 levels deep like Python)
        for entry in walkdir::WalkDir::new(&path)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let rel_path = entry.path().strip_prefix(&path).unwrap_or(entry.path());
            if rel_path.as_os_str().is_empty() {
                continue;
            }

            let name = rel_path.to_string_lossy().to_string();
            let type_str = if entry.file_type().is_dir() {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("{} ({})", name, type_str));
            if entries.len() >= 1000 {
                break;
            }
        }
    } else {
        let read_dir = fs::read_dir(&path).map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Error listing directory: {}", e).into(),
            data: None,
        })?;
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let type_str = if entry.path().is_dir() { "dir" } else { "file" };
            entries.push(format!("{} ({})", name, type_str));
            if entries.len() >= 1000 {
                break;
            }
        }
    }

    entries.sort();
    let total_count = entries.len();
    let truncated = total_count >= 1000;

    let mut header = format!(
        "Listed directory: {} ({} entries",
        path.display(),
        total_count
    );
    if truncated {
        header.push_str(", truncated to 1000");
    }
    header.push_str(")");

    Ok(format!("{}\n{}", header, entries.join("\n")))
}

pub fn run_delete_file(args: &DeleteFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Error: File not found: {}", path.display()).into(),
            data: None,
        });
    }

    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Error deleting directory: {}", e).into(),
            data: None,
        })?;
        Ok(format!("Deleted directory: {}", path.display()))
    } else {
        fs::remove_file(&path).map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Error deleting file: {}", e).into(),
            data: None,
        })?;
        Ok(format!("Deleted file: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_file_with_pagination() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\n").unwrap();

        let args = ReadFileArgs {
            path: "test.txt".to_string(),
            offset: Some(1),
            limit: Some(2),
        };

        let result = run_read_file(&args, dir.path()).unwrap();
        assert!(result.contains("Read file:"));
        assert!(result.contains("showing lines 2-3 of 4"));
        assert!(result.contains("     2\tline2"));
        assert!(result.contains("     3\tline3"));
        assert!(!result.contains("line1"));
    }

    #[test]
    fn test_write_file_new_and_update() {
        let dir = tempdir().unwrap();
        
        let args_create = WriteFileArgs {
            path: "new.txt".to_string(),
            content: "hello".to_string(),
        };
        let res1 = run_write_file(&args_create, dir.path()).unwrap();
        assert!(res1.contains("Created file"));
        
        let args_update = WriteFileArgs {
            path: "new.txt".to_string(),
            content: "world".to_string(),
        };
        let res2 = run_write_file(&args_update, dir.path()).unwrap();
        assert!(res2.contains("Updated file"));
    }

    #[test]
    fn test_list_files_basic() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("f1.txt"), "").unwrap();
        fs::create_dir(dir.path().join("d1")).unwrap();

        let args = ListFilesArgs {
            path: ".".to_string(),
            recursive: Some(false),
        };
        let result = run_list_files(&args, dir.path()).unwrap();
        assert!(result.contains("f1.txt (file)"));
        assert!(result.contains("d1 (dir)"));
    }

    #[test]
    fn test_delete_file_and_dir() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f.txt");
        let sub_dir = dir.path().join("d");
        fs::write(&file_path, "").unwrap();
        fs::create_dir(&sub_dir).unwrap();

        let args_f = DeleteFileArgs { path: "f.txt".to_string() };
        run_delete_file(&args_f, dir.path()).unwrap();
        assert!(!file_path.exists());

        let args_d = DeleteFileArgs { path: "d".to_string() };
        run_delete_file(&args_d, dir.path()).unwrap();
        assert!(!sub_dir.exists());
    }
}

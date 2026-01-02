use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Tool;

pub struct GrepTool {
    working_dir: PathBuf,
}

impl GrepTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    fn search_directory(
        &self,
        dir: &Path,
        pattern: &Regex,
        include_filter: Option<&Regex>,
        matches: &mut Vec<(PathBuf, SystemTime)>,
    ) -> Result<(), String> {
        if matches.len() >= 100 {
            return Ok(());
        }

        let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;

        for entry in entries {
            if matches.len() >= 100 {
                break;
            }

            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            // Skip hidden files and directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }

            if path.is_dir() {
                // Recurse into subdirectory
                self.search_directory(&path, pattern, include_filter, matches)?;
            } else if path.is_file() {
                // Check include filter
                if let Some(filter) = include_filter {
                    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                        if !filter.is_match(filename) {
                            continue;
                        }
                    }
                }

                // Try to read and search file content
                if let Ok(content) = fs::read_to_string(&path) {
                    if pattern.is_match(&content) {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                matches.push((path.clone(), modified));
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> String {
        "grep".to_string()
    }

    fn description(&self) -> String {
        format!(
            "Fast content search tool. Searches file contents using regular expressions. \
            Supports full regex syntax. Filter files by pattern with the include parameter. \
            Returns matching file paths sorted by modification time. \
            Only the first 100 results are returned. \
            Your current working directory is: {}",
            self.working_dir.display()
        )
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "Optional directory to search in (defaults to working directory)"
                },
                "include": {
                    "type": "string",
                    "description": "Optional file pattern to filter which files to search (e.g., '*.js', '*.{ts,tsx}')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let pattern_str = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'pattern' argument")?;

        // Compile regex pattern (case-insensitive)
        let pattern = Regex::new(&format!("(?i){}", pattern_str))
            .map_err(|e| format!("Invalid regex pattern: {}", e))?;

        let search_path = if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
            PathBuf::from(path_str)
        } else {
            self.working_dir.clone()
        };

        // Validate search path
        if !search_path.is_dir() {
            return Err(format!(
                "Search path '{}' is not a valid directory",
                search_path.display()
            ));
        }

        // Parse include filter if provided
        let include_filter = if let Some(include_str) = args.get("include").and_then(|v| v.as_str())
        {
            // Convert glob pattern to regex
            let regex_pattern = include_str
                .replace(".", "\\.")
                .replace("*", ".*")
                .replace("{", "(")
                .replace("}", ")")
                .replace(",", "|");
            Some(
                Regex::new(&format!("^{}$", regex_pattern))
                    .map_err(|e| format!("Invalid include pattern: {}", e))?,
            )
        } else {
            None
        };

        // Search for matches
        let mut matches = Vec::new();
        self.search_directory(&search_path, &pattern, include_filter.as_ref(), &mut matches)?;

        // Sort by modification time (newest first)
        matches.sort_by(|a, b| b.1.cmp(&a.1));

        let truncated = matches.len() >= 100;
        let file_paths: Vec<String> = matches
            .into_iter()
            .map(|(path, _)| path.to_string_lossy().to_string())
            .collect();

        // Format output
        let include_info = if let Some(inc) = args.get("include").and_then(|v| v.as_str()) {
            format!(" (filtered by '{}')", inc)
        } else {
            String::new()
        };

        if file_paths.is_empty() {
            Ok(format!(
                "No files found containing pattern '{}' in directory '{}'{}",
                pattern_str,
                search_path.display(),
                include_info
            ))
        } else {
            let mut output = format!(
                "Found {} file(s) containing pattern '{}' in '{}'{}:\n{}",
                file_paths.len(),
                pattern_str,
                search_path.display(),
                include_info,
                file_paths.join("\n")
            );

            if truncated {
                output.push_str(
                    "\n\n[Results truncated to first 100 files. Consider using a more specific pattern.]"
                );
            }

            Ok(output)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_grep_basic() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create test files
        fs::write(temp_path.join("file1.txt"), "Hello world").unwrap();
        fs::write(temp_path.join("file2.txt"), "Goodbye world").unwrap();
        fs::write(temp_path.join("file3.txt"), "Something else").unwrap();

        let tool = GrepTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "world"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("file1.txt"));
        assert!(result.contains("file2.txt"));
        assert!(!result.contains("file3.txt"));
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "HELLO WORLD").unwrap();

        let tool = GrepTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "hello"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_grep_with_include_filter() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "content").unwrap();
        fs::write(temp_path.join("test.rs"), "content").unwrap();

        let tool = GrepTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "content",
            "include": "*.rs"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("test.rs"));
        assert!(!result.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_grep_regex() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("file1.txt"), "error: something failed").unwrap();
        fs::write(temp_path.join("file2.txt"), "warning: be careful").unwrap();

        let tool = GrepTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "error.*failed"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("file1.txt"));
        assert!(!result.contains("file2.txt"));
    }
}

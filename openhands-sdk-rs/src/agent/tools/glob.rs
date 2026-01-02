use async_trait::async_trait;
use glob::glob;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use super::Tool;

pub struct GlobTool {
    working_dir: PathBuf,
}

impl GlobTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> String {
        "glob".to_string()
    }

    fn description(&self) -> String {
        format!(
            "Fast file pattern matching tool. Supports glob patterns like '**/*.js' or 'src/**/*.ts'. \
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
                    "description": "The glob pattern to match files (e.g., '**/*.js', 'src/**/*.ts')"
                },
                "path": {
                    "type": "string",
                    "description": "Optional directory to search in (defaults to working directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'pattern' argument")?;

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

        // Build full glob pattern
        let full_pattern = search_path.join(pattern);
        let pattern_str = full_pattern
            .to_str()
            .ok_or("Invalid path encoding")?;

        // Execute glob search
        let mut matches: Vec<(PathBuf, SystemTime)> = Vec::new();
        
        for entry in glob(pattern_str).map_err(|e| e.to_string())? {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        if let Ok(metadata) = fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                matches.push((path, modified));
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Glob error: {}", e);
                }
            }

            // Limit to 100 files
            if matches.len() >= 100 {
                break;
            }
        }

        // Sort by modification time (newest first)
        matches.sort_by(|a, b| b.1.cmp(&a.1));

        let truncated = matches.len() >= 100;
        let file_paths: Vec<String> = matches
            .into_iter()
            .map(|(path, _)| path.to_string_lossy().to_string())
            .collect();

        // Format output
        if file_paths.is_empty() {
            Ok(format!(
                "No files found matching pattern '{}' in directory '{}'",
                pattern,
                search_path.display()
            ))
        } else {
            let mut output = format!(
                "Found {} file(s) matching pattern '{}' in '{}':\n{}",
                file_paths.len(),
                pattern,
                search_path.display(),
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
    async fn test_glob_basic() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create test files
        fs::write(temp_path.join("test1.txt"), "content1").unwrap();
        fs::write(temp_path.join("test2.txt"), "content2").unwrap();
        fs::write(temp_path.join("test.rs"), "rust code").unwrap();

        let tool = GlobTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "*.txt"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("test1.txt"));
        assert!(result.contains("test2.txt"));
        assert!(!result.contains("test.rs"));
    }

    #[tokio::test]
    async fn test_glob_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create nested structure
        fs::create_dir_all(temp_path.join("src/utils")).unwrap();
        fs::write(temp_path.join("src/main.rs"), "main").unwrap();
        fs::write(temp_path.join("src/utils/helper.rs"), "helper").unwrap();

        let tool = GlobTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "**/*.rs"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("main.rs"));
        assert!(result.contains("helper.rs"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let tool = GlobTool::new(temp_path.to_path_buf());
        let args = serde_json::json!({
            "pattern": "*.nonexistent"
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("No files found"));
    }
}

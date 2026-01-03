use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use super::Tool;

pub struct ApplyPatchTool {
    working_dir: PathBuf,
}

impl ApplyPatchTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    fn parse_patch(&self, patch_text: &str) -> Result<Vec<FilePatch>, String> {
        let lines: Vec<&str> = patch_text.lines().collect();
        let mut patches = Vec::new();
        let mut i = 0;

        // Find "*** Begin Patch" marker
        while i < lines.len() {
            if lines[i].trim() == "*** Begin Patch" {
                i += 1;
                break;
            }
            i += 1;
        }

        if i == 0 || i >= lines.len() {
            return Err("Patch must start with '*** Begin Patch'".to_string());
        }

        // Parse file patches until "*** End Patch"
        while i < lines.len() {
            if lines[i].trim() == "*** End Patch" {
                break;
            }

            // Look for file header (e.g., "--- a/path/to/file.txt")
            if lines[i].starts_with("--- ") {
                let old_file = lines[i]
                    .strip_prefix("--- ")
                    .and_then(|s| s.strip_prefix("a/"))
                    .unwrap_or(&lines[i][4..])
                    .trim();

                i += 1;
                if i >= lines.len() || !lines[i].starts_with("+++ ") {
                    return Err("Expected '+++ ' line after '--- ' line".to_string());
                }

                let new_file = lines[i]
                    .strip_prefix("+++ ")
                    .and_then(|s| s.strip_prefix("b/"))
                    .unwrap_or(&lines[i][4..])
                    .trim();

                i += 1;

                // Parse hunks for this file
                let mut hunks = Vec::new();
                while i < lines.len()
                    && !lines[i].starts_with("---")
                    && lines[i].trim() != "*** End Patch"
                {
                    if lines[i].starts_with("@@ ") {
                        // Parse hunk header
                        let hunk_start = i;
                        i += 1;

                        // Collect hunk lines
                        let mut hunk_lines = Vec::new();
                        while i < lines.len()
                            && !lines[i].starts_with("@@")
                            && !lines[i].starts_with("---")
                            && lines[i].trim() != "*** End Patch"
                        {
                            hunk_lines.push(lines[i].to_string());
                            i += 1;
                        }

                        hunks.push(Hunk {
                            header: lines[hunk_start].to_string(),
                            lines: hunk_lines,
                        });
                    } else {
                        i += 1;
                    }
                }

                patches.push(FilePatch {
                    old_path: old_file.to_string(),
                    new_path: new_file.to_string(),
                    hunks,
                });
            } else {
                i += 1;
            }
        }

        if patches.is_empty() {
            return Err("No valid patches found in input".to_string());
        }

        Ok(patches)
    }

    fn apply_file_patch(&self, file_patch: &FilePatch) -> Result<String, String> {
        let file_path = self.working_dir.join(&file_patch.new_path);

        // Read existing file or start with empty content
        let original_content = if file_path.exists() {
            std::fs::read_to_string(&file_path)
                .map_err(|e| format!("Failed to read '{}': {}", file_patch.new_path, e))?
        } else {
            String::new()
        };

        let mut lines: Vec<String> = original_content.lines().map(|s| s.to_string()).collect();

        // Apply each hunk
        for hunk in &file_patch.hunks {
            self.apply_hunk(&mut lines, hunk)?;
        }

        // Write modified content
        let new_content = lines.join("\n");
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        std::fs::write(&file_path, &new_content)
            .map_err(|e| format!("Failed to write '{}': {}", file_patch.new_path, e))?;

        Ok(format!("Applied patch to '{}'", file_patch.new_path))
    }

    fn apply_hunk(&self, lines: &mut Vec<String>, hunk: &Hunk) -> Result<(), String> {
        // Parse hunk header to get line numbers
        // Format: @@ -old_start,old_count +new_start,new_count @@
        let header_parts: Vec<&str> = hunk.header.split_whitespace().collect();
        if header_parts.len() < 3 {
            return Err(format!("Invalid hunk header: {}", hunk.header));
        }

        let old_range = header_parts[1].trim_start_matches('-');
        let old_start: usize = old_range
            .split(',')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or("Invalid old line number")?;

        // Build expected and new content from hunk
        let mut expected_lines = Vec::new();
        let mut new_lines = Vec::new();

        for line in &hunk.lines {
            if line.is_empty() {
                continue;
            }

            let first_char = line.chars().next().unwrap();
            let content = if line.len() > 1 { &line[1..] } else { "" };

            match first_char {
                '-' => {
                    expected_lines.push(content.to_string());
                }
                '+' => {
                    new_lines.push(content.to_string());
                }
                ' ' => {
                    expected_lines.push(content.to_string());
                    new_lines.push(content.to_string());
                }
                _ => {}
            }
        }

        // Find matching location (with fuzzy matching)
        let start_idx = old_start.saturating_sub(1);
        let end_idx = (start_idx + expected_lines.len()).min(lines.len());

        // Check if lines match
        let actual_lines: Vec<String> = if start_idx < lines.len() {
            lines[start_idx..end_idx].to_vec()
        } else {
            Vec::new()
        };

        // Simple fuzzy matching: allow if at least 70% of lines match
        let matching_lines = expected_lines
            .iter()
            .zip(actual_lines.iter())
            .filter(|(exp, act)| exp.trim() == act.trim())
            .count();

        let match_ratio = if expected_lines.is_empty() {
            1.0
        } else {
            matching_lines as f64 / expected_lines.len() as f64
        };

        if match_ratio < 0.7 {
            return Err(format!(
                "Hunk does not match file content ({}% match)",
                (match_ratio * 100.0) as usize
            ));
        }

        // Apply the change
        lines.splice(start_idx..end_idx, new_lines);

        Ok(())
    }
}

#[derive(Debug)]
struct FilePatch {
    #[allow(dead_code)]
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    header: String,
    lines: Vec<String>,
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> String {
        "apply_patch".to_string()
    }

    fn description(&self) -> String {
        format!(
            "Apply unified text patches to files. Input must start with '*** Begin Patch' and end with '*** End Patch'. \
            Your current working directory is: {}",
            self.working_dir.display()
        )
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Patch content following the '*** Begin Patch' ... '*** End Patch' format"
                }
            },
            "required": ["patch"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, String> {
        let patch_text = args
            .get("patch")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'patch' argument")?;

        // Parse the patch
        let file_patches = self.parse_patch(patch_text)?;

        // Apply each file patch
        let mut results = Vec::new();
        for file_patch in &file_patches {
            match self.apply_file_patch(file_patch) {
                Ok(msg) => results.push(msg),
                Err(e) => return Err(format!("Failed to apply patch: {}", e)),
            }
        }

        Ok(format!(
            "Successfully applied {} patch(es):\n{}",
            results.len(),
            results.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_apply_patch_basic() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create original file
        fs::write(temp_path.join("test.txt"), "line1\nline2\nline3\n").unwrap();

        let tool = ApplyPatchTool::new(temp_path.to_path_buf());

        let patch = r#"*** Begin Patch
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 line1
-line2
+modified_line2
 line3
*** End Patch"#;

        let args = serde_json::json!({
            "patch": patch
        });

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("Successfully applied"));

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert!(content.contains("modified_line2"));
        assert!(!content.contains("line1\nline2\nline3"));
        // Verify the structure is correct
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1], "modified_line2");
    }

    #[tokio::test]
    async fn test_apply_patch_add_lines() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(temp_path.join("test.txt"), "line1\nline3\n").unwrap();

        let tool = ApplyPatchTool::new(temp_path.to_path_buf());

        let patch = r#"*** Begin Patch
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,3 @@
 line1
+line2
 line3
*** End Patch"#;

        let args = serde_json::json!({
            "patch": patch
        });

        tool.call(args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("test.txt")).unwrap();
        assert!(content.contains("line2"));
    }

    #[tokio::test]
    async fn test_apply_patch_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let tool = ApplyPatchTool::new(temp_path.to_path_buf());

        let patch = r#"*** Begin Patch
--- a/newfile.txt
+++ b/newfile.txt
@@ -0,0 +1,2 @@
+line1
+line2
*** End Patch"#;

        let args = serde_json::json!({
            "patch": patch
        });

        tool.call(args).await.unwrap();

        let content = fs::read_to_string(temp_path.join("newfile.txt")).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
    }
}

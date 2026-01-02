use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct DiffError(String);

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DiffError {}

#[derive(Debug, Clone, PartialEq)]
enum ActionType {
    Add,
    Delete,
    Update,
}

#[derive(Debug, Clone)]
struct Chunk {
    orig_index: usize,
    del_lines: Vec<String>,
    ins_lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct PatchAction {
    action_type: ActionType,
    new_file: Option<String>,
    chunks: Vec<Chunk>,
    move_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Patch {
    actions: HashMap<String, PatchAction>,
}

struct Parser<'a> {
    current_files: &'a HashMap<String, String>,
    lines: Vec<&'a str>,
    index: usize,
    patch: Patch,
    fuzz: usize,
}

impl<'a> Parser<'a> {
    fn new(current_files: &'a HashMap<String, String>, lines: Vec<&'a str>) -> Self {
        Self {
            current_files,
            lines,
            index: 1, // Skip "*** Begin Patch" which is checked before
            patch: Patch::default(),
            fuzz: 0,
        }
    }

    fn is_done(&self, prefixes: &[&str]) -> bool {
        if self.index >= self.lines.len() {
            return true;
        }
        let line = self.lines[self.index];
        for prefix in prefixes {
            if line.starts_with(prefix) {
                return true;
            }
        }
        false
    }

    fn read_str(&mut self, prefix: &str) -> Option<String> {
        if self.index >= self.lines.len() {
            return None;
        }
        let line = self.lines[self.index];
        if line.starts_with(prefix) {
            let text = line[prefix.len()..].to_string();
            self.index += 1;
            Some(text)
        } else {
            None
        }
    }

    fn parse(&mut self) -> Result<(), DiffError> {
        while !self.is_done(&["*** End Patch"]) {
            if let Some(path) = self.read_str("*** Update File: ") {
                if self.patch.actions.contains_key(&path) {
                    return Err(DiffError(format!(
                        "Update File Error: Duplicate Path: {}",
                        path
                    )));
                }
                let move_to = self.read_str("*** Move to: ");

                let text = self.current_files.get(&path).ok_or_else(|| {
                    DiffError(format!("Update File Error: Missing File: {}", path))
                })?;

                let mut action = self.parse_update_file(text)?;
                action.move_path = move_to;
                self.patch.actions.insert(path, action);
                continue;
            }
            if let Some(path) = self.read_str("*** Delete File: ") {
                if self.patch.actions.contains_key(&path) {
                    return Err(DiffError(format!(
                        "Delete File Error: Duplicate Path: {}",
                        path
                    )));
                }
                if !self.current_files.contains_key(&path) {
                    return Err(DiffError(format!(
                        "Delete File Error: Missing File: {}",
                        path
                    )));
                }
                self.patch.actions.insert(
                    path,
                    PatchAction {
                        action_type: ActionType::Delete,
                        new_file: None,
                        chunks: Vec::new(),
                        move_path: None,
                    },
                );
                continue;
            }
            if let Some(path) = self.read_str("*** Add File: ") {
                if self.patch.actions.contains_key(&path) {
                    return Err(DiffError(format!(
                        "Add File Error: Duplicate Path: {}",
                        path
                    )));
                }
                let action = self.parse_add_file()?;
                self.patch.actions.insert(path, action);
                continue;
            }
            return Err(DiffError(format!(
                "Unknown Line: {}",
                self.lines[self.index]
            )));
        }
        if self.index >= self.lines.len() || self.lines[self.index] != "*** End Patch" {
            return Err(DiffError("Missing End Patch".to_string()));
        }
        self.index += 1;
        Ok(())
    }

    fn parse_update_file(&mut self, text: &str) -> Result<PatchAction, DiffError> {
        let mut action = PatchAction {
            action_type: ActionType::Update,
            new_file: None,
            chunks: Vec::new(),
            move_path: None,
        };
        let lines: Vec<&str> = text.split('\n').collect();
        let mut index = 0;

        let terminators = [
            "*** End Patch",
            "*** Update File:",
            "*** Delete File:",
            "*** Add File:",
            "*** End of File",
        ];

        while !self.is_done(&terminators) {
            let mut def_str_opt = self.read_str("@@ ");
            let mut section_str = "";

            if def_str_opt.is_none() {
                if self.lines[self.index] == "@@" {
                    section_str = "@@";
                    self.index += 1;
                }
            }

            if def_str_opt.is_none() && section_str.is_empty() && index != 0 {
                return Err(DiffError(format!(
                    "Invalid Line:\n{}",
                    self.lines[self.index]
                )));
            }

            // Fuzzy logic to find start index
            // Simplified for Rust implementation: exact -> whitespace ignore
            // Python implementation handles more cases.
            if let Some(def_str) = &def_str_opt {
                if !def_str.trim().is_empty() {
                    let mut found = false;
                    // Assuming sequential access, but we can search.
                    // Try exact match in remaining lines
                    for (i, line) in lines.iter().enumerate().skip(index) {
                        if *line == *def_str {
                            index = i + 1;
                            found = true;
                            break;
                        }
                    }

                    if !found {
                        // Try stripped match
                        for (i, line) in lines.iter().enumerate().skip(index) {
                            if line.trim() == def_str.trim() {
                                index = i + 1;
                                self.fuzz += 1;
                                break;
                            }
                        }
                    }
                }
            }

            let (next_chunk_context, chunks, end_patch_index, eof) =
                peek_next_section(&self.lines, self.index)?;

            let (new_index, fuzz) = find_context(&lines, &next_chunk_context, index, eof);
            println!("Context match: new_index={}, fuzz={}", new_index, fuzz);
            if new_index == usize::MAX {
                let context_str = next_chunk_context.join("\n");
                return Err(DiffError(format!(
                    "Invalid Context {}:\n{}",
                    index, context_str
                )));
            }

            self.fuzz += fuzz;
            for mut ch in chunks {
                println!("Chunk before offset: {:?}", ch);
                ch.orig_index += new_index;
                println!("Chunk after offset: {:?}", ch);
                action.chunks.push(ch);
            }
            index = new_index + next_chunk_context.len();
            self.index = end_patch_index;
        }
        Ok(action)
    }

    fn parse_add_file(&mut self) -> Result<PatchAction, DiffError> {
        let mut lines = Vec::new();
        let terminators = [
            "*** End Patch",
            "*** Update File:",
            "*** Delete File:",
            "*** Add File:",
        ];
        while !self.is_done(&terminators) {
            let line = self.lines[self.index];
            if !line.starts_with('+') {
                return Err(DiffError(format!("Invalid Add File Line: {}", line)));
            }
            lines.push(line[1..].to_string());
            self.index += 1;
        }
        Ok(PatchAction {
            action_type: ActionType::Add,
            new_file: Some(lines.join("\n")),
            chunks: Vec::new(),
            move_path: None,
        })
    }
}

fn peek_next_section<'a>(
    lines: &[&'a str],
    mut index: usize,
) -> Result<(Vec<String>, Vec<Chunk>, usize, bool), DiffError> {
    let mut old: Vec<String> = Vec::new();
    let mut del_lines: Vec<String> = Vec::new();
    let mut ins_lines: Vec<String> = Vec::new();
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut mode = "keep";
    let orig_index = index;

    while index < lines.len() {
        let s = lines[index];
        if s.starts_with("@@")
            || s.starts_with("*** End Patch")
            || s.starts_with("*** Update File:")
            || s.starts_with("*** Delete File:")
            || s.starts_with("*** Add File:")
            || s.starts_with("*** End of File")
        {
            break;
        }
        if s == "***" {
            break;
        } else if s.starts_with("***") {
            return Err(DiffError(format!("Invalid Line: {}", s)));
        }

        index += 1;
        let last_mode = mode;
        let line_content = if s.is_empty() { " " } else { s };

        let char0 = line_content.chars().next().unwrap_or(' ');
        let content = if line_content.len() > 1 {
            &line_content[1..]
        } else {
            ""
        };

        if char0 == '+' {
            mode = "add";
        } else if char0 == '-' {
            mode = "delete";
        } else if char0 == ' ' {
            mode = "keep";
        } else {
            return Err(DiffError(format!("Invalid Line: {}", s)));
        }

        if mode == "keep" && last_mode != mode {
            if !ins_lines.is_empty() || !del_lines.is_empty() {
                chunks.push(Chunk {
                    orig_index: old.len() - del_lines.len(),
                    del_lines: del_lines.clone(),
                    ins_lines: ins_lines.clone(),
                });
            }
            del_lines.clear();
            ins_lines.clear();
        }

        if mode == "add" {
            ins_lines.push(content.to_string());
        } else if mode == "delete" {
            del_lines.push(content.to_string());
            old.push(content.to_string());
        } else if mode == "keep" {
            old.push(content.to_string());
        }
    }

    if !ins_lines.is_empty() || !del_lines.is_empty() {
        chunks.push(Chunk {
            orig_index: old.len() - del_lines.len(),
            del_lines: del_lines.clone(),
            ins_lines: ins_lines.clone(),
        });
    }

    let mut eof = false;
    if index < lines.len() && lines[index] == "*** End of File" {
        index += 1;
        eof = true;
    }

    if index == orig_index {
        return Err(DiffError(format!(
            "Nothing in this section - index={} {}",
            index, lines[index]
        )));
    }

    Ok((old, chunks, index, eof))
}

fn find_context(lines: &[&str], context: &[String], start: usize, eof: bool) -> (usize, usize) {
    if eof {
        let (idx, fuzz) =
            find_context_core(lines, context, lines.len().saturating_sub(context.len()));
        if idx != usize::MAX {
            return (idx, fuzz);
        }
        let (idx, fuzz) = find_context_core(lines, context, start);
        if idx != usize::MAX {
            return (idx, fuzz + 10000);
        }
    }
    find_context_core(lines, context, start)
}

fn find_context_core(lines: &[&str], context: &[String], start: usize) -> (usize, usize) {
    if context.is_empty() {
        return (start, 0);
    }

    // Exact match
    for i in start..lines.len() {
        if i + context.len() <= lines.len() {
            let mut match_found = true;
            for (j, ctx_line) in context.iter().enumerate() {
                if lines[i + j] != ctx_line {
                    match_found = false;
                    break;
                }
            }
            if match_found {
                return (i, 0);
            }
        }
    }

    // Whitespace ignore (rstrip)
    for i in start..lines.len() {
        if i + context.len() <= lines.len() {
            let mut match_found = true;
            for (j, ctx_line) in context.iter().enumerate() {
                if lines[i + j].trim_end() != ctx_line.trim_end() {
                    match_found = false;
                    break;
                }
            }
            if match_found {
                return (i, 1);
            }
        }
    }

    // Whitespace ignore (strip)
    for i in start..lines.len() {
        if i + context.len() <= lines.len() {
            let mut match_found = true;
            for (j, ctx_line) in context.iter().enumerate() {
                if lines[i + j].trim() != ctx_line.trim() {
                    match_found = false;
                    break;
                }
            }
            if match_found {
                return (i, 100);
            }
        }
    }

    (usize::MAX, 0)
}

fn get_updated_file(text: &str, action: &PatchAction, path: &str) -> Result<String, DiffError> {
    if action.action_type != ActionType::Update {
        panic!("Should only call on UPDATE");
    }
    let orig_lines: Vec<&str> = text.split('\n').collect();
    let mut dest_lines: Vec<&str> = Vec::new();
    let mut orig_index = 0;

    for chunk in &action.chunks {
        if chunk.orig_index > orig_lines.len() {
            return Err(DiffError(format!(
                "{}: chunk.orig_index {} > len(lines) {}",
                path,
                chunk.orig_index,
                orig_lines.len()
            )));
        }
        if orig_index > chunk.orig_index {
            return Err(DiffError(format!(
                "{}: orig_index {} > chunk.orig_index {}",
                path, orig_index, chunk.orig_index
            )));
        }

        dest_lines.extend_from_slice(&orig_lines[orig_index..chunk.orig_index]);
        orig_index = chunk.orig_index;

        for ins in &chunk.ins_lines {
            dest_lines.push(ins);
        }
        orig_index += chunk.del_lines.len();
    }

    dest_lines.extend_from_slice(&orig_lines[orig_index..]);
    Ok(dest_lines.join("\n"))
}

pub fn process_patch(
    text: &str,
    orig_files: HashMap<String, String>,
) -> Result<(String, usize, HashMap<String, Option<String>>), DiffError> {
    if !text.starts_with("*** Begin Patch") {
        return Err(DiffError("Invalid patch text".to_string()));
    }

    let lines: Vec<&str> = text.trim().split('\n').collect();
    if lines.last() != Some(&"*** End Patch") {
        return Err(DiffError("Missing End Patch".to_string()));
    }

    let mut parser = Parser::new(&orig_files, lines);
    parser.parse()?;

    let mut result_files = HashMap::new();

    for (path, action) in &parser.patch.actions {
        match action.action_type {
            ActionType::Delete => {
                result_files.insert(path.clone(), None);
            }
            ActionType::Add => {
                result_files.insert(path.clone(), Some(action.new_file.clone().unwrap()));
            }
            ActionType::Update => {
                let orig = orig_files.get(path).unwrap();
                let new_content = get_updated_file(orig, action, path)?;
                if let Some(move_to) = &action.move_path {
                    result_files.insert(path.clone(), None);
                    result_files.insert(move_to.clone(), Some(new_content));
                } else {
                    result_files.insert(path.clone(), Some(new_content));
                }
            }
        }
    }

    Ok(("Done!".to_string(), parser.fuzz, result_files))
}

pub fn identify_files_needed(text: &str) -> Vec<String> {
    let mut result = HashSet::new();
    for line in text.lines() {
        if let Some(stripped) = line.strip_prefix("*** Update File: ") {
            result.insert(stripped.to_string());
        }
        if let Some(stripped) = line.strip_prefix("*** Delete File: ") {
            result.insert(stripped.to_string());
        }
        // Add file doesn't need original file
    }
    result.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_add_file() {
        let patch_text = "*** Begin Patch\n*** Add File: new.txt\n+hello\n+world\n*** End Patch";
        let (_, _, results) = process_patch(patch_text, HashMap::new()).expect("Parse failed");
        assert_eq!(
            results.get("new.txt"),
            Some(&Some("hello\nworld".to_string()))
        );
    }

    #[test]
    fn test_parse_delete_file() {
        let mut orig = HashMap::new();
        orig.insert("old.txt".to_string(), "content".to_string());

        let patch_text = "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch";
        let (_, _, results) = process_patch(patch_text, orig).expect("Parse failed");
        assert_eq!(results.get("old.txt"), Some(&None));
    }

    #[test]
    fn test_parse_update_file_exact() {
        let mut orig = HashMap::new();
        orig.insert("file.txt".to_string(), "line1\nline2\nline3".to_string());

        let patch_text = r#"*** Begin Patch
*** Update File: file.txt
@@ -1,3 +1,3 @@
 line1
-line2
+line2_modified
 line3
*** End Patch"#;

        let (_, _, results) = process_patch(patch_text, orig).expect("Parse failed");
        let new_content = results.get("file.txt").unwrap().as_ref().unwrap();
        assert_eq!(new_content, "line1\nline2_modified\nline3");
    }

    #[test]
    fn test_parse_update_file_fuzzy() {
        let mut orig = HashMap::new();
        // Original has extra spaces
        orig.insert("file.txt".to_string(), "line1  \nline2\nline3".to_string());

        // Patch uses cleaner format
        let patch_text = r#"*** Begin Patch
*** Update File: file.txt
@@ -1,3 +1,3 @@
 line1
-line2
+line2_modified
 line3
*** End Patch"#;

        let (_, fuzz, results) = process_patch(patch_text, orig).expect("Parse failed");
        let new_content = results.get("file.txt").unwrap().as_ref().unwrap();
        // Should preserve the original's extra space in the context line if it wasn't modified
        // Actually our logic reconstructs using original context for kept lines?
        // Let's check implementation. get_updated_file uses orig_lines slices using indices derived from fuzzy match.
        // So "line1  " should remain.
        assert_eq!(new_content, "line1  \nline2_modified\nline3");
        assert!(fuzz > 0);
    }

    #[test]
    fn test_identify_files_needed() {
        let patch_text = r#"*** Begin Patch
*** Update File: file1.txt
...
*** Delete File: file2.txt
...
*** Add File: file3.txt
...
*** End Patch"#;
        let files = identify_files_needed(patch_text);
        assert!(files.contains(&"file1.txt".to_string()));
        assert!(files.contains(&"file2.txt".to_string()));
        // Add file doesn't need original
        assert!(!files.contains(&"file3.txt".to_string()));
    }
}

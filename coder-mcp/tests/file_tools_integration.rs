// Integration tests for file tools
// These tests verify that the file tools work correctly in realistic scenarios
// with multiple operations and state management

use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;
use tokio::sync::Mutex;

// Import the file tools functions
use coder_mcp::service::*;
use coder_mcp::tools::file_tools::*;

// Helper to create a test workspace
fn create_test_workspace() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    temp_dir
}

#[tokio::test]
async fn test_full_file_editing_workflow() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path().join("workspace");
    let history: Mutex<HashMap<std::path::PathBuf, Vec<String>>> = Mutex::new(HashMap::new());

    // 1. Create a file
    let create_args = CreateFileArgs {
        path: "project.rs".to_string(),
        content: "fn main() {\n    println!(\"Hello\");\n}".to_string(),
    };
    let result = run_create_file(&create_args, &workspace).await;
    assert!(result.is_ok());
    assert!(workspace.join("project.rs").exists());

    // 2. View the file
    let view_args = ViewFileArgs {
        path: "project.rs".to_string(),
        start_line: None,
        end_line: None,
    };
    let result = run_view_file(&view_args, &workspace).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Hello"));

    // 3. Replace text
    let replace_args = StrReplaceArgs {
        path: "project.rs".to_string(),
        old_str: "Hello".to_string(),
        new_str: "Hello, World".to_string(),
    };
    let result = run_str_replace(&replace_args, &workspace, &history).await;
    assert!(result.is_ok());

    // 4. Insert a line
    let insert_args = InsertLinesArgs {
        path: "project.rs".to_string(),
        insert_line: 2,
        content: "    // Added comment".to_string(),
    };
    let result = run_insert_lines(&insert_args, &workspace, &history).await;
    assert!(result.is_ok());

    // 5. Undo the insertion
    let undo_args = UndoEditArgs {
        path: "project.rs".to_string(),
    };
    let result = run_undo_edit(&undo_args, &workspace, &history).await;
    assert!(result.is_ok());

    // 6. Verify final state (should have the replacement but not the insertion)
    let content = fs::read_to_string(workspace.join("project.rs")).unwrap();
    assert!(content.contains("Hello, World"));
    assert!(!content.contains("Added comment"));

    // 7. Delete the file
    let delete_args = DeleteFileArgs {
        path: "project.rs".to_string(),
    };
    let result = run_delete_file(&delete_args, &workspace).await;
    assert!(result.is_ok());
    assert!(!workspace.join("project.rs").exists());
}

#[tokio::test]
async fn test_multi_file_project_workflow() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path().join("workspace");

    // Create a multi-file project structure
    let files = vec![
        ("src/main.rs", "fn main() {}"),
        ("src/lib.rs", "pub fn hello() {}"),
        ("Cargo.toml", "[package]\nname = \"test\""),
        ("README.md", "# Test Project"),
    ];

    for (path, content) in files {
        let create_args = CreateFileArgs {
            path: path.to_string(),
            content: content.to_string(),
        };
        let result = run_create_file(&create_args, &workspace).await;
        assert!(result.is_ok(), "Failed to create {}", path);
    }

    // List the src directory
    let list_args = ListDirectoryArgs {
        path: "src".to_string(),
    };
    let result = run_list_directory(&list_args, &workspace).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("main.rs"));
    assert!(output.contains("lib.rs"));

    // List root directory
    let list_args = ListDirectoryArgs {
        path: ".".to_string(),
    };
    let result = run_list_directory(&list_args, &workspace).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("src/"));
    assert!(output.contains("Cargo.toml"));
    assert!(output.contains("README.md"));
}

#[tokio::test]
async fn test_concurrent_edits_with_history() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path().join("workspace");
    let history: Mutex<HashMap<std::path::PathBuf, Vec<String>>> = Mutex::new(HashMap::new());

    // Create initial file
    let create_args = CreateFileArgs {
        path: "test.txt".to_string(),
        content: "version 0".to_string(),
    };
    run_create_file(&create_args, &workspace).await.unwrap();

    // Make multiple edits
    for i in 1..=5 {
        let replace_args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: format!("version {}", i - 1),
            new_str: format!("version {}", i),
        };
        let result = run_str_replace(&replace_args, &workspace, &history).await;
        assert!(result.is_ok(), "Edit {} failed", i);
    }

    // Verify final state
    let content = fs::read_to_string(workspace.join("test.txt")).unwrap();
    assert_eq!(content, "version 5");

    // Undo all edits one by one
    for i in (0..5).rev() {
        let undo_args = UndoEditArgs {
            path: "test.txt".to_string(),
        };
        run_undo_edit(&undo_args, &workspace, &history)
            .await
            .unwrap();

        let content = fs::read_to_string(workspace.join("test.txt")).unwrap();
        assert_eq!(content, format!("version {}", i));
    }
}

#[tokio::test]
async fn test_error_recovery_workflow() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path().join("workspace");
    let history: Mutex<HashMap<std::path::PathBuf, Vec<String>>> = Mutex::new(HashMap::new());

    // Try to view non-existent file (should return error message, not panic)
    let view_args = ViewFileArgs {
        path: "nonexistent.txt".to_string(),
        start_line: None,
        end_line: None,
    };
    let result = run_view_file(&view_args, &workspace).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Error"));

    // Try to delete non-existent file
    let delete_args = DeleteFileArgs {
        path: "nonexistent.txt".to_string(),
    };
    let result = run_delete_file(&delete_args, &workspace).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Error"));

    // Try to replace in non-existent file
    let replace_args = StrReplaceArgs {
        path: "nonexistent.txt".to_string(),
        old_str: "old".to_string(),
        new_str: "new".to_string(),
    };
    let result = run_str_replace(&replace_args, &workspace, &history).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Error"));

    // Create a file, then try invalid operations
    let create_args = CreateFileArgs {
        path: "test.txt".to_string(),
        content: "content".to_string(),
    };
    run_create_file(&create_args, &workspace).await.unwrap();

    // Try to create again (should fail)
    let result = run_create_file(&create_args, &workspace).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("already exists"));
}

#[tokio::test]
async fn test_complex_text_operations() {
    let temp_dir = create_test_workspace();
    let workspace = temp_dir.path().join("workspace");
    let history: Mutex<HashMap<std::path::PathBuf, Vec<String>>> = Mutex::new(HashMap::new());

    // Create a file with complex content
    let content = "Line 1: Introduction\nLine 2: Body\nLine 3: Conclusion\nLine 4: References";
    let create_args = CreateFileArgs {
        path: "document.txt".to_string(),
        content: content.to_string(),
    };
    run_create_file(&create_args, &workspace).await.unwrap();

    // View specific range
    let view_args = ViewFileArgs {
        path: "document.txt".to_string(),
        start_line: Some(2),
        end_line: Some(3),
    };
    let result = run_view_file(&view_args, &workspace).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("Body"));
    assert!(output.contains("Conclusion"));
    assert!(!output.contains("Introduction"));
    assert!(!output.contains("References"));

    // Replace multi-word text
    let replace_args = StrReplaceArgs {
        path: "document.txt".to_string(),
        old_str: "Line 2: Body".to_string(),
        new_str: "Line 2: Main Content".to_string(),
    };
    let result = run_str_replace(&replace_args, &workspace, &history).await;
    assert!(result.is_ok());

    // Insert at specific position
    let insert_args = InsertLinesArgs {
        path: "document.txt".to_string(),
        insert_line: 3,
        content: "Line 2.5: Additional Info".to_string(),
    };
    let result = run_insert_lines(&insert_args, &workspace, &history).await;
    assert!(result.is_ok());

    // Verify final content
    let final_content = fs::read_to_string(workspace.join("document.txt")).unwrap();
    let lines: Vec<&str> = final_content.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[1].contains("Main Content"));
    assert!(lines[2].contains("Additional Info"));
}

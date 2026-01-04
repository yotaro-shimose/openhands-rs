use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct TaskTrackerArgs {
    pub command: String, // "view" or "plan"
    pub task_list: Option<Vec<TaskItem>>,
}

#[derive(Deserialize, schemars::JsonSchema, Serialize, Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub title: String,
    pub notes: String,
    pub status: String, // "todo", "in_progress", "done"
}

pub fn run_task_tracker(args: &TaskTrackerArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let tasks_file = workspace_dir.join("tasks.json");

    // Load existing tasks
    let mut tasks: Vec<TaskItem> = if tasks_file.exists() {
        match fs::read_to_string(&tasks_file) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(e) => return Ok(format!("Error: Failed to read tasks.json: {}", e)),
        }
    } else {
        Vec::new()
    };

    match args.command.as_str() {
        "view" => {
            // Return current tasks
        }
        "plan" => {
            // Update tasks
            if let Some(new_tasks) = &args.task_list {
                tasks = new_tasks.clone();
                let content = match serde_json::to_string_pretty(&tasks) {
                    Ok(c) => c,
                    Err(e) => return Ok(format!("Error: Failed to serialize tasks: {}", e)),
                };
                if let Err(e) = fs::write(&tasks_file, content) {
                    return Ok(format!("Error: Failed to write tasks.json: {}", e));
                }
            }
        }
        _ => {
            return Ok(format!(
                "Error: Unknown command '{}'. Use 'view' or 'plan'.",
                args.command
            ));
        }
    }

    // Format output
    let mut output = String::new();
    for (i, task) in tasks.iter().enumerate() {
        let status_mark = match task.status.as_str() {
            "done" => "[x]",
            "in_progress" => "[/]",
            _ => "[ ]",
        };
        output.push_str(&format!(
            "{} {}. {} - {}\n",
            status_mark,
            i + 1,
            task.title,
            task.notes
        ));
    }
    if output.is_empty() {
        output = "No tasks in the list.".to_string();
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_task_tracker_plan_and_view() {
        let dir = tempdir().unwrap();

        let new_tasks = vec![
            TaskItem {
                title: "Task 1".to_string(),
                notes: "Notes 1".to_string(),
                status: "todo".to_string(),
            },
            TaskItem {
                title: "Task 2".to_string(),
                notes: "Notes 2".to_string(),
                status: "done".to_string(),
            },
        ];

        let args_plan = TaskTrackerArgs {
            command: "plan".to_string(),
            task_list: Some(new_tasks),
        };

        let result_plan = run_task_tracker(&args_plan, dir.path()).unwrap();
        assert!(result_plan.contains("[ ] 1. Task 1"));
        assert!(result_plan.contains("[x] 2. Task 2"));

        // Verify persistence
        let args_view = TaskTrackerArgs {
            command: "view".to_string(),
            task_list: None,
        };
        let result_view = run_task_tracker(&args_view, dir.path()).unwrap();
        assert_eq!(result_plan, result_view);
    }

    #[test]
    fn test_task_tracker_unknown_command_returns_ok() {
        let dir = tempdir().unwrap();
        let args = TaskTrackerArgs {
            command: "unknown".to_string(),
            task_list: None,
        };
        let result = run_task_tracker(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Unknown command"));
    }
}

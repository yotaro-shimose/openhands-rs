use rmcp::model::ErrorCode;
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
        let content = fs::read_to_string(&tasks_file).map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Failed to read tasks.json: {}", e).into(),
            data: None,
        })?;
        serde_json::from_str(&content).unwrap_or_default()
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
                let content = serde_json::to_string_pretty(&tasks).map_err(|e| McpError {
                    code: ErrorCode(-32603),
                    message: format!("Failed to serialize tasks: {}", e).into(),
                    data: None,
                })?;
                fs::write(&tasks_file, content).map_err(|e| McpError {
                    code: ErrorCode(-32603),
                    message: format!("Failed to write tasks.json: {}", e).into(),
                    data: None,
                })?;
            }
        }
        _ => {
            return Err(McpError {
                code: ErrorCode(-32602),
                message: format!("Unknown command: {}", args.command).into(),
                data: None,
            })
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
}

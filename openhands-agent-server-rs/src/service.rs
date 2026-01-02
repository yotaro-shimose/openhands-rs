use openhands_sdk_rs::models::{BashEvent, ExecuteBashRequest, FileReadRequest, FileWriteRequest};
use openhands_sdk_rs::runtime::bash::BashEventService;
use openhands_sdk_rs::runtime::file::FileService;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::tools::file_editor::{run_file_editor, FileEditorArgs};
use crate::tools::glob::{run_glob, GlobArgs};
use crate::tools::grep::{run_grep, GrepArgs};
use crate::tools::task_tracker::{run_task_tracker, TaskTrackerArgs};

#[derive(Clone)]
pub struct OpenHandsService {
    bash: Arc<BashEventService>,
    file: Arc<FileService>,
    editor_history: Arc<Mutex<HashMap<PathBuf, Vec<String>>>>,
    tool_router: ToolRouter<OpenHandsService>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ExecuteBashArgs {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReadFileArgs {
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ListFilesArgs {
    pub path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ApplyPatchArgs {
    pub patch: String,
}

#[tool_router]
impl OpenHandsService {
    pub fn new(bash: BashEventService, file: FileService) -> Self {
        Self {
            bash: Arc::new(bash),
            file: Arc::new(file),
            editor_history: Arc::new(Mutex::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "glob",
        description = "Fast file pattern matching tool. Finds files by name patterns (e.g. '**/*.js'). Returns matching file paths."
    )]
    async fn glob_files(
        &self,
        Parameters(args): Parameters<GlobArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_glob(&args, &self.file.workspace_dir)?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        name = "grep",
        description = "Fast content search tool. Searches file contents using regex. Returns matching file paths."
    )]
    async fn grep_files(
        &self,
        Parameters(args): Parameters<GrepArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_grep(&args, &self.file.workspace_dir)?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        name = "task_tracker",
        description = "Track and manage tasks. Command 'view' shows current tasks. 'plan' updates tasks."
    )]
    async fn task_tracker(
        &self,
        Parameters(args): Parameters<TaskTrackerArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_task_tracker(&args, &self.file.workspace_dir)?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        name = "file_editor",
        description = "Edit files. Commands: view, create, str_replace, insert, undo_edit."
    )]
    async fn file_editor(
        &self,
        Parameters(args): Parameters<FileEditorArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_file_editor(&args, &self.file.workspace_dir, &self.editor_history).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        name = "terminal",
        description = "Execute shell commands. Wraps execute_bash."
    )]
    async fn terminal(
        &self,
        Parameters(args): Parameters<ExecuteBashArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Reuse execute_bash logic
        self.execute_bash(Parameters(args)).await
    }

    #[tool(name = "execute_bash", description = "Execute a bash command")]
    async fn execute_bash(
        &self,
        Parameters(args): Parameters<ExecuteBashArgs>,
    ) -> Result<CallToolResult, McpError> {
        let req = ExecuteBashRequest {
            command: args.command,
            cwd: args.cwd,
            timeout: args.timeout,
        };

        let cmd = self.bash.start_bash_command(req);

        // Simple polling loop
        let mut attempts = 0;
        loop {
            sleep(Duration::from_millis(100)).await;
            let page = self.bash.search_bash_events(Some(cmd.id));
            if let Some(last_item) = page.items.last() {
                if let BashEvent::BashOutput(out) = last_item {
                    // Combine stdout and stderr
                    let mut result_str = String::new();
                    if let Some(stdout) = &out.stdout {
                        result_str.push_str(stdout);
                    }
                    if let Some(stderr) = &out.stderr {
                        if !result_str.is_empty() {
                            result_str.push('\n');
                        }
                        result_str.push_str(stderr);
                    }
                    return Ok(CallToolResult::success(vec![Content::text(result_str)]));
                }
            }

            attempts += 1;
            if attempts > 3000 {
                return Err(McpError {
                    code: ErrorCode(0),
                    message: "Polling timed out".to_string().into(),
                    data: None,
                });
            }
        }
    }

    #[tool(name = "read_file", description = "Read a file from the workspace")]
    async fn read_file(
        &self,
        Parameters(args): Parameters<ReadFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let req = FileReadRequest {
            path: args.path.clone(),
        };
        let res = self.file.read_file(req);
        if res.success {
            Ok(CallToolResult::success(vec![Content::text(
                res.content.unwrap_or_default(),
            )]))
        } else {
            Err(McpError {
                code: ErrorCode(0),
                message: res
                    .error
                    .unwrap_or_else(|| "Unknown error reading file".to_string())
                    .into(),
                data: None,
            })
        }
    }

    #[tool(
        name = "write_file",
        description = "Write content to a file in the workspace"
    )]
    async fn write_file(
        &self,
        Parameters(args): Parameters<WriteFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let req = FileWriteRequest {
            path: args.path,
            content: args.content,
        };
        let res = self.file.write_file(req);
        if res.success {
            Ok(CallToolResult::success(vec![Content::text(
                "File written successfully",
            )]))
        } else {
            Err(McpError {
                code: ErrorCode(0),
                message: res
                    .error
                    .unwrap_or_else(|| "Unknown error writing file".to_string())
                    .into(),
                data: None,
            })
        }
    }

    #[tool(
        name = "list_files",
        description = "List files in a directory in the workspace"
    )]
    async fn list_files(
        &self,
        Parameters(args): Parameters<ListFilesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.file.workspace_dir.join(&args.path);
        match fs::read_dir(path) {
            Ok(entries) => {
                let mut files = Vec::new();
                for entry in entries.filter_map(Result::ok) {
                    if let Some(name) = entry.file_name().to_str() {
                        let type_str = if entry.path().is_dir() { "dir" } else { "file" };
                        files.push(format!("{} ({})", name, type_str));
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(
                    files.join("\n"),
                )]))
            }
            Err(e) => Err(McpError {
                code: ErrorCode(0),
                message: e.to_string().into(),
                data: None,
            }),
        }
    }

    #[tool(name = "delete_file", description = "Delete a file from the workspace")]
    async fn delete_file(
        &self,
        Parameters(args): Parameters<DeleteFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.file.workspace_dir.join(&args.path);
        match fs::remove_file(path) {
            Ok(_) => Ok(CallToolResult::success(vec![Content::text(
                "File deleted successfully",
            )])),
            Err(e) => Err(McpError {
                code: ErrorCode(0),
                message: e.to_string().into(),
                data: None,
            }),
        }
    }

    #[tool(
        name = "apply_patch",
        description = "Apply a patch to files in the workspace. Supports custom GPT-5.1 patch format."
    )]
    async fn apply_patch(
        &self,
        Parameters(args): Parameters<ApplyPatchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let paths = crate::patch::identify_files_needed(&args.patch);
        let mut orig_files = HashMap::new();

        for p in paths {
            let path_buf = self.file.workspace_dir.join(&p);
            if path_buf.exists() {
                let content = fs::read_to_string(&path_buf).map_err(|e| McpError {
                    code: ErrorCode(-32603),
                    message: format!("Failed to read {}: {}", p, e).into(),
                    data: None,
                })?;
                orig_files.insert(p, content);
            }
        }

        match crate::patch::process_patch(&args.patch, orig_files) {
            Ok((msg, _fuzz, results)) => {
                for (path, content_opt) in results {
                    let full_path = self.file.workspace_dir.join(&path);
                    if let Some(content) = content_opt {
                        if let Some(parent) = full_path.parent() {
                            fs::create_dir_all(parent).map_err(|e| McpError {
                                code: ErrorCode(-32603),
                                message: format!(
                                    "Failed to create directory {}: {}",
                                    parent.display(),
                                    e
                                )
                                .into(),
                                data: None,
                            })?;
                        }
                        fs::write(&full_path, &content).map_err(|e| McpError {
                            code: ErrorCode(-32603),
                            message: format!("Failed to write {}: {}", path, e).into(),
                            data: None,
                        })?;
                    } else {
                        if full_path.exists() {
                            fs::remove_file(&full_path).map_err(|e| McpError {
                                code: ErrorCode(-32603),
                                message: format!("Failed to delete {}: {}", path, e).into(),
                                data: None,
                            })?;
                        }
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Err(e) => Err(McpError {
                code: ErrorCode(-32603),
                message: format!("Patch failed: {}", e).into(),
                data: None,
            }),
        }
    }
}

#[tool_handler]
impl ServerHandler for OpenHandsService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("OpenHands Agent Server providing Bash and File tools".to_string()),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info().into())
    }
}

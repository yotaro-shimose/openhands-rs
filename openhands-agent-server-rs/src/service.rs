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
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[derive(Clone)]
pub struct OpenHandsService {
    bash: Arc<BashEventService>,
    file: Arc<FileService>,
    tool_router: ToolRouter<OpenHandsService>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ExecuteBashArgs {
    pub command: String,
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

#[tool_router]
impl OpenHandsService {
    pub fn new(bash: BashEventService, file: FileService) -> Self {
        Self {
            bash: Arc::new(bash),
            file: Arc::new(file),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(name = "execute_bash", description = "Execute a bash command")]
    async fn execute_bash(
        &self,
        Parameters(args): Parameters<ExecuteBashArgs>,
    ) -> Result<CallToolResult, McpError> {
        let req = ExecuteBashRequest {
            command: args.command,
            cwd: None,
            timeout: None,
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

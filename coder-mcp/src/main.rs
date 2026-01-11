use axum::Router;
use coder_mcp::logger;
use coder_mcp::runtime::bash::BashEventService;
use coder_mcp::service::CoderMcpService;
use rmcp::transport::{
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
    StreamableHttpServerConfig,
};
use std::env;
use std::path::PathBuf;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // Set up tracing using the local logger
    logger::init_logging();

    let cwd = env::current_dir().unwrap();

    // Use WORKSPACE_DIR env var if set, otherwise default to current_dir/workspace
    let workspace_path = env::var("WORKSPACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| cwd.join("workspace"));

    let bash_service = BashEventService::new(cwd.join("bash_events"), Some(workspace_path.clone()));

    // Create the MCP service
    let coder_mcp_service = CoderMcpService::new(bash_service, workspace_path);

    // Wrap it in StreamableHttpService
    let mcp_service: StreamableHttpService<CoderMcpService, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(coder_mcp_service.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

    // Build our application with a route
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .nest_service("/mcp", mcp_service);

    // Run it
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

mod service;
mod tools;

use axum::Router;
use openhands_sdk_rs::runtime::bash::BashEventService;
use openhands_sdk_rs::runtime::file::FileService;
use rmcp::transport::{
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
    StreamableHttpServerConfig,
};
use service::OpenHandsService;
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // Set up tracing using the SDK's logger
    openhands_sdk_rs::logger::init_logging();

    let cwd = env::current_dir().unwrap();
    let bash_events_dir = cwd.join("bash_events");

    let bash_service = BashEventService::new(bash_events_dir);
    let file_service = FileService::new(cwd.join("workspace"));

    // Create the MCP service
    let openhands_service = OpenHandsService::new(bash_service, file_service);

    // Wrap it in StreamableHttpService
    let mcp_service: StreamableHttpService<OpenHandsService, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(openhands_service.clone()),
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

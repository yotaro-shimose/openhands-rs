mod bash_service;
pub mod conversation_api;
mod file_service;
mod handlers;
mod models;
mod system;

use crate::bash_service::BashEventService;
use crate::file_service::FileService;
use crate::handlers::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use conversation_api::{init_conversation, submit_message};
use std::env;
use std::sync::Arc;
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
    let state = Arc::new(AppState::new(bash_service, file_service));

    // Build our application with a route
    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/alive", get(handlers::alive))
        .route("/server_info", get(handlers::server_info))
        .route(
            "/bash/start_bash_command",
            post(handlers::start_bash_command),
        )
        .route(
            "/bash/execute_bash_command",
            post(handlers::execute_bash_command),
        )
        .route(
            "/bash/bash_events/search",
            get(handlers::search_bash_events),
        )
        .route("/bash/bash_events/:id", get(handlers::get_bash_event))
        // File Routes
        .route("/file/read", post(handlers::read_file))
        .route("/file/write", post(handlers::write_file))
        // Conversation Routes
        .route("/api/conversations", post(init_conversation))
        .route("/api/conversations/:id/message", post(submit_message))
        .with_state(state);

    // Run it
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

use crate::bash_service::BashEventService;
use crate::conversation_api::ConversationManager;
use crate::models::{BashEvent, BashOutput, ExecuteBashRequest};
use crate::system;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct AppState {
    pub bash_service: Arc<BashEventService>,
    pub conversation_manager: Arc<RwLock<ConversationManager>>,
}

impl AppState {
    pub fn new(bash_service: BashEventService) -> Self {
        Self {
            bash_service: Arc::new(bash_service),
            conversation_manager: Arc::new(RwLock::new(ConversationManager::new())),
        }
    }
}
#[derive(Deserialize)]
pub struct SearchParams {
    pub command_id: Option<Uuid>,
}

pub async fn health() -> impl IntoResponse {
    "OK"
}

pub async fn alive() -> impl IntoResponse {
    Json(json!({
        "status": "ok"
    }))
}

pub async fn server_info() -> impl IntoResponse {
    let info = system::get_system_info().await;
    Json(info)
}

pub async fn start_bash_command(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteBashRequest>,
) -> impl IntoResponse {
    let command = state.bash_service.start_bash_command(req);
    Json(command)
}

pub async fn execute_bash_command(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteBashRequest>,
) -> impl IntoResponse {
    // For execute (synchronous wait), we can reuse the background service logic but we need to wait.
    // However, the current service spawns background task.
    // To match Python's execute_bash_command: "start command and wait for result".
    // We can start it, then poll for the output event.

    let command = state.bash_service.start_bash_command(req);

    // Poll for completion (output event with this command id)
    // Simple polling loop
    let mut attempts = 0;
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        // Search for output events for this command
        let page = state.bash_service.search_bash_events(Some(command.id));
        // Find the last output event
        if let Some(last_item) = page.items.last() {
            if let BashEvent::BashOutput(out) = last_item {
                // If it has exit code or we deem it done (in our simple impl, one output event = done)
                return Json(out.clone());
            }
        }

        attempts += 1;
        if attempts > 3000 {
            // ~5 minutes safety
            break;
        }
    }

    // Fallback if timeout in polling
    Json(BashOutput {
        id: Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        command_id: command.id,
        order: 0,
        exit_code: Some(-1),
        stdout: None,
        stderr: Some("Polling timed out".to_string()),
    })
}

pub async fn search_bash_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    let page = state.bash_service.search_bash_events(params.command_id);
    Json(page)
}

pub async fn get_bash_event(
    State(state): State<Arc<AppState>>,
    Path(event_id): Path<Uuid>,
) -> impl IntoResponse {
    match state.bash_service.get_bash_event(event_id) {
        Some(event) => Json(event).into_response(),
        None => (StatusCode::NOT_FOUND, "Event not found").into_response(),
    }
}

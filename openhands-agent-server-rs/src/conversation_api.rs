use crate::AppState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Json},
};
use openhands_sdk_rs::models::{
    ConversationResponse, InitConversationRequest, MessageRequest, MessageResponse,
};
pub use openhands_sdk_rs::session::{Conversation, ConversationManager};
use std::sync::Arc;

pub async fn init_conversation(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InitConversationRequest>,
) -> impl IntoResponse {
    let mut manager = state.conversation_manager.write().unwrap();

    let system_message = payload
        .system_message
        .unwrap_or_else(|| "You are a helpful assistant.".to_string());

    let conversation = manager.create_conversation(system_message);

    Json(ConversationResponse {
        id: conversation.id,
        status: "created".to_string(),
    })
}

pub async fn submit_message(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MessageRequest>,
) -> Result<Json<MessageResponse>, String> {
    let agent_deps = {
        let manager = state.conversation_manager.read().unwrap();
        manager
            .get_conversation(&id)
            .map(|c| (c.agent.clone(), c.history.clone(), c.runtime.clone()))
    };

    if let Some((agent, history_lock, runtime_lock)) = agent_deps {
        let user_event =
            openhands_sdk_rs::events::Event::Message(openhands_sdk_rs::events::MessageEvent {
                source: "user".to_string(),
                content: payload.content.clone(),
            });

        {
            let mut history = history_lock.write().await;
            history.push(user_event.clone());
        }

        let history_snapshot = {
            let history = history_lock.read().await;
            history.clone()
        };

        let response_event = {
            let mut runtime = runtime_lock.write().await;
            agent
                .step(&history_snapshot, runtime.as_mut())
                .await
                .map_err(|e| e.to_string())?
        };

        if let openhands_sdk_rs::events::Event::Message(ref m) = response_event {
            let mut history = history_lock.write().await;
            history.push(response_event.clone());
            return Ok(Json(MessageResponse {
                response: m.content.clone(),
            }));
        }

        Ok(Json(MessageResponse {
            response: "".to_string(),
        }))
    } else {
        Err("Conversation not found".to_string())
    }
}

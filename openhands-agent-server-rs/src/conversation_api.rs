use axum::{
    extract::{Path, State},
    response::Json,
};
use openhands_sdk_rs::{
    agent::Agent,
    events::Event,
    llm::{LLMConfig, LLM},
    runtime::LocalRuntime,
    tools::{CmdTool, FileReadTool, FileWriteTool, Tool},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

#[derive(Clone)]
/// Represents an active conversation session.
///
/// A conversation holds the state of the agent's interaction, including:
/// - `id`: Unique identifier for the session.
/// - `agent`: The AI agent configuration (LLM, prompt).
/// - `history`: The log of events (messages, tool calls/outputs).
/// - `runtime`: The execution environment (Local or Docker) where tools are run.
///
/// The `runtime` is stored as a dynamic trait object (`Box<dyn Runtime>`) wrapped in `Arc<RwLock>`,
/// allowing the conversation to support different runtime implementations transparently.
pub struct Conversation {
    pub id: String,
    pub agent: Arc<Agent>,
    pub history: Arc<RwLock<Vec<Event>>>,
    pub runtime: Arc<RwLock<Box<dyn openhands_sdk_rs::runtime::Runtime + Send + Sync>>>,
}

pub struct ConversationManager {
    conversations: HashMap<String, Conversation>,
}

impl ConversationManager {
    pub fn new() -> Self {
        Self {
            conversations: HashMap::new(),
        }
    }

    /// Creates a new conversation with the specified system message.
    ///
    /// This method:
    /// 1. Generates a unique Conversation ID.
    /// 2. Configures the LLM (defaulting to GPT-4o style config for now).
    /// 3. Instantiates the Agent.
    /// 4. Selects and initializes the appropriate `Runtime` based on the `RUNTIME_ENV` environment variable:
    ///    - `RUNTIME_ENV="docker"`: Starts a new Docker container using `DockerRuntime`.
    ///    - Other: Uses `LocalRuntime` to execute tools directly on the host.
    pub fn create_conversation(&mut self, system_message: String) -> Conversation {
        let id = Uuid::new_v4().to_string();

        let config = LLMConfig {
            model: "gpt-5-nano".to_string(),
            api_key: std::env::var("OPENAI_API_KEY").ok(),
            reasoning_effort: Some("minimal".to_string()),
        };
        let llm = LLM::new(config);
        let agent = Agent::new(llm, system_message);

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(CmdTool),
            Box::new(FileReadTool),
            Box::new(FileWriteTool),
        ];

        // Check environment variable to decide Runtime
        let runtime: Box<dyn openhands_sdk_rs::runtime::Runtime + Send + Sync> =
            if std::env::var("RUNTIME_ENV").unwrap_or_default() == "docker" {
                // Use DockerRuntime
                // Note: Image name could be configurable too
                Box::new(openhands_sdk_rs::runtime::DockerRuntime::new(
                    "openhands-agent-server-rs:latest",
                    tools,
                ))
            } else {
                // Default to LocalRuntime
                Box::new(LocalRuntime::new(tools))
            };

        let conversation = Conversation {
            id: id.clone(),
            agent: Arc::new(agent),
            history: Arc::new(RwLock::new(Vec::new())),
            runtime: Arc::new(RwLock::new(runtime)),
        };

        self.conversations.insert(id, conversation.clone());
        conversation
    }

    pub fn get_conversation(&self, id: &str) -> Option<&Conversation> {
        self.conversations.get(id)
    }
}

// Data Models

#[derive(Deserialize)]
pub struct InitConversationRequest {
    pub system_message: Option<String>,
}

#[derive(Serialize)]
pub struct ConversationResponse {
    pub id: String,
    pub status: String, // "running", "created" etc.
}

#[derive(Deserialize)]
pub struct MessageRequest {
    pub content: String,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub response: String,
}

// Handlers

pub async fn init_conversation(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InitConversationRequest>,
) -> Json<ConversationResponse> {
    // AppState uses std::sync::RwLock, so we use std write()
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
    // 1. Get Conversation (Sync lock on manager)
    let agent_deps = {
        let manager = state.conversation_manager.read().unwrap();
        manager
            .get_conversation(&id)
            .map(|c| (c.agent.clone(), c.history.clone(), c.runtime.clone()))
    };

    if let Some((agent, history_lock, runtime_lock)) = agent_deps {
        // 2. Add User Event (Async lock on history)
        let user_event =
            openhands_sdk_rs::events::Event::Message(openhands_sdk_rs::events::MessageEvent {
                source: "user".to_string(),
                content: payload.content.clone(),
            });

        {
            let mut history = history_lock.write().await;
            history.push(user_event.clone());
        }

        // 3. Run Agent Step
        // Snapshot history
        let history_snapshot = {
            let history = history_lock.read().await;
            history.clone()
        };

        let response_event = {
            // Async lock on runtime, held across await -> OK with Tokio RwLock
            let mut runtime = runtime_lock.write().await;
            agent
                .step(history_snapshot, runtime.as_mut())
                .await
                .map_err(|e| e.to_string())?
        };

        // 4. Update History with Response
        if let openhands_sdk_rs::events::Event::Message(ref m) = response_event {
            let mut history = history_lock.write().await;
            history.push(response_event.clone());
            return Ok(Json(MessageResponse {
                response: m.content.clone(),
            }));
        }

        // Fallback
        Ok(Json(MessageResponse {
            response: "".to_string(),
        }))
    } else {
        Err("Conversation not found".to_string())
    }
}

use crate::agent::Agent;
use crate::events::Event;
use crate::llm::{LLM, LLMConfig};
use crate::runtime::{DockerRuntime, LocalRuntime, Runtime};
use crate::tools::{CmdTool, FileReadTool, FileWriteTool, Tool};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct Conversation {
    pub id: String,
    pub agent: Arc<Agent>,
    pub history: Arc<RwLock<Vec<Event>>>,
    pub runtime: Arc<RwLock<Box<dyn Runtime + Send + Sync>>>,
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

        let runtime: Box<dyn Runtime + Send + Sync> =
            if std::env::var("RUNTIME_ENV").unwrap_or_default() == "docker" {
                Box::new(DockerRuntime::new(
                    "openhands-agent-server-rs:latest",
                    tools,
                ))
            } else {
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

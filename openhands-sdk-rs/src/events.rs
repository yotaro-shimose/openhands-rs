use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    Message(MessageEvent),
    Action(ActionEvent),
    Observation(ObservationEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEvent {
    pub source: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub arguments: serde_json::Value,
    pub thought: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationEvent {
    pub source: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub content: String,
}

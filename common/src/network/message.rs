use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Ping,
    Pong,
    Discovery,
    DiscoveryResponse,
    NodeJoin,
    NodeLeave,
    Subscribe,
    Unsubscribe,
    Data,
    Command,
    Response,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub message_type: MessageType,
    pub sender: String,
    pub receiver: Option<String>,
    pub timestamp: u64,
    pub data: String,
    pub metadata: HashMap<String, String>,
}

impl Message {
    pub fn new(message_type: MessageType, sender: &str, data: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            message_type,
            sender: sender.to_string(),
            receiver: None,
            timestamp: chrono::Utc::now().timestamp() as u64,
            data: data.to_string(),
            metadata: HashMap::new(),
        }
    }
    
    pub fn with_receiver(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }
    
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }
    
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
    
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
} 
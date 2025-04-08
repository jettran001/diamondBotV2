// External imports
use anyhow::{Result, Context};

// Standard library imports
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

// Third party imports
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde_json;

/// Loại message trong mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// Message ping
    Ping,
    /// Message pong
    Pong,
    /// Message khám phá node
    Discovery,
    /// Message phản hồi khám phá
    DiscoveryResponse,
    /// Message node tham gia
    NodeJoin,
    /// Message node rời đi
    NodeLeave,
    /// Message đăng ký
    Subscribe,
    /// Message hủy đăng ký
    Unsubscribe,
    /// Message dữ liệu
    Data,
    /// Message lệnh
    Command,
    /// Message phản hồi
    Response,
    /// Message lỗi
    Error,
}

impl Display for MessageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MessageType::Ping => write!(f, "Ping"),
            MessageType::Pong => write!(f, "Pong"),
            MessageType::Discovery => write!(f, "Discovery"),
            MessageType::DiscoveryResponse => write!(f, "DiscoveryResponse"),
            MessageType::NodeJoin => write!(f, "NodeJoin"),
            MessageType::NodeLeave => write!(f, "NodeLeave"),
            MessageType::Subscribe => write!(f, "Subscribe"),
            MessageType::Unsubscribe => write!(f, "Unsubscribe"),
            MessageType::Data => write!(f, "Data"),
            MessageType::Command => write!(f, "Command"),
            MessageType::Response => write!(f, "Response"),
            MessageType::Error => write!(f, "Error"),
        }
    }
}

/// Message trong mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ID của message
    pub id: String,
    /// Loại message
    pub message_type: MessageType,
    /// Node gửi
    pub sender: String,
    /// Node nhận (nếu có)
    pub receiver: Option<String>,
    /// Thời gian gửi
    pub timestamp: DateTime<Utc>,
    /// Dữ liệu
    pub data: String,
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl Message {
    /// Tạo mới một message
    pub fn new(message_type: MessageType, sender: &str, data: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message_type,
            sender: sender.to_string(),
            receiver: None,
            timestamp: Utc::now(),
            data: data.to_string(),
            metadata: HashMap::new(),
        }
    }
    
    /// Thêm node nhận
    pub fn with_receiver(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }
    
    /// Thêm metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }
    
    /// Chuyển đổi sang JSON
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .context("Failed to serialize message to JSON")
    }
    
    /// Chuyển đổi từ JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .context("Failed to deserialize message from JSON")
    }

    /// Kiểm tra message có phải là broadcast không
    pub fn is_broadcast(&self) -> bool {
        self.receiver.is_none()
    }

    /// Kiểm tra message có phải là ping/pong không
    pub fn is_heartbeat(&self) -> bool {
        matches!(self.message_type, MessageType::Ping | MessageType::Pong)
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test MessageType
    #[test]
    fn test_message_type() {
        assert_eq!(MessageType::Ping.to_string(), "Ping");
        assert_eq!(MessageType::Pong.to_string(), "Pong");
        assert_eq!(MessageType::Discovery.to_string(), "Discovery");
        assert_eq!(MessageType::DiscoveryResponse.to_string(), "DiscoveryResponse");
        assert_eq!(MessageType::NodeJoin.to_string(), "NodeJoin");
        assert_eq!(MessageType::NodeLeave.to_string(), "NodeLeave");
        assert_eq!(MessageType::Subscribe.to_string(), "Subscribe");
        assert_eq!(MessageType::Unsubscribe.to_string(), "Unsubscribe");
        assert_eq!(MessageType::Data.to_string(), "Data");
        assert_eq!(MessageType::Command.to_string(), "Command");
        assert_eq!(MessageType::Response.to_string(), "Response");
        assert_eq!(MessageType::Error.to_string(), "Error");
    }

    /// Test Message
    #[test]
    fn test_message() {
        let message = Message::new(
            MessageType::Data,
            "sender",
            "test data",
        );

        assert_eq!(message.sender, "sender");
        assert_eq!(message.data, "test data");
        assert!(message.receiver.is_none());
        assert!(message.is_broadcast());
        assert!(!message.is_heartbeat());

        let message = message
            .with_receiver("receiver")
            .with_metadata("key", "value");

        assert_eq!(message.receiver, Some("receiver".to_string()));
        assert_eq!(message.metadata.get("key"), Some(&"value".to_string()));
        assert!(!message.is_broadcast());

        let json = message.to_json().unwrap();
        let decoded = Message::from_json(&json).unwrap();
        assert_eq!(decoded.id, message.id);
        assert_eq!(decoded.sender, message.sender);
        assert_eq!(decoded.receiver, message.receiver);
        assert_eq!(decoded.data, message.data);
        assert_eq!(decoded.metadata, message.metadata);
    }

    /// Test heartbeat messages
    #[test]
    fn test_heartbeat() {
        let ping = Message::new(MessageType::Ping, "sender", "");
        assert!(ping.is_heartbeat());

        let pong = Message::new(MessageType::Pong, "sender", "");
        assert!(pong.is_heartbeat());

        let data = Message::new(MessageType::Data, "sender", "");
        assert!(!data.is_heartbeat());
    }
} 
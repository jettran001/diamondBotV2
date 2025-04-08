use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::net::SocketAddr;
use std::fmt::{self, Display, Formatter};
use chrono::{DateTime, Utc};
use anyhow::Result;

/// Node trong mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// ID của node
    pub id: Uuid,
    /// Địa chỉ của node
    pub address: String,
    /// Loại node
    pub node_type: NodeType,
    /// Trạng thái node
    pub status: NodeStatus,
    /// Thời gian kết nối
    pub connected_at: DateTime<Utc>,
    /// Thời gian hoạt động cuối
    pub last_activity: DateTime<Utc>,
}

/// Loại node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    /// Node master
    Master,
    /// Node slave
    Slave,
    /// Node edge
    Edge,
}

/// Trạng thái node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Node đang online
    Online,
    /// Node đang offline
    Offline,
    /// Node đang kết nối
    Connecting,
    /// Node bị lỗi
    Error,
}

/// Message trong mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ID của message
    pub id: Uuid,
    /// ID của node gửi
    pub sender: Uuid,
    /// ID của node nhận (nếu có)
    pub recipient: Option<Uuid>,
    /// Loại message
    pub message_type: MessageType,
    /// Nội dung message
    pub payload: String,
    /// Thời gian gửi
    pub timestamp: DateTime<Utc>,
}

/// Loại message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Message heartbeat
    Heartbeat,
    /// Message command
    Command,
    /// Message response
    Response,
    /// Message data
    Data,
    /// Message error
    Error,
}

/// Thông tin kết nối
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// ID của kết nối
    pub id: Uuid,
    /// Địa chỉ kết nối
    pub address: SocketAddr,
    /// Thời gian kết nối
    pub connected_at: DateTime<Utc>,
    /// Thời gian hoạt động cuối
    pub last_activity: DateTime<Utc>,
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test Node
    #[test]
    fn test_node() {
        let mut node = Node::new(
            "127.0.0.1:8080".to_string(),
            NodeType::Master,
        );
        assert_eq!(node.address, "127.0.0.1:8080");
        assert_eq!(node.node_type, NodeType::Master);
        assert_eq!(node.status, NodeStatus::Connecting);
        
        let old_activity = node.last_activity;
        node.update_activity();
        assert!(node.last_activity > old_activity);
        
        assert!(!node.is_online());
        node.status = NodeStatus::Online;
        assert!(node.is_online());
    }

    /// Test Message
    #[test]
    fn test_message() {
        let sender = Uuid::new_v4();
        let recipient = Some(Uuid::new_v4());
        let message = Message::new(
            sender,
            recipient,
            MessageType::Command,
            "test".to_string(),
        );
        assert_eq!(message.sender, sender);
        assert_eq!(message.recipient, recipient);
        assert_eq!(message.message_type, MessageType::Command);
        assert_eq!(message.payload, "test");
        assert!(!message.is_broadcast());
        
        let broadcast = Message::new(
            sender,
            None,
            MessageType::Data,
            "broadcast".to_string(),
        );
        assert!(broadcast.is_broadcast());
    }

    /// Test ConnectionInfo
    #[test]
    fn test_connection_info() {
        let address: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut conn = ConnectionInfo::new(address);
        assert_eq!(conn.address, address);
        
        let old_activity = conn.last_activity;
        conn.update_activity();
        assert!(conn.last_activity > old_activity);
    }
} 
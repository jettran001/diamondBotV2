use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: Uuid,
    pub address: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    Master,
    Slave,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    Connecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub sender: Uuid,
    pub recipient: Option<Uuid>,
    pub message_type: MessageType,
    pub payload: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    Heartbeat,
    Command,
    Response,
    Data,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub id: Uuid,
    pub address: SocketAddr,
    pub connected_at: u64,
    pub last_activity: u64,
} 
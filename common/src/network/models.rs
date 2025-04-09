
// External imports
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::net::SocketAddr;

/// Thông tin về node mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// ID của node
    pub id: Uuid,
    /// Tên của node
    pub name: String,
    /// Địa chỉ IP và port
    pub address: String,
    /// Loại node (master, slave)
    pub node_type: NodeType,
    /// Trạng thái hiện tại
    pub status: NodeStatus,
    /// Thời điểm kết nối lần cuối
    pub last_seen: DateTime<Utc>,
    /// Metadata bổ sung
    pub metadata: serde_json::Value,
}

/// Loại node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeType {
    /// Node chính
    Master,
    /// Node phụ
    Slave,
    /// Node quan sát
    Observer,
}

/// Trạng thái của node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Đang hoạt động
    Online,
    /// Ngoại tuyến
    Offline,
    /// Đang kết nối
    Connecting,
    /// Trạng thái lỗi
    Error,
}

/// Thông tin về kết nối
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// ID của kết nối
    pub id: Uuid,
    /// Node nguồn
    pub source_node: Uuid,
    /// Node đích
    pub target_node: Uuid,
    /// Loại kết nối
    pub connection_type: ConnectionType,
    /// Thời điểm thiết lập
    pub established_at: DateTime<Utc>,
    /// Ping cuối cùng (ms)
    pub last_ping_ms: u64,
}

/// Loại kết nối
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionType {
    /// WebSocket
    WebSocket,
    /// QUIC
    QUIC,
    /// gRPC
    GRPC,
    /// MQTT
    MQTT,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Unknown".to_string(),
            address: "0.0.0.0:8080".to_string(),
            node_type: NodeType::Slave,
            status: NodeStatus::Offline,
            last_seen: Utc::now(),
            metadata: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_default() {
        let node = Node::default();
        assert_eq!(node.name, "Unknown");
        assert_eq!(node.node_type, NodeType::Slave);
        assert_eq!(node.status, NodeStatus::Offline);
    }
}

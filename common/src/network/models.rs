use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::net::SocketAddr;
use std::fmt::{self, Display, Formatter};
use chrono::{DateTime, Utc};
use anyhow::Result;
use ethers::types::U256;

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

impl Node {
    /// Tạo node mới
    pub fn new(address: String, node_type: NodeType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            address,
            node_type,
            status: NodeStatus::Connecting,
            connected_at: now,
            last_activity: now,
        }
    }
    
    /// Cập nhật thời gian hoạt động
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }
    
    /// Kiểm tra node có online không
    pub fn is_online(&self) -> bool {
        self.status == NodeStatus::Online
    }
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

impl Message {
    /// Tạo message mới
    pub fn new(sender: Uuid, recipient: Option<Uuid>, message_type: MessageType, payload: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            sender,
            recipient,
            message_type,
            payload,
            timestamp: Utc::now(),
        }
    }
    
    /// Kiểm tra message có phải broadcast không
    pub fn is_broadcast(&self) -> bool {
        self.recipient.is_none()
    }
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

impl ConnectionInfo {
    /// Tạo kết nối mới
    pub fn new(address: SocketAddr) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            address,
            connected_at: now,
            last_activity: now,
        }
    }
    
    /// Cập nhật thời gian hoạt động
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }
}

/// Cấu trúc thống kê mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Số bytes đã gửi
    pub bytes_sent: u64,
    /// Số bytes đã nhận
    pub bytes_received: u64,
    /// Số packets đã gửi
    pub packets_sent: u64,
    /// Số packets đã nhận
    pub packets_received: u64,
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
        }
    }
}

/// Thông tin về trạng thái mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    /// Giá gas hiện tại
    pub gas_price: u64,
    /// Mức độ tắc nghẽn (0-100)
    pub congestion_level: u8,
    /// Thời gian block trung bình
    pub block_time: f64,
    /// Số lượng giao dịch đang chờ
    pub pending_tx_count: u32,
    /// Base fee của block mới nhất
    pub base_fee: U256,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            gas_price: 0,
            congestion_level: 0,
            block_time: 0.0,
            pending_tx_count: 0,
            base_fee: U256::zero(),
        }
    }
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

    /// Test NetworkStats
    #[test]
    fn test_network_stats() {
        let stats = NetworkStats {
            bytes_sent: 1000,
            bytes_received: 500,
            packets_sent: 10,
            packets_received: 5,
        };
        assert_eq!(stats.bytes_sent, 1000);
        assert_eq!(stats.bytes_received, 500);
        assert_eq!(stats.packets_sent, 10);
        assert_eq!(stats.packets_received, 5);
        
        let default_stats = NetworkStats::default();
        assert_eq!(default_stats.bytes_sent, 0);
        assert_eq!(default_stats.bytes_received, 0);
    }

    /// Test NetworkState
    #[test]
    fn test_network_state() {
        let state = NetworkState {
            gas_price: 50,
            congestion_level: 75,
            block_time: 12.5,
            pending_tx_count: 1000,
            base_fee: U256::from(100),
        };
        assert_eq!(state.gas_price, 50);
        assert_eq!(state.congestion_level, 75);
        assert_eq!(state.block_time, 12.5);
        assert_eq!(state.pending_tx_count, 1000);
        assert_eq!(state.base_fee, U256::from(100));
        
        let default_state = NetworkState::default();
        assert_eq!(default_state.gas_price, 0);
        assert_eq!(default_state.congestion_level, 0);
    }
} 
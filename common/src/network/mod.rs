// External imports
use ethers::types as ethers_types;
use ethers::types::U256;

// Standard library imports
use std::net::SocketAddr;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
    fmt::{Display, Formatter},
};

// Internal modules
mod config;
mod error;
mod models;
mod types;
mod utils;
mod server;
pub mod message;

// Re-exports
pub use config::NetworkConfig as NetworkConfigInternal;
pub use error::{NetworkError, NetworkResult};
pub use models::*;
pub use types::*;
pub use utils::*;
pub use server::*;

use anyhow::Result;
use async_trait::async_trait;

/// Cấu hình mạng
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub rpc_url: String,
    pub chain_id: u64,
    pub max_gas_price: U256,
    pub retry_interval: u64,
    pub timeout: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 1,
            max_gas_price: U256::from(100_000_000_000u64), // 100 gwei
            retry_interval: 5,
            timeout: 30,
        }
    }
}

/// Network manager
#[async_trait]
pub trait NetworkManager: Send + Sync + 'static {
    /// Khởi tạo manager
    async fn init(&self, config: NetworkConfig) -> Result<()>;
    
    /// Lấy endpoint khả dụng
    async fn get_available_endpoint(&self) -> Result<String>;
    
    /// Kiểm tra trạng thái mạng
    async fn check_network_status(&self) -> Result<bool>;
    
    /// Lấy thống kê mạng
    async fn get_network_stats(&self) -> Result<NetworkStats>;
    
    /// Lấy trạng thái mạng
    async fn get_network_state(&self) -> Result<NetworkState>;
}

/// RPC endpoint
#[derive(Debug, Clone)]
pub struct RPCEndpoint {
    /// URL của endpoint
    pub url: String,
    /// Trạng thái
    pub status: EndpointStatus,
    /// Số lần retry còn lại
    pub retries_left: u32,
}

/// Trạng thái endpoint
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EndpointStatus {
    /// Đang hoạt động
    Active,
    /// Đang bảo trì
    Maintenance,
    /// Không khả dụng
    Unavailable,
}

/// RPC client
#[async_trait]
pub trait RPCClient: Send + Sync + 'static {
    /// Gửi request
    async fn send_request(&self, request: &[u8]) -> Result<Vec<u8>>;
    
    /// Kiểm tra kết nối
    async fn check_connection(&self) -> Result<bool>;
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// Test NetworkConfig
    #[test]
    fn test_network_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.rpc_url, "http://localhost:8545");
        assert_eq!(config.chain_id, 1);
        assert_eq!(config.timeout, 30);
    }

    /// Test Node
    #[test]
    fn test_node() {
        let node = Node::new(
            "127.0.0.1:8080".to_string(),
            NodeType::Master,
        );
        assert_eq!(node.address, "127.0.0.1:8080");
        assert_eq!(node.node_type, NodeType::Master);
        assert_eq!(node.status, NodeStatus::Connecting);
    }

    /// Test Message
    #[test]
    fn test_message() {
        let sender = Uuid::new_v4();
        let message = Message::new(
            sender,
            None, // broadcast
            MessageType::Data,
            "test data".to_string(),
        );
        assert_eq!(message.message_type, MessageType::Data);
        assert!(message.is_broadcast());
    }
} 
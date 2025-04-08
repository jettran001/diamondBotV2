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

/// Network manager
#[async_trait]
pub trait NetworkManager: Send + Sync + 'static {
    /// Khởi tạo manager
    async fn init(&self, config: NetworkConfig) -> Result<()>;
    
    /// Lấy endpoint khả dụng
    async fn get_available_endpoint(&self) -> Result<String>;
    
    /// Kiểm tra trạng thái mạng
    async fn check_network_status(&self) -> Result<bool>;
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

    /// Test NetworkConfig
    #[test]
    fn test_network_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.connection_timeout, std::time::Duration::from_secs(30));
    }

    /// Test Node
    #[test]
    fn test_node() {
        let node = Node::new(
            "node1".to_string(),
            NodeType::Master,
            "127.0.0.1:8080".parse().unwrap(),
        );
        assert_eq!(node.id, "node1");
        assert_eq!(node.node_type, NodeType::Master);
        assert_eq!(node.address, "127.0.0.1:8080".parse::<SocketAddr>().unwrap());
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
        assert!(message.is_broadcast());
    }
} 
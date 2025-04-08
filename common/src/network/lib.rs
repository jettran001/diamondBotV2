// External imports
use anyhow::{Result, Context};

// Standard library imports
use std::{
    collections::HashMap,
    sync::Arc,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

// Third party imports
use serde::{Serialize, Deserialize};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{RwLock, Mutex},
    time::sleep,
};

// Internal imports
use super::error::{NetworkError, NetworkResult};
use super::models::{Node, NodeType, NodeStatus, Message, MessageType, ConnectionInfo};
use super::config::NetworkConfig;

/// Module error
pub mod error;

/// Module models
pub mod models;

/// Module utils
pub mod utils;

/// Module config
pub mod config;

/// Module message
pub mod message;

/// Module types
pub mod types;

/// Module server
pub mod server;

/// Module blockchain
#[cfg(feature = "blockchain")]
pub mod blockchain;

// Re-export các kiểu dữ liệu quan trọng
pub use error::{NetworkError, NetworkResult};
pub use models::{Node, NodeType, NodeStatus, Message, MessageType, ConnectionInfo};
pub use config::NetworkConfig;
pub use server::*;

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test NetworkConfig
    #[test]
    fn test_network_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.connection_timeout, Duration::from_secs(30));
    }

    /// Test Node
    #[test]
    fn test_node() {
        let node = Node::new(
            "node1".to_string(),
            NodeType::Validator,
            "127.0.0.1:8080".parse().unwrap(),
        );
        assert_eq!(node.id, "node1");
        assert_eq!(node.node_type, NodeType::Validator);
        assert_eq!(node.address, "127.0.0.1:8080".parse().unwrap());
    }

    /// Test Message
    #[test]
    fn test_message() {
        let message = Message::new(
            "sender".to_string(),
            "receiver".to_string(),
            MessageType::Block,
            vec![1, 2, 3],
        );
        assert_eq!(message.sender, "sender");
        assert_eq!(message.receiver, "receiver");
        assert_eq!(message.message_type, MessageType::Block);
        assert_eq!(message.data, vec![1, 2, 3]);
    }

    /// Test Server
    #[test]
    fn test_server() {
        let server = WebSocketServer::new("secret".to_string());
        assert_eq!(server.jwt_secret, "secret");
    }
} 
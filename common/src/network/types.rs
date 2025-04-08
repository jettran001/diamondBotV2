use serde::{Serialize, Deserialize};
use std::fmt::{self, Display, Formatter};
use std::net::SocketAddr;
use std::error::Error;
use anyhow::{Result, Context};
use chrono::{DateTime, Utc};
use std::io::{self, ErrorKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum NodeType {
    Validator,
    #[default]
    FullNode,
    LightNode,
    Relayer,
    Api,
    Custom(String),
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeType::Validator => write!(f, "validator"),
            NodeType::FullNode => write!(f, "full_node"),
            NodeType::LightNode => write!(f, "light_node"),
            NodeType::Relayer => write!(f, "relayer"),
            NodeType::Api => write!(f, "api"),
            NodeType::Custom(name) => write!(f, "custom_{}", name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub node_id: String,
    pub node_type: NodeType,
    pub address: String,
    pub port: u16,
    pub version: String,
    pub supported_protocols: Vec<String>,
    pub last_seen: DateTime<Utc>,
    pub is_active: bool,
}

impl ConnectionInfo {
    pub fn new(
        node_id: String,
        node_type: NodeType,
        address: String,
        port: u16,
        version: String,
    ) -> Self {
        Self {
            node_id,
            node_type,
            address,
            port,
            version,
            supported_protocols: Vec::new(),
            last_seen: Utc::now(),
            is_active: true,
        }
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, io::Error> {
        format!("{}:{}", self.address, self.port).parse()
            .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e))
    }

    pub fn update_last_seen(&mut self) {
        self.last_seen = Utc::now();
    }

    pub fn add_protocol(&mut self, protocol: String) {
        if !self.supported_protocols.contains(&protocol) {
            self.supported_protocols.push(protocol);
        }
    }
}

#[derive(Debug)]
pub enum NetworkError {
    ConnectionFailed(String),
    MessageSendFailed(String),
    MessageReceiveFailed(String),
    DiscoveryFailed(String),
    ProtocolError(String),
    InvalidAddress(String),
    ConfigError(String),
    AuthenticationFailed(String),
    Timeout(String),
    Unknown(String),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            NetworkError::MessageSendFailed(msg) => write!(f, "Failed to send message: {}", msg),
            NetworkError::MessageReceiveFailed(msg) => write!(f, "Failed to receive message: {}", msg),
            NetworkError::DiscoveryFailed(msg) => write!(f, "Node discovery failed: {}", msg),
            NetworkError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            NetworkError::InvalidAddress(msg) => write!(f, "Invalid address: {}", msg),
            NetworkError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            NetworkError::AuthenticationFailed(msg) => write!(f, "Authentication failed: {}", msg),
            NetworkError::Timeout(msg) => write!(f, "Operation timed out: {}", msg),
            NetworkError::Unknown(msg) => write!(f, "Unknown error: {}", msg),
        }
    }
}

impl Error for NetworkError {}

pub type NetworkResult<T> = Result<T, NetworkError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type() {
        assert_eq!(NodeType::Validator.to_string(), "validator");
        assert_eq!(NodeType::FullNode.to_string(), "full_node");
        assert_eq!(NodeType::LightNode.to_string(), "light_node");
        assert_eq!(NodeType::Relayer.to_string(), "relayer");
        assert_eq!(NodeType::Api.to_string(), "api");
        assert_eq!(NodeType::Custom("test".to_string()).to_string(), "custom_test");
    }

    #[test]
    fn test_connection_info() {
        let mut conn = ConnectionInfo::new(
            "node1".to_string(),
            NodeType::Validator,
            "127.0.0.1".to_string(),
            8080,
            "1.0.0".to_string(),
        );

        assert_eq!(conn.node_id, "node1");
        assert_eq!(conn.node_type, NodeType::Validator);
        assert_eq!(conn.address, "127.0.0.1");
        assert_eq!(conn.port, 8080);
        assert_eq!(conn.version, "1.0.0");
        assert!(conn.supported_protocols.is_empty());
        assert!(conn.is_active);

        let old_last_seen = conn.last_seen;
        conn.update_last_seen();
        assert!(conn.last_seen > old_last_seen);

        conn.add_protocol("tcp".to_string());
        assert_eq!(conn.supported_protocols, vec!["tcp"]);
        conn.add_protocol("tcp".to_string());
        assert_eq!(conn.supported_protocols, vec!["tcp"]);

        let socket = conn.socket_addr().unwrap();
        assert_eq!(socket.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn test_network_error() {
        let error = NetworkError::ConnectionFailed("test".to_string());
        assert_eq!(error.to_string(), "Connection failed: test");

        let error = NetworkError::MessageSendFailed("test".to_string());
        assert_eq!(error.to_string(), "Failed to send message: test");

        let error = NetworkError::MessageReceiveFailed("test".to_string());
        assert_eq!(error.to_string(), "Failed to receive message: test");

        let error = NetworkError::DiscoveryFailed("test".to_string());
        assert_eq!(error.to_string(), "Node discovery failed: test");

        let error = NetworkError::ProtocolError("test".to_string());
        assert_eq!(error.to_string(), "Protocol error: test");

        let error = NetworkError::InvalidAddress("test".to_string());
        assert_eq!(error.to_string(), "Invalid address: test");

        let error = NetworkError::ConfigError("test".to_string());
        assert_eq!(error.to_string(), "Configuration error: test");

        let error = NetworkError::AuthenticationFailed("test".to_string());
        assert_eq!(error.to_string(), "Authentication failed: test");

        let error = NetworkError::Timeout("test".to_string());
        assert_eq!(error.to_string(), "Operation timed out: test");

        let error = NetworkError::Unknown("test".to_string());
        assert_eq!(error.to_string(), "Unknown error: test");
    }
} 
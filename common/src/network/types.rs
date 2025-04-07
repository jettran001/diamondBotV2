use serde::{Serialize, Deserialize};
use std::fmt;
use std::net::SocketAddr;
use std::error::Error;

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
    pub last_seen: u64,
    pub is_active: bool,
}

impl ConnectionInfo {
    pub fn socket_addr(&self) -> Result<SocketAddr, std::io::Error> {
        format!("{}:{}", self.address, self.port).parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
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
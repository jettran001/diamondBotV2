pub mod error;
pub mod models;
pub mod utils;
pub mod config;
pub mod message;
pub mod types;
pub mod server;

#[cfg(feature = "blockchain")]
pub mod blockchain;

// Re-export các kiểu dữ liệu quan trọng
pub use error::{NetworkError, NetworkResult};
pub use models::{Node, NodeType, NodeStatus, Message, MessageType, ConnectionInfo};
pub use config::NetworkConfig;
pub use server::*; 
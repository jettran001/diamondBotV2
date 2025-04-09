
// External imports
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use thiserror::Error;

// Module exports
pub mod models;
pub mod error;
pub mod types;
pub mod utils;
pub mod config;
pub mod message;

// Re-export for convenience
pub use self::models::*;
pub use self::error::*;
pub use self::types::*;
pub use self::utils::*;
pub use self::config::*;
pub use self::message::*;

/// Cấu hình mạng chung
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Cổng mạng
    pub port: u16,
    /// Địa chỉ host
    pub host: String,
    /// Timeout kết nối (giây)
    pub timeout_seconds: u64,
    /// Số lượng retry tối đa
    pub max_retries: u32,
    /// SSL được bật không
    pub use_ssl: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
            use_ssl: false,
        }
    }
}

/// Kết quả từ network operations
pub type NetworkResult<T> = Result<T, NetworkError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.use_ssl, false);
    }
}

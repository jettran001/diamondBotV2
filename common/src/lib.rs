// External imports
use ethers::core::types::{Address, H256, U256};
use anyhow::Result;
use serde::{Serialize, Deserialize};

// Module declarations
pub mod types;
pub mod error;
pub mod utils;
pub mod cache;
pub mod models;
pub mod middleware;
pub mod server;
pub mod worker;
pub mod retry_policy;
pub mod config;
pub mod subscription;
pub mod chain_adapter;
pub mod state_manager;
pub mod task_manager;
pub mod validator;
pub mod executor;
pub mod scheduler;
pub mod security;
pub mod user;
pub mod ai;
pub mod event_handler;
pub mod diamond_manager;
pub mod logger;
pub mod metrics;
pub mod monte_equilibrium;
pub mod equilibrium;

// Re-exports for common crate
pub use types::*;
pub use error::*;
pub use utils::*;
pub use cache::*;
pub use models::*;
pub use server::*;

// Network types defined locally instead of re-exported from network crate
// to avoid cyclic dependency
pub mod network_types {
    pub type NetworkResult<T> = Result<T, String>;
    
    #[derive(Debug, Clone)]
    pub struct NetworkConfig {
        pub endpoint: String,
        pub timeout: std::time::Duration,
    }
    
    #[derive(Debug, Clone)]
    pub enum NetworkError {
        ConnectionFailed,
        Timeout,
        InvalidResponse,
        Other(String),
    }
    
    #[derive(Debug, Clone)]
    pub enum EndpointStatus {
        Online,
        Offline,
        Degraded,
    }
    
    pub trait NetworkManager {
        fn connect(&self) -> NetworkResult<()>;
        fn disconnect(&self) -> NetworkResult<()>;
        fn is_connected(&self) -> bool;
    }
    
    #[derive(Debug, Clone, Default)]
    pub struct NetworkState {
        pub connected: bool,
        pub last_ping: Option<std::time::Duration>,
    }
    
    #[derive(Debug, Clone, Default)]
    pub struct NetworkStats {
        pub requests_sent: u64,
        pub responses_received: u64,
        pub errors: u64,
    }
    
    pub trait RPCClient {
        fn call(&self, method: &str, params: Vec<serde_json::Value>) -> NetworkResult<serde_json::Value>;
    }
    
    pub trait RPCEndpoint {
        fn get_status(&self) -> EndpointStatus;
        fn get_url(&self) -> String;
    }
}

// Re-export for backward compatibility
pub use network_types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
        assert!(true);
    }
}

/// Server Gateway cho toàn bộ hệ thống
/// Cung cấp điểm kết nối chung cho tất cả các domain
/// * blockchain - Quản lý blockchain và hợp đồng thông minh
/// * network - Kết nối với các dịch vụ mạng
/// * snipebot - Logic bot giao dịch 
/// * ai - Các module AI và phân tích dữ liệu
/// * wallet - Quản lý ví và giao dịch
#[derive(Debug)]
pub struct ServerGateway {
    /// Server đang chạy hay không
    is_running: bool,
    /// Cấu hình server
    config: config::Config,
}

impl ServerGateway {
    /// Tạo một server gateway mới
    pub fn new(config: config::Config) -> Self {
        Self {
            is_running: false,
            config,
        }
    }
    
    /// Khởi động gateway
    pub async fn start(&mut self) -> Result<()> {
        self.is_running = true;
        Ok(())
    }
    
    /// Dừng gateway
    pub async fn stop(&mut self) -> Result<()> {
        self.is_running = false;
        Ok(())
    }
} 
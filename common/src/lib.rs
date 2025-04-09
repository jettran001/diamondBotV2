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

// Public re-exports from network crate
pub use network::core::{
    NetworkConfig, 
    NetworkError, 
    NetworkResult,
    RPCClient,
    RPCEndpoint,
    EndpointStatus,
    NetworkManager,
    NetworkState,
    NetworkStats,
};

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
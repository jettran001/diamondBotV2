use std::sync::Arc;
use tokio::net::TcpListener;
use anyhow::Result;
use tracing::{info, error};

use crate::network::{
    NetworkConfig,
    NetworkError,
    NetworkResult,
};

/// Server trait định nghĩa các phương thức cần thiết cho một server
pub trait Server: Send + Sync {
    /// Khởi động server
    async fn start(&mut self) -> Result<()>;
    
    /// Dừng server
    async fn stop(&mut self) -> Result<()>;
    
    /// Kiểm tra trạng thái server
    fn is_running(&self) -> bool;
}

/// Cấu trúc NetworkServer
pub struct NetworkServer {
    config: Arc<NetworkConfig>,
    listener: Option<TcpListener>,
    is_running: bool,
}

impl NetworkServer {
    /// Tạo một NetworkServer mới
    pub fn new(config: Arc<NetworkConfig>) -> Self {
        Self {
            config,
            listener: None,
            is_running: false,
        }
    }
}

impl Server for NetworkServer {
    async fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Err(NetworkError::ServerAlreadyRunning.into());
        }
        
        let addr = format!("{}:{}", self.config.rpc_url, self.config.chain_id);
        let listener = TcpListener::bind(&addr).await?;
        
        info!("Server started on {}", addr);
        
        self.listener = Some(listener);
        self.is_running = true;
        
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Err(NetworkError::ServerNotRunning.into());
        }
        
        if let Some(listener) = self.listener.take() {
            drop(listener);
        }
        
        self.is_running = false;
        info!("Server stopped");
        
        Ok(())
    }
    
    fn is_running(&self) -> bool {
        self.is_running
    }
} 
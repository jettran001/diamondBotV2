// Standard library imports
use std::sync::Arc;
use std::collections::HashMap;

// External imports
use tokio::net::TcpListener;
use anyhow::{Result, Context};
use tracing::{info, error, debug};
use async_trait::async_trait;

// Internal imports
use network::core::{
    NetworkConfig,
    NetworkError,
    NetworkResult,
};

/// Server trait định nghĩa các phương thức cần thiết cho một server
#[async_trait]
pub trait Server: Send + Sync {
    /// Khởi động server
    async fn start(&mut self) -> Result<()>;
    
    /// Dừng server
    async fn stop(&mut self) -> Result<()>;
    
    /// Kiểm tra trạng thái server
    fn is_running(&self) -> bool;
}

/// Gateway server cho các domain services
/// 
/// Cung cấp một điểm kết nối chung cho các dịch vụ:
/// - Blockchain
/// - Network
/// - Snipebot
/// - AI
/// - Wallet
#[derive(Debug)]
pub struct GatewayServer {
    /// Cấu hình server
    config: Arc<crate::config::Config>,
    /// TCP listener
    listener: Option<TcpListener>,
    /// Trạng thái server
    is_running: bool,
    /// Quản lý services
    services: HashMap<String, Box<dyn Server>>,
}

impl GatewayServer {
    /// Tạo một gateway server mới
    pub fn new(config: Arc<crate::config::Config>) -> Self {
        Self {
            config,
            listener: None,
            is_running: false,
            services: HashMap::new(),
        }
    }
    
    /// Đăng ký service
    pub fn register_service(&mut self, name: &str, service: Box<dyn Server>) -> Result<()> {
        if self.services.contains_key(name) {
            return Err(anyhow::anyhow!("Service {} đã tồn tại", name));
        }
        
        self.services.insert(name.to_string(), service);
        debug!("Đã đăng ký service: {}", name);
        
        Ok(())
    }
    
    /// Lấy service theo tên
    pub fn get_service(&self, name: &str) -> Option<&Box<dyn Server>> {
        self.services.get(name)
    }
}

#[async_trait]
impl Server for GatewayServer {
    async fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Err(anyhow::anyhow!("Server đã được khởi động"));
        }
        
        // Khởi động các service
        for (name, service) in self.services.iter_mut() {
            service.start().await
                .with_context(|| format!("Không thể khởi động service {}", name))?;
            
            info!("Đã khởi động service: {}", name);
        }
        
        // Nếu có gateway port được đặt trong config, lắng nghe ở port đó
        if let Some(port) = self.config.gateway_port {
            let addr = format!("127.0.0.1:{}", port);
            let listener = TcpListener::bind(&addr).await
                .with_context(|| format!("Không thể lắng nghe ở địa chỉ {}", addr))?;
            
            info!("Gateway đang lắng nghe ở {}", addr);
            self.listener = Some(listener);
        }
        
        self.is_running = true;
        info!("Gateway server đã được khởi động");
        
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Err(anyhow::anyhow!("Server chưa được khởi động"));
        }
        
        // Dừng các service
        for (name, service) in self.services.iter_mut() {
            service.stop().await
                .with_context(|| format!("Không thể dừng service {}", name))?;
            
            info!("Đã dừng service: {}", name);
        }
        
        // Dọn dẹp listener
        if let Some(listener) = self.listener.take() {
            drop(listener);
        }
        
        self.is_running = false;
        info!("Gateway server đã được dừng");
        
        Ok(())
    }
    
    fn is_running(&self) -> bool {
        self.is_running
    }
} 
} 
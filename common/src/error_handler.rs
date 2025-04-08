// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

// Third party imports
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Error handler trait
#[async_trait]
pub trait ErrorHandler: Send + Sync + 'static {
    /// Thêm lỗi
    async fn add_error(&self, error: Error) -> Result<()>;

    /// Lấy lỗi
    async fn get_error(&self, error_id: &str) -> Result<Option<Error>>;

    /// Lấy tất cả lỗi
    async fn get_errors(&self) -> Result<Vec<Error>>;

    /// Xóa lỗi
    async fn remove_error(&self, error_id: &str) -> Result<()>;

    /// Xóa tất cả lỗi
    async fn clear_errors(&self) -> Result<()>;
}

/// Lỗi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    /// ID lỗi
    pub error_id: String,
    /// Tên lỗi
    pub name: String,
    /// Mô tả lỗi
    pub description: String,
    /// Trạng thái lỗi
    pub status: ErrorStatus,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Trạng thái lỗi
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorStatus {
    /// Chờ
    Pending,
    /// Đang xử lý
    Processing,
    /// Đã xử lý
    Resolved,
    /// Lỗi
    Error,
}

/// Basic error handler
#[derive(Debug, Clone)]
pub struct BasicErrorHandler {
    config: Arc<RwLock<ErrorHandlerConfig>>,
    errors: Arc<RwLock<HashMap<String, Error>>>,
}

/// Cấu hình error handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandlerConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian lưu trữ
    pub retention_period: Duration,
}

impl BasicErrorHandler {
    /// Tạo error handler mới
    pub fn new(config: ErrorHandlerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            errors: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ErrorHandler for BasicErrorHandler {
    async fn add_error(&self, error: Error) -> Result<()> {
        let mut errors = self.errors.write().unwrap();
        errors.insert(error.error_id.clone(), error);
        Ok(())
    }

    async fn get_error(&self, error_id: &str) -> Result<Option<Error>> {
        let errors = self.errors.read().unwrap();
        Ok(errors.get(error_id).cloned())
    }

    async fn get_errors(&self) -> Result<Vec<Error>> {
        let errors = self.errors.read().unwrap();
        Ok(errors.values().cloned().collect())
    }

    async fn remove_error(&self, error_id: &str) -> Result<()> {
        let mut errors = self.errors.write().unwrap();
        errors.remove(error_id);
        Ok(())
    }

    async fn clear_errors(&self) -> Result<()> {
        let mut errors = self.errors.write().unwrap();
        errors.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test Error
    #[test]
    fn test_error() {
        let now = SystemTime::now();
        let error = Error {
            error_id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            status: ErrorStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(error.error_id, "test");
        assert_eq!(error.name, "Test");
    }

    /// Test ErrorStatus
    #[test]
    fn test_error_status() {
        assert_eq!(ErrorStatus::Pending as u8, 0);
        assert_eq!(ErrorStatus::Processing as u8, 1);
        assert_eq!(ErrorStatus::Resolved as u8, 2);
        assert_eq!(ErrorStatus::Error as u8, 3);
    }

    /// Test ErrorHandlerConfig
    #[test]
    fn test_error_handler_config() {
        let config = ErrorHandlerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicErrorHandler
    #[test]
    fn test_basic_error_handler() {
        let config = ErrorHandlerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let handler = BasicErrorHandler::new(config);
        assert!(handler.config.read().unwrap().config_id == "test");
    }
} 
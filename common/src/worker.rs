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

/// Worker trait
#[async_trait]
pub trait Worker: Send + Sync + 'static {
    /// Khởi tạo worker
    async fn init(&self) -> Result<()>;

    /// Dừng worker
    async fn stop(&self) -> Result<()>;

    /// Lấy trạng thái worker
    async fn get_status(&self) -> Result<WorkerStatus>;

    /// Lấy thông tin worker
    async fn get_info(&self) -> Result<WorkerInfo>;
}

/// Thông tin worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// ID worker
    pub worker_id: String,
    /// Tên worker
    pub name: String,
    /// Mô tả worker
    pub description: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Trạng thái worker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkerStatus {
    /// Đã khởi tạo
    Initialized,
    /// Đang chạy
    Running,
    /// Đã dừng
    Stopped,
    /// Lỗi
    Error,
}

/// Basic worker
#[derive(Debug, Clone)]
pub struct BasicWorker {
    config: Arc<RwLock<WorkerConfig>>,
    status: Arc<RwLock<WorkerStatus>>,
    info: Arc<RwLock<WorkerInfo>>,
}

/// Cấu hình worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
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

impl BasicWorker {
    /// Tạo worker mới
    pub fn new(config: WorkerConfig) -> Self {
        let now = SystemTime::now();
        let info = WorkerInfo {
            worker_id: config.config_id.clone(),
            name: config.name.clone(),
            description: "Basic worker".to_string(),
            version: config.version.clone(),
            created_at: now,
            updated_at: now,
        };
        Self {
            config: Arc::new(RwLock::new(config)),
            status: Arc::new(RwLock::new(WorkerStatus::Initialized)),
            info: Arc::new(RwLock::new(info)),
        }
    }
}

#[async_trait]
impl Worker for BasicWorker {
    async fn init(&self) -> Result<()> {
        let mut status = self.status.write().unwrap();
        *status = WorkerStatus::Running;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let mut status = self.status.write().unwrap();
        *status = WorkerStatus::Stopped;
        Ok(())
    }

    async fn get_status(&self) -> Result<WorkerStatus> {
        let status = self.status.read().unwrap();
        Ok(*status)
    }

    async fn get_info(&self) -> Result<WorkerInfo> {
        let info = self.info.read().unwrap();
        Ok(info.clone())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test WorkerInfo
    #[test]
    fn test_worker_info() {
        let now = SystemTime::now();
        let info = WorkerInfo {
            worker_id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: now,
            updated_at: now,
        };
        assert_eq!(info.worker_id, "test");
        assert_eq!(info.name, "Test");
    }

    /// Test WorkerStatus
    #[test]
    fn test_worker_status() {
        assert_eq!(WorkerStatus::Initialized as u8, 0);
        assert_eq!(WorkerStatus::Running as u8, 1);
        assert_eq!(WorkerStatus::Stopped as u8, 2);
        assert_eq!(WorkerStatus::Error as u8, 3);
    }

    /// Test WorkerConfig
    #[test]
    fn test_worker_config() {
        let config = WorkerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicWorker
    #[test]
    fn test_basic_worker() {
        let config = WorkerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let worker = BasicWorker::new(config);
        assert!(worker.config.read().unwrap().config_id == "test");
    }
} 
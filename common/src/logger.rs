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

/// Logger trait
#[async_trait]
pub trait Logger: Send + Sync + 'static {
    /// Ghi log
    async fn log(&self, level: LogLevel, message: &str) -> Result<()>;

    /// Lấy log
    async fn get_logs(&self, level: Option<LogLevel>) -> Result<Vec<LogEntry>>;

    /// Xóa log
    async fn clear_logs(&self) -> Result<()>;
}

/// Mức độ log
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogLevel {
    /// Debug
    Debug,
    /// Info
    Info,
    /// Warning
    Warning,
    /// Error
    Error,
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Mức độ log
    pub level: LogLevel,
    /// Nội dung log
    pub message: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Basic logger
#[derive(Debug, Clone)]
pub struct BasicLogger {
    config: Arc<RwLock<LoggerConfig>>,
    logs: Arc<RwLock<Vec<LogEntry>>>,
}

/// Cấu hình logger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Mức độ log mặc định
    pub default_level: LogLevel,
}

impl BasicLogger {
    /// Tạo logger mới
    pub fn new(config: LoggerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            logs: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Logger for BasicLogger {
    async fn log(&self, level: LogLevel, message: &str) -> Result<()> {
        let entry = LogEntry {
            level,
            message: message.to_string(),
            created_at: SystemTime::now(),
        };
        let mut logs = self.logs.write().unwrap();
        logs.push(entry);
        Ok(())
    }

    async fn get_logs(&self, level: Option<LogLevel>) -> Result<Vec<LogEntry>> {
        let logs = self.logs.read().unwrap();
        if let Some(level) = level {
            Ok(logs
                .iter()
                .filter(|entry| entry.level == level)
                .cloned()
                .collect())
        } else {
            Ok(logs.clone())
        }
    }

    async fn clear_logs(&self) -> Result<()> {
        let mut logs = self.logs.write().unwrap();
        logs.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test LogLevel
    #[test]
    fn test_log_level() {
        assert_eq!(LogLevel::Debug as u8, 0);
        assert_eq!(LogLevel::Info as u8, 1);
        assert_eq!(LogLevel::Warning as u8, 2);
        assert_eq!(LogLevel::Error as u8, 3);
    }

    /// Test LogEntry
    #[test]
    fn test_log_entry() {
        let entry = LogEntry {
            level: LogLevel::Info,
            message: "test".to_string(),
            created_at: SystemTime::now(),
        };
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.message, "test");
    }

    /// Test LoggerConfig
    #[test]
    fn test_logger_config() {
        let config = LoggerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            default_level: LogLevel::Info,
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicLogger
    #[test]
    fn test_basic_logger() {
        let config = LoggerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            default_level: LogLevel::Info,
        };
        let logger = BasicLogger::new(config);
        assert!(logger.config.read().unwrap().config_id == "test");
    }
} 
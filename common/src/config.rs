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

/// Config trait
#[async_trait]
pub trait Config: Send + Sync + 'static {
    /// Lấy giá trị config
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>>;

    /// Lưu giá trị config
    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()>;

    /// Xóa giá trị config
    async fn remove(&self, key: &str) -> Result<()>;

    /// Xóa tất cả giá trị config
    async fn clear(&self) -> Result<()>;
}

/// Config entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry<T> {
    /// Giá trị
    pub value: T,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Basic config
#[derive(Debug, Clone)]
pub struct BasicConfig {
    config: Arc<RwLock<ConfigConfig>>,
    entries: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

/// Cấu hình config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

impl BasicConfig {
    /// Tạo config mới
    pub fn new(config: ConfigConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Config for BasicConfig {
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let entries = self.entries.read().unwrap();
        if let Some(data) = entries.get(key) {
            let entry: ConfigEntry<T> = bincode::deserialize(data)?;
            return Ok(Some(entry.value));
        }
        Ok(None)
    }

    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()> {
        let now = SystemTime::now();
        let entry = ConfigEntry {
            value,
            created_at: now,
            updated_at: now,
        };
        let data = bincode::serialize(&entry)?;
        let mut entries = self.entries.write().unwrap();
        entries.insert(key.to_string(), data);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test ConfigEntry
    #[test]
    fn test_config_entry() {
        let now = SystemTime::now();
        let entry = ConfigEntry {
            value: "test".to_string(),
            created_at: now,
            updated_at: now,
        };
        assert_eq!(entry.value, "test");
    }

    /// Test ConfigConfig
    #[test]
    fn test_config_config() {
        let config = ConfigConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicConfig
    #[test]
    fn test_basic_config() {
        let config = ConfigConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
        };
        let basic_config = BasicConfig::new(config);
        assert!(basic_config.config.read().unwrap().config_id == "test");
    }
} 
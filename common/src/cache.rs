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

/// Cache trait
#[async_trait]
pub trait Cache: Send + Sync + 'static {
    /// Lấy giá trị từ cache
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>>;

    /// Lưu giá trị vào cache
    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()>;

    /// Xóa giá trị khỏi cache
    async fn remove(&self, key: &str) -> Result<()>;

    /// Xóa tất cả giá trị khỏi cache
    async fn clear(&self) -> Result<()>;
}

/// Cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    /// Giá trị
    pub value: T,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian hết hạn
    pub expires_at: Option<SystemTime>,
}

/// Basic cache
#[derive(Debug, Clone)]
pub struct BasicCache {
    config: Arc<RwLock<CacheConfig>>,
    entries: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

/// Cấu hình cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian sống mặc định
    pub default_ttl: Duration,
}

impl BasicCache {
    /// Tạo cache mới
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Cache for BasicCache {
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let entries = self.entries.read().unwrap();
        if let Some(data) = entries.get(key) {
            let entry: CacheEntry<T> = bincode::deserialize(data)?;
            if let Some(expires_at) = entry.expires_at {
                if SystemTime::now() > expires_at {
                    return Ok(None);
                }
            }
            return Ok(Some(entry.value));
        }
        Ok(None)
    }

    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()> {
        let config = self.config.read().unwrap();
        let entry = CacheEntry {
            value,
            created_at: SystemTime::now(),
            expires_at: Some(SystemTime::now() + config.default_ttl),
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

    /// Test CacheEntry
    #[test]
    fn test_cache_entry() {
        let entry = CacheEntry {
            value: "test".to_string(),
            created_at: SystemTime::now(),
            expires_at: None,
        };
        assert_eq!(entry.value, "test");
    }

    /// Test CacheConfig
    #[test]
    fn test_cache_config() {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            default_ttl: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicCache
    #[test]
    fn test_basic_cache() {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            default_ttl: Duration::from_secs(3600),
        };
        let cache = BasicCache::new(config);
        assert!(cache.config.read().unwrap().config_id == "test");
    }
} 
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

/// State manager trait
#[async_trait]
pub trait StateManager: Send + Sync + 'static {
    /// Lấy trạng thái
    async fn get_state<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>>;

    /// Lưu trạng thái
    async fn set_state<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()>;

    /// Xóa trạng thái
    async fn remove_state(&self, key: &str) -> Result<()>;

    /// Xóa tất cả trạng thái
    async fn clear_state(&self) -> Result<()>;
}

/// Trạng thái
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State<T> {
    /// Giá trị
    pub value: T,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Basic state manager
#[derive(Debug, Clone)]
pub struct BasicStateManager {
    config: Arc<RwLock<StateManagerConfig>>,
    states: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

/// Cấu hình state manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManagerConfig {
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

impl BasicStateManager {
    /// Tạo state manager mới
    pub fn new(config: StateManagerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl StateManager for BasicStateManager {
    async fn get_state<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let states = self.states.read().unwrap();
        if let Some(data) = states.get(key) {
            let state: State<T> = bincode::deserialize(data)?;
            return Ok(Some(state.value));
        }
        Ok(None)
    }

    async fn set_state<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()> {
        let now = SystemTime::now();
        let state = State {
            value,
            created_at: now,
            updated_at: now,
        };
        let data = bincode::serialize(&state)?;
        let mut states = self.states.write().unwrap();
        states.insert(key.to_string(), data);
        Ok(())
    }

    async fn remove_state(&self, key: &str) -> Result<()> {
        let mut states = self.states.write().unwrap();
        states.remove(key);
        Ok(())
    }

    async fn clear_state(&self) -> Result<()> {
        let mut states = self.states.write().unwrap();
        states.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test State
    #[test]
    fn test_state() {
        let now = SystemTime::now();
        let state = State {
            value: "test".to_string(),
            created_at: now,
            updated_at: now,
        };
        assert_eq!(state.value, "test");
    }

    /// Test StateManagerConfig
    #[test]
    fn test_state_manager_config() {
        let config = StateManagerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicStateManager
    #[test]
    fn test_basic_state_manager() {
        let config = StateManagerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let manager = BasicStateManager::new(config);
        assert!(manager.config.read().unwrap().config_id == "test");
    }
} 
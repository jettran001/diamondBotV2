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

/// Event handler trait
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// Xử lý sự kiện
    async fn handle_event(&self, event: &Event) -> Result<()>;

    /// Lấy lịch sử sự kiện
    async fn get_event_history(&self) -> Result<Vec<Event>>;

    /// Xóa lịch sử sự kiện
    async fn clear_event_history(&self) -> Result<()>;
}

/// Sự kiện
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// ID sự kiện
    pub event_id: String,
    /// Loại sự kiện
    pub event_type: String,
    /// Dữ liệu sự kiện
    pub data: Vec<u8>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Basic event handler
#[derive(Debug, Clone)]
pub struct BasicEventHandler {
    config: Arc<RwLock<EventHandlerConfig>>,
    events: Arc<RwLock<Vec<Event>>>,
}

/// Cấu hình event handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventHandlerConfig {
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

impl BasicEventHandler {
    /// Tạo event handler mới
    pub fn new(config: EventHandlerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl EventHandler for BasicEventHandler {
    async fn handle_event(&self, event: &Event) -> Result<()> {
        let mut events = self.events.write().unwrap();
        events.push(event.clone());
        Ok(())
    }

    async fn get_event_history(&self) -> Result<Vec<Event>> {
        let events = self.events.read().unwrap();
        Ok(events.clone())
    }

    async fn clear_event_history(&self) -> Result<()> {
        let mut events = self.events.write().unwrap();
        events.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test Event
    #[test]
    fn test_event() {
        let event = Event {
            event_id: "test".to_string(),
            event_type: "test".to_string(),
            data: vec![1, 2, 3],
            created_at: SystemTime::now(),
        };
        assert_eq!(event.event_id, "test");
        assert_eq!(event.event_type, "test");
    }

    /// Test EventHandlerConfig
    #[test]
    fn test_event_handler_config() {
        let config = EventHandlerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicEventHandler
    #[test]
    fn test_basic_event_handler() {
        let config = EventHandlerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let handler = BasicEventHandler::new(config);
        assert!(handler.config.read().unwrap().config_id == "test");
    }
} 
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

/// Task manager trait
#[async_trait]
pub trait TaskManager: Send + Sync + 'static {
    /// Thêm task
    async fn add_task(&self, task: Task) -> Result<()>;

    /// Lấy task
    async fn get_task(&self, task_id: &str) -> Result<Option<Task>>;

    /// Lấy tất cả task
    async fn get_tasks(&self) -> Result<Vec<Task>>;

    /// Xóa task
    async fn remove_task(&self, task_id: &str) -> Result<()>;

    /// Xóa tất cả task
    async fn clear_tasks(&self) -> Result<()>;
}

/// Task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// ID task
    pub task_id: String,
    /// Tên task
    pub name: String,
    /// Mô tả task
    pub description: String,
    /// Trạng thái task
    pub status: TaskStatus,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Trạng thái task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Chờ
    Pending,
    /// Đang chạy
    Running,
    /// Hoàn thành
    Completed,
    /// Lỗi
    Error,
}

/// Basic task manager
#[derive(Debug, Clone)]
pub struct BasicTaskManager {
    config: Arc<RwLock<TaskManagerConfig>>,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
}

/// Cấu hình task manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManagerConfig {
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

impl BasicTaskManager {
    /// Tạo task manager mới
    pub fn new(config: TaskManagerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl TaskManager for BasicTaskManager {
    async fn add_task(&self, task: Task) -> Result<()> {
        let mut tasks = self.tasks.write().unwrap();
        tasks.insert(task.task_id.clone(), task);
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        let tasks = self.tasks.read().unwrap();
        Ok(tasks.get(task_id).cloned())
    }

    async fn get_tasks(&self) -> Result<Vec<Task>> {
        let tasks = self.tasks.read().unwrap();
        Ok(tasks.values().cloned().collect())
    }

    async fn remove_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().unwrap();
        tasks.remove(task_id);
        Ok(())
    }

    async fn clear_tasks(&self) -> Result<()> {
        let mut tasks = self.tasks.write().unwrap();
        tasks.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test Task
    #[test]
    fn test_task() {
        let now = SystemTime::now();
        let task = Task {
            task_id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(task.task_id, "test");
        assert_eq!(task.name, "Test");
    }

    /// Test TaskStatus
    #[test]
    fn test_task_status() {
        assert_eq!(TaskStatus::Pending as u8, 0);
        assert_eq!(TaskStatus::Running as u8, 1);
        assert_eq!(TaskStatus::Completed as u8, 2);
        assert_eq!(TaskStatus::Error as u8, 3);
    }

    /// Test TaskManagerConfig
    #[test]
    fn test_task_manager_config() {
        let config = TaskManagerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicTaskManager
    #[test]
    fn test_basic_task_manager() {
        let config = TaskManagerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let manager = BasicTaskManager::new(config);
        assert!(manager.config.read().unwrap().config_id == "test");
    }
} 
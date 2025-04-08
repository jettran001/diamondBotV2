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

/// Validator trait
#[async_trait]
pub trait Validator: Send + Sync + 'static {
    /// Thêm rule
    async fn add_rule(&self, rule: Rule) -> Result<()>;

    /// Lấy rule
    async fn get_rule(&self, rule_id: &str) -> Result<Option<Rule>>;

    /// Lấy tất cả rule
    async fn get_rules(&self) -> Result<Vec<Rule>>;

    /// Xóa rule
    async fn remove_rule(&self, rule_id: &str) -> Result<()>;

    /// Xóa tất cả rule
    async fn clear_rules(&self) -> Result<()>;
}

/// Rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// ID rule
    pub rule_id: String,
    /// Tên rule
    pub name: String,
    /// Mô tả rule
    pub description: String,
    /// Trạng thái rule
    pub status: RuleStatus,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Trạng thái rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleStatus {
    /// Chờ
    Pending,
    /// Đang chạy
    Running,
    /// Hoàn thành
    Completed,
    /// Lỗi
    Error,
}

/// Basic validator
#[derive(Debug, Clone)]
pub struct BasicValidator {
    config: Arc<RwLock<ValidatorConfig>>,
    rules: Arc<RwLock<HashMap<String, Rule>>>,
}

/// Cấu hình validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
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

impl BasicValidator {
    /// Tạo validator mới
    pub fn new(config: ValidatorConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            rules: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Validator for BasicValidator {
    async fn add_rule(&self, rule: Rule) -> Result<()> {
        let mut rules = self.rules.write().unwrap();
        rules.insert(rule.rule_id.clone(), rule);
        Ok(())
    }

    async fn get_rule(&self, rule_id: &str) -> Result<Option<Rule>> {
        let rules = self.rules.read().unwrap();
        Ok(rules.get(rule_id).cloned())
    }

    async fn get_rules(&self) -> Result<Vec<Rule>> {
        let rules = self.rules.read().unwrap();
        Ok(rules.values().cloned().collect())
    }

    async fn remove_rule(&self, rule_id: &str) -> Result<()> {
        let mut rules = self.rules.write().unwrap();
        rules.remove(rule_id);
        Ok(())
    }

    async fn clear_rules(&self) -> Result<()> {
        let mut rules = self.rules.write().unwrap();
        rules.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test Rule
    #[test]
    fn test_rule() {
        let now = SystemTime::now();
        let rule = Rule {
            rule_id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            status: RuleStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(rule.rule_id, "test");
        assert_eq!(rule.name, "Test");
    }

    /// Test RuleStatus
    #[test]
    fn test_rule_status() {
        assert_eq!(RuleStatus::Pending as u8, 0);
        assert_eq!(RuleStatus::Running as u8, 1);
        assert_eq!(RuleStatus::Completed as u8, 2);
        assert_eq!(RuleStatus::Error as u8, 3);
    }

    /// Test ValidatorConfig
    #[test]
    fn test_validator_config() {
        let config = ValidatorConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicValidator
    #[test]
    fn test_basic_validator() {
        let config = ValidatorConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let validator = BasicValidator::new(config);
        assert!(validator.config.read().unwrap().config_id == "test");
    }
} 
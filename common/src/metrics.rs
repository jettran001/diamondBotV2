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

/// Metrics trait
#[async_trait]
pub trait Metrics: Send + Sync + 'static {
    /// Ghi metric
    async fn record(&self, name: &str, value: f64) -> Result<()>;

    /// Lấy metric
    async fn get_metric(&self, name: &str) -> Result<Option<MetricValue>>;

    /// Lấy tất cả metrics
    async fn get_metrics(&self) -> Result<HashMap<String, MetricValue>>;

    /// Xóa metric
    async fn clear_metric(&self, name: &str) -> Result<()>;

    /// Xóa tất cả metrics
    async fn clear_metrics(&self) -> Result<()>;
}

/// Giá trị metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    /// Giá trị
    pub value: f64,
    /// Số lần ghi
    pub count: u64,
    /// Giá trị tối thiểu
    pub min: f64,
    /// Giá trị tối đa
    pub max: f64,
    /// Giá trị trung bình
    pub avg: f64,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Basic metrics
#[derive(Debug, Clone)]
pub struct BasicMetrics {
    config: Arc<RwLock<MetricsConfig>>,
    metrics: Arc<RwLock<HashMap<String, MetricValue>>>,
}

/// Cấu hình metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
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

impl BasicMetrics {
    /// Tạo metrics mới
    pub fn new(config: MetricsConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Metrics for BasicMetrics {
    async fn record(&self, name: &str, value: f64) -> Result<()> {
        let mut metrics = self.metrics.write().unwrap();
        let now = SystemTime::now();
        let metric = metrics.entry(name.to_string()).or_insert_with(|| MetricValue {
            value,
            count: 0,
            min: f64::MAX,
            max: f64::MIN,
            avg: 0.0,
            created_at: now,
            updated_at: now,
        });
        metric.value = value;
        metric.count += 1;
        metric.min = metric.min.min(value);
        metric.max = metric.max.max(value);
        metric.avg = (metric.avg * (metric.count - 1) as f64 + value) / metric.count as f64;
        metric.updated_at = now;
        Ok(())
    }

    async fn get_metric(&self, name: &str) -> Result<Option<MetricValue>> {
        let metrics = self.metrics.read().unwrap();
        Ok(metrics.get(name).cloned())
    }

    async fn get_metrics(&self) -> Result<HashMap<String, MetricValue>> {
        let metrics = self.metrics.read().unwrap();
        Ok(metrics.clone())
    }

    async fn clear_metric(&self, name: &str) -> Result<()> {
        let mut metrics = self.metrics.write().unwrap();
        metrics.remove(name);
        Ok(())
    }

    async fn clear_metrics(&self) -> Result<()> {
        let mut metrics = self.metrics.write().unwrap();
        metrics.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test MetricValue
    #[test]
    fn test_metric_value() {
        let now = SystemTime::now();
        let value = MetricValue {
            value: 42.0,
            count: 1,
            min: 42.0,
            max: 42.0,
            avg: 42.0,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(value.value, 42.0);
        assert_eq!(value.count, 1);
    }

    /// Test MetricsConfig
    #[test]
    fn test_metrics_config() {
        let config = MetricsConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicMetrics
    #[test]
    fn test_basic_metrics() {
        let config = MetricsConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        let metrics = BasicMetrics::new(config);
        assert!(metrics.config.read().unwrap().config_id == "test");
    }
} 
// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

// Third party imports
use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Cấu hình retry policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: u64,
    pub max_delay: u64,
    pub jitter: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Retry policy
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    config: RetryConfig,
    metrics: Arc<RwLock<RetryMetrics>>,
}

/// Metrics cho retry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetryMetrics {
    pub total_retries: u64,
    pub successful_retries: u64,
    pub failed_retries: u64,
    pub total_time: Duration,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl RetryPolicy {
    /// Tạo retry policy mới
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(RetryMetrics::default())),
        }
    }

    /// Thực hiện retry
    pub async fn retry<F, T, E>(&self, f: F) -> Result<T>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>> + Send + Sync,
        E: std::error::Error + Send + Sync + 'static,
    {
        let mut retries = 0;
        let mut delay = self.config.initial_delay;
        let start_time = Instant::now();

        loop {
            match f().await {
                Ok(result) => {
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.total_retries += retries;
                    metrics.successful_retries += 1;
                    metrics.total_time += start_time.elapsed();
                    return Ok(result);
                }
                Err(e) => {
                    if retries >= self.config.max_retries {
                        let mut metrics = self.metrics.write().unwrap();
                        metrics.total_retries += retries;
                        metrics.failed_retries += 1;
                        metrics.total_time += start_time.elapsed();
                        return Err(anyhow::Error::new(e));
                    }

                    retries += 1;
                    let jitter = rand::random::<f64>() * self.config.jitter;
                    let delay_with_jitter = (delay as f64 * (1.0 + jitter)) as u64;
                    tokio::time::sleep(Duration::from_millis(delay_with_jitter)).await;
                    delay = std::cmp::min(delay * 2, self.config.max_delay);
                }
            }
        }
    }

    /// Lấy metrics
    pub fn get_metrics(&self) -> RetryMetrics {
        self.metrics.read().unwrap().clone()
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test RetryConfig
    #[test]
    fn test_retry_config() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: 100,
            max_delay: 1000,
            jitter: 0.1,
            created_at: chrono::Utc::now(),
        };
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, 100);
    }

    /// Test RetryMetrics
    #[test]
    fn test_retry_metrics() {
        let metrics = RetryMetrics {
            total_retries: 10,
            successful_retries: 8,
            failed_retries: 2,
            total_time: Duration::from_secs(1),
            created_at: chrono::Utc::now(),
        };
        assert_eq!(metrics.total_retries, 10);
        assert_eq!(metrics.successful_retries, 8);
    }

    /// Test RetryPolicy
    #[test]
    fn test_retry_policy() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: 100,
            max_delay: 1000,
            jitter: 0.1,
            created_at: chrono::Utc::now(),
        };
        let policy = RetryPolicy::new(config);
        assert_eq!(policy.config.max_retries, 3);
    }
} 
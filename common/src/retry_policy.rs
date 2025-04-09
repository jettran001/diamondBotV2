// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
    collections::HashMap,
};

// Third party imports
use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};
use tokio::time::sleep;
use rand::Rng;

/// Cấu hình retry policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Số lần retry tối đa
    pub max_retries: u32,
    /// Độ trễ ban đầu (milliseconds)
    pub initial_delay: u64,
    /// Độ trễ tối đa (milliseconds)
    pub max_delay: u64,
    /// Hệ số jitter (0-1) để tránh thundering herd
    pub jitter: f64,
    /// Hệ số backoff cho mỗi lần retry
    pub backoff_factor: f64,
    /// Cho phép retry hay không
    pub enabled: bool,
    /// Thời gian tạo
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
    pub average_retries: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Stats cho retry (tương thích với legacy RetryStats)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStats {
    pub total_attempts: u64,
    pub successful_attempts: u64,
    pub failed_attempts: u64,
    pub average_retries: f64,
    pub timestamp: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: 100,
            max_delay: 10000,
            jitter: 0.1,
            backoff_factor: 2.0,
            enabled: true,
            created_at: chrono::Utc::now(),
        }
    }
}

impl RetryPolicy {
    /// Tạo retry policy mới
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(RetryMetrics {
                created_at: chrono::Utc::now(),
                ..Default::default()
            })),
        }
    }

    /// Thực hiện retry
    pub async fn retry<F, T, E>(&self, f: F) -> Result<T>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>> + Send + Sync, 
        E: std::error::Error + Send + Sync + 'static,
    {
        if !self.config.enabled {
            return match f().await {
                Ok(result) => Ok(result),
                Err(e) => Err(anyhow::Error::new(e)),
            };
        }

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
                    
                    if metrics.successful_retries > 0 {
                        metrics.average_retries = metrics.total_retries as f64 / metrics.successful_retries as f64;
                    }
                    
                    debug!("Operation succeeded after {} retries in {:?}", retries, start_time.elapsed());
                    return Ok(result);
                }
                Err(e) => {
                    if retries >= self.config.max_retries {
                        let mut metrics = self.metrics.write().unwrap();
                        metrics.total_retries += retries;
                        metrics.failed_retries += 1;
                        metrics.total_time += start_time.elapsed();
                        
                        error!("Operation failed after {} retries: {}", retries, e);
                        return Err(anyhow::Error::new(e).context(format!("Failed after {} retries", retries)));
                    }

                    retries += 1;
                    let jitter_factor = 1.0 + (rand::random::<f64>() * self.config.jitter);
                    let delay_with_jitter = (delay as f64 * jitter_factor) as u64;
                    
                    debug!("Retry {}/{}: waiting for {}ms before next attempt", 
                           retries, self.config.max_retries, delay_with_jitter);
                    
                    sleep(Duration::from_millis(delay_with_jitter)).await;
                    delay = std::cmp::min(
                        (delay as f64 * self.config.backoff_factor) as u64, 
                        self.config.max_delay
                    );
                }
            }
        }
    }

    /// Lấy metrics
    pub fn get_metrics(&self) -> RetryMetrics {
        self.metrics.read().unwrap().clone()
    }
    
    /// Lấy stats theo định dạng cũ
    pub fn get_stats(&self) -> RetryStats {
        let metrics = self.metrics.read().unwrap();
        RetryStats {
            total_attempts: metrics.total_retries + metrics.successful_retries,
            successful_attempts: metrics.successful_retries,
            failed_attempts: metrics.failed_retries,
            average_retries: metrics.average_retries,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
    
    /// Kiểm tra xem retry có được bật không
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
    
    /// Thiết lập trạng thái bật/tắt retry
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }
    
    /// Cấu hình lại retry policy
    pub fn reconfigure(&mut self, config: RetryConfig) {
        self.config = config;
    }
}

/// Trait cho các đối tượng hỗ trợ retry
#[async_trait]
pub trait Retryable: Send + Sync + 'static {
    /// Loại dữ liệu trả về
    type Output;
    
    /// Thực hiện operation với retry
    async fn execute_with_retry(&self, retry_policy: &RetryPolicy) -> Result<Self::Output>;
}

/// Circuit breaker để tránh retry quá nhiều
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Số lần lỗi liên tiếp tối đa trước khi mở circuit
    failure_threshold: u32,
    /// Thời gian đợi trước khi thử lại sau khi mở circuit (milliseconds)
    reset_timeout: u64,
    /// Trạng thái hiện tại
    state: RwLock<CircuitBreakerState>,
}

/// Trạng thái của circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Circuit đóng, cho phép thực hiện operation
    Closed,
    /// Circuit mở, không cho phép thực hiện operation
    Open,
    /// Circuit đang trong trạng thái half-open, thử thực hiện operation
    HalfOpen,
}

impl CircuitBreaker {
    /// Tạo circuit breaker mới
    pub fn new(failure_threshold: u32, reset_timeout: u64) -> Self {
        Self {
            failure_threshold,
            reset_timeout,
            state: RwLock::new(CircuitBreakerState::Closed),
        }
    }
    
    /// Kiểm tra xem circuit có cho phép thực hiện operation không
    pub fn can_execute(&self) -> bool {
        let state = *self.state.read().unwrap();
        state != CircuitBreakerState::Open
    }
    
    /// Thông báo thành công
    pub fn on_success(&self) {
        let mut state = self.state.write().unwrap();
        if *state == CircuitBreakerState::HalfOpen {
            *state = CircuitBreakerState::Closed;
            debug!("Circuit breaker state changed to Closed after successful operation");
        }
    }
    
    /// Thông báo lỗi
    pub fn on_failure(&self) {
        let mut state = self.state.write().unwrap();
        if *state == CircuitBreakerState::Closed {
            *state = CircuitBreakerState::Open;
            debug!("Circuit breaker state changed to Open after failure");
            
            // Reset circuit breaker sau một thời gian
            let reset_timeout = self.reset_timeout;
            let state_clone = self.state.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(reset_timeout)).await;
                let mut state = state_clone.write().unwrap();
                *state = CircuitBreakerState::HalfOpen;
                debug!("Circuit breaker state changed to HalfOpen after reset timeout");
            });
        }
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
            backoff_factor: 2.0,
            enabled: true,
            created_at: chrono::Utc::now(),
        };
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, 100);
        assert_eq!(config.backoff_factor, 2.0);
        assert!(config.enabled);
    }

    /// Test RetryMetrics
    #[test]
    fn test_retry_metrics() {
        let metrics = RetryMetrics {
            total_retries: 10,
            successful_retries: 8,
            failed_retries: 2,
            total_time: Duration::from_secs(1),
            average_retries: 1.25,
            created_at: chrono::Utc::now(),
        };
        assert_eq!(metrics.total_retries, 10);
        assert_eq!(metrics.successful_retries, 8);
        assert_eq!(metrics.average_retries, 1.25);
    }

    /// Test RetryPolicy
    #[test]
    fn test_retry_policy() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: 100,
            max_delay: 1000,
            jitter: 0.1,
            backoff_factor: 2.0,
            enabled: true,
            created_at: chrono::Utc::now(),
        };
        let policy = RetryPolicy::new(config);
        assert_eq!(policy.config.max_retries, 3);
        assert!(policy.is_enabled());
    }
    
    /// Test CircuitBreaker
    #[test]
    fn test_circuit_breaker() {
        let cb = CircuitBreaker::new(3, 1000);
        assert!(cb.can_execute());
        
        cb.on_failure();
        assert!(!cb.can_execute());
        
        // Đặt lại trạng thái để test tiếp
        *cb.state.write().unwrap() = CircuitBreakerState::HalfOpen;
        assert!(cb.can_execute());
        
        cb.on_success();
        assert_eq!(*cb.state.read().unwrap(), CircuitBreakerState::Closed);
    }
} 
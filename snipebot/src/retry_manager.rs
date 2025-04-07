use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStats {
    pub total_attempts: u64,
    pub successful_attempts: u64,
    pub failed_attempts: u64,
    pub average_retries: f64,
    pub timestamp: u64,
} 
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub url: String,
    pub timeout: Duration,
    pub retry_count: u32,
    pub retry_delay: Duration,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub active_connections: u32,
    pub total_requests: u64,
    pub success_rate: f64,
    pub average_latency: Duration,
    pub timestamp: u64,
} 
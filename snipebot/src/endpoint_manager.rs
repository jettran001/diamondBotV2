use serde::{Serialize, Deserialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub url: String,
    pub chain_id: u64,
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointStats {
    pub total_requests: u64,
    pub success_rate: f64,
    pub average_latency: u64,
    pub last_error: Option<String>,
    pub timestamp: u64,
} 
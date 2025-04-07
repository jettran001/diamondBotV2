use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTuningConfig {
    pub enabled: bool,
    pub update_interval: Duration,
    pub max_adjustment: f64,
    pub min_adjustment: f64,
    pub learning_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTuningStats {
    pub total_adjustments: u64,
    pub successful_adjustments: u64,
    pub failed_adjustments: u64,
    pub average_performance: f64,
    pub timestamp: u64,
} 
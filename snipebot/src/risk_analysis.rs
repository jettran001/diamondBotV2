use ethers::types::{Address, U256};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAnalysis {
    pub token_address: Address,
    pub risk_score: f64,
    pub risk_factors: Vec<RiskFactor>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub name: String,
    pub score: f64,
    pub description: String,
} 
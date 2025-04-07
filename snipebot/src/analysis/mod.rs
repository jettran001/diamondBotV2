pub mod wasm_engine;
pub mod token_analyzer;

use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::sync::Arc;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAnalysisResult {
    pub address: String,
    pub is_honeypot: bool,
    pub is_mintable: bool,
    pub has_blacklist: bool,
    pub has_whitelist: bool,
    pub has_trading_cooldown: bool,
    pub has_anti_whale: bool,
    pub has_high_fee: bool,
    pub risk_score: u8,  // 0-100
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionAnalysisResult {
    pub transaction_hash: String,
    pub is_swap: bool,
    pub token_address: Option<String>,
    pub value_usd: Option<f64>,
    pub method_id: String,
    pub method_name: Option<String>,
    pub gas_price: u64,
    pub priority: u8,  // 0-10
}

pub async fn init_analysis_system(config: Arc<Config>) -> Result<()> {
    // Khởi tạo WASM engine
    wasm_engine::init_wasm_engine().await?;
    
    // Khởi tạo token analyzer
    token_analyzer::init_token_analyzer(config).await?;
    
    Ok(())
}

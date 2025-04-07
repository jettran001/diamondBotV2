use serde::{Serialize, Deserialize};
// Bỏ import dotenv nếu không cần
// use dotenv::dotenv;
// use std::sync::Arc;
// use std::env;
use std::path::Path;
use std::fs;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Cấu hình chung
    pub chain_id: u64,
    pub rpc_url: String,
    pub wallet_folder: String,
    pub wallet_encryption_seed: String,
    
    // Cấu hình gas
    pub gas_limit: u64,
    pub gas_price_multiplier: f64,
    pub max_gas_price: Option<u64>,
    
    // Thời gian chờ giao dịch
    pub transaction_timeout: u64, // Thời gian (giây) chờ giao dịch được xác nhận
    
    // Mạng và chuỗi
    pub network_name: String, // ethereum, binance, polygon, etc.
    pub native_symbol: String, // ETH, BNB, MATIC, etc.
    
    // Cấu hình ví
    pub auto_balance_check: bool, // Tự động kiểm tra số dư
    pub balance_check_interval: u64, // Khoảng thời gian kiểm tra (giây)
}

impl Config {
    pub fn new() -> Self {
        Self {
            chain_id: 1, // Default: Ethereum Mainnet
            rpc_url: "https://ethereum.publicnode.com".to_string(),
            wallet_folder: "./data/wallets".to_string(),
            wallet_encryption_seed: "default_encryption_seed_change_this".to_string(),
            gas_limit: 300000,
            gas_price_multiplier: 1.1,
            max_gas_price: Some(100000000000), // 100 gwei
            transaction_timeout: 120, // 2 phút
            network_name: "ethereum".to_string(),
            native_symbol: "ETH".to_string(),
            auto_balance_check: true,
            balance_check_interval: 300, // 5 phút
        }
    }
    
    pub fn from_file(path: &str) -> Result<Self> {
        if !Path::new(path).exists() {
            let default_config = Self::new();
            let json = serde_json::to_string_pretty(&default_config)?;
            
            if let Some(parent) = Path::new(path).parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            
            fs::write(path, json)?;
            return Ok(default_config);
        }
        
        let contents = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }
    
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        
        if let Some(parent) = Path::new(path).parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        
        fs::write(path, json)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
} 
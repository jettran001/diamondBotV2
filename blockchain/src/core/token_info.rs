// External imports
use ethers::{
    core::types::{Address, U256},
    contract::Contract,
};

// Standard library imports
use std::{
    sync::Arc,
    str::FromStr,
    time::Instant,
};

// Third party imports
use anyhow::{Result, anyhow, Context};
use serde::{Serialize, Deserialize};
use tracing::{warn, debug};

// Internal imports
use crate::abi;
use common::cache::{Cache, CacheEntry};

/// Cấu trúc thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Địa chỉ của token
    pub address: String,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số chữ số thập phân
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U256,
}

impl TokenInfo {
    /// Tạo thông tin token mới
    pub fn new(address: String, name: String, symbol: String, decimals: u8, total_supply: U256) -> Self {
        Self {
            address,
            name,
            symbol,
            decimals,
            total_supply,
        }
    }
    
    /// Tạo thông tin token mặc định với địa chỉ
    pub fn default_with_address(address: String) -> Self {
        Self {
            address,
            name: "Unknown".to_string(),
            symbol: "UNK".to_string(),
            decimals: 18,
            total_supply: U256::zero(),
        }
    }
    
    /// Tạo key cache cho token
    pub fn cache_key(address: &str) -> String {
        format!("token_info_{}", address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_token_info_creation() {
        let token_info = TokenInfo::new(
            "0x1234567890123456789012345678901234567890".to_string(),
            "Test Token".to_string(),
            "TEST".to_string(),
            18,
            U256::from(1000000000000000000u64),
        );
        
        assert_eq!(token_info.address, "0x1234567890123456789012345678901234567890");
        assert_eq!(token_info.name, "Test Token");
        assert_eq!(token_info.symbol, "TEST");
        assert_eq!(token_info.decimals, 18);
        assert_eq!(token_info.total_supply, U256::from(1000000000000000000u64));
    }
    
    #[test]
    fn test_token_info_default() {
        let address = "0x1234567890123456789012345678901234567890".to_string();
        let token_info = TokenInfo::default_with_address(address.clone());
        
        assert_eq!(token_info.address, address);
        assert_eq!(token_info.name, "Unknown");
        assert_eq!(token_info.symbol, "UNK");
        assert_eq!(token_info.decimals, 18);
        assert_eq!(token_info.total_supply, U256::zero());
    }
    
    #[test]
    fn test_cache_key() {
        let address = "0x1234567890123456789012345678901234567890";
        let key = TokenInfo::cache_key(address);
        
        assert_eq!(key, "token_info_0x1234567890123456789012345678901234567890");
    }
} 
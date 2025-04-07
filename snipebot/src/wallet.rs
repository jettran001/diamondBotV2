use ethers::types::U256;
use serde::{Serialize, Deserialize};
use crate::token_status::TokenBalance;

/// Cấu trúc lưu trữ số dư ví
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    pub native: U256,
    pub tokens: Vec<TokenBalance>,
} 
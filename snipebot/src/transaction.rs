use ethers::types::{Address, U256, H256, U64};
use serde::{Serialize, Deserialize};

/// Cấu trúc lưu trữ thông tin giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub to_address: Address,
    pub transaction_hash: H256,
    pub block_number: U64,
    pub from_address: Address,
    pub value: U256,
    pub gas_used: U256,
    pub gas_price: U256,
    pub success: bool,
} 
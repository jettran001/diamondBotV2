use serde::{Serialize, Deserialize};
use std::sync::Arc;
use ethers::types::U256;

/// Cấu trúc thống kê mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

/// Thông tin về trạng thái mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    pub gas_price: u64,
    pub congestion_level: u8, // 0-100
    pub block_time: f64,
    pub pending_tx_count: u32,
    pub base_fee: U256, // Base fee của block mới nhất
} 
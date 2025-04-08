// External imports
use anyhow::{Result, Context};
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

// Third party imports
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use ethers::types::{Block as EthBlock, U64};

/// Trạng thái block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockStatus {
    /// Đang chờ
    Pending,
    /// Đã xác nhận
    Confirmed,
    /// Đã thất bại
    Failed,
    /// Đã bị bỏ qua
    Skipped,
}

impl Display for BlockStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockStatus::Pending => write!(f, "Pending"),
            BlockStatus::Confirmed => write!(f, "Confirmed"),
            BlockStatus::Failed => write!(f, "Failed"),
            BlockStatus::Skipped => write!(f, "Skipped"),
        }
    }
}

/// Cấu trúc Block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: Uuid,
    pub hash: String,
    pub parent_hash: String,
    pub number: u64,
    pub timestamp: u64,
    pub author: String,
    pub base_fee_per_gas: Option<U256>,
    pub created_at: DateTime<Utc>,
    pub difficulty: U256,
    pub extra_data: Vec<u8>,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub logs_bloom: Vec<u8>,
    pub miner: String,
    pub mix_hash: String,
    pub nonce: u64,
    pub receipts_root: String,
    pub sha3_uncles: String,
    pub size: u64,
    pub state_root: String,
    pub total_difficulty: U256,
    pub transactions: Vec<String>,
    pub transactions_root: String,
    pub uncles: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub status: BlockStatus,
}

impl Block {
    /// Tạo block mới
    pub fn new(number: u64, hash: String, parent_hash: String, transactions: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            number,
            hash,
            parent_hash,
            miner: Address::zero().to_string(),
            difficulty: U256::zero(),
            total_difficulty: U256::zero(),
            size: 0,
            gas_limit: U256::zero(),
            gas_used: U256::zero(),
            timestamp: 0,
            transactions,
            status: BlockStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: String::new(),
            base_fee_per_gas: None,
            extra_data: Vec::new(),
            logs_bloom: Vec::new(),
            mix_hash: String::new(),
            nonce: 0,
            receipts_root: String::new(),
            sha3_uncles: String::new(),
            state_root: String::new(),
            transactions_root: String::new(),
            uncles: Vec::new(),
        }
    }

    /// Cập nhật thông tin block
    pub fn update_info(
        &mut self,
        miner: Address,
        difficulty: U256,
        total_difficulty: U256,
        size: u64,
        gas_limit: U256,
        gas_used: U256,
        timestamp: u64,
    ) {
        self.miner = miner.to_string();
        self.difficulty = difficulty;
        self.total_difficulty = total_difficulty;
        self.size = size;
        self.gas_limit = gas_limit;
        self.gas_used = gas_used;
        self.timestamp = timestamp;
        self.updated_at = Utc::now();
    }

    /// Cập nhật trạng thái
    pub fn update_status(&mut self, status: BlockStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Thêm giao dịch
    pub fn add_transaction(&mut self, tx_hash: String) {
        self.transactions.push(tx_hash);
        self.updated_at = Utc::now();
    }

    /// Tạo từ block Ethereum
    pub fn from_eth_block(block: EthBlock<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            number: block.number.map(|n| n.as_u64()).unwrap_or_default(),
            hash: block.hash.map(|h| h.to_string()).unwrap_or_default(),
            parent_hash: block.parent_hash.to_string(),
            miner: block.author.map(|a| a.to_string()).unwrap_or_default(),
            difficulty: block.difficulty,
            total_difficulty: block.total_difficulty.expect("Total difficulty is required"),
            size: block.size.map(|s| s.as_u64()).unwrap_or_default(),
            gas_limit: block.gas_limit,
            gas_used: block.gas_used,
            timestamp: block.timestamp.as_u64(),
            transactions: block.transactions.iter().map(|tx| tx.to_string()).collect(),
            status: BlockStatus::Confirmed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: block.author.map(|a| a.to_string()).unwrap_or_default(),
            base_fee_per_gas: block.base_fee_per_gas,
            extra_data: block.extra_data.to_vec(),
            logs_bloom: block.logs_bloom.map(|b| b.0.to_vec()).unwrap_or_default(),
            mix_hash: block.mix_hash.map(|h| h.to_string()).unwrap_or_default(),
            nonce: block.nonce.map(|n| n.0[0] as u64).unwrap_or_default(),
            receipts_root: block.receipts_root.to_string(),
            sha3_uncles: block.uncles_hash.to_string(),
            state_root: block.state_root.to_string(),
            transactions_root: block.transactions_root.to_string(),
            uncles: block.uncles.iter().map(|u| u.to_string()).collect(),
        }
    }

    /// Kiểm tra block đã được xác nhận
    pub fn is_confirmed(&self) -> bool {
        self.status == BlockStatus::Confirmed
    }

    /// Tính tổng phí gas
    pub fn calculate_total_gas_fee(&self) -> Option<U256> {
        Some(self.gas_used * self.difficulty)
    }
}

impl From<EthBlock<String>> for Block {
    fn from(block: EthBlock<String>) -> Self {
        Self::from_eth_block(block)
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test tạo block
    #[test]
    fn test_create_block() {
        let block = Block::new(
            1,
            String::from("hash1"),
            String::from("parent_hash1"),
            vec![],
        );
        assert_eq!(block.number, 1);
        assert_eq!(block.status, BlockStatus::Pending);
        assert!(block.transactions.is_empty());
    }

    /// Test cập nhật thông tin
    #[test]
    fn test_update_info() {
        let mut block = Block::new(
            1,
            String::from("hash1"),
            String::from("parent_hash1"),
            vec![],
        );
        block.update_info(
            Address::random(),
            U256::from(1000000),
            U256::from(10000000),
            1000,
            U256::from(8000000),
            U256::from(4000000),
            1234567890,
        );
        assert_ne!(block.miner, Address::zero().to_string());
        assert_eq!(block.difficulty, U256::from(1000000));
        assert_eq!(block.total_difficulty, U256::from(10000000));
        assert_eq!(block.size, 1000);
        assert_eq!(block.gas_limit, U256::from(8000000));
        assert_eq!(block.gas_used, U256::from(4000000));
        assert_eq!(block.timestamp, 1234567890);
    }

    /// Test cập nhật trạng thái
    #[test]
    fn test_update_status() {
        let mut block = Block::new(
            1,
            String::from("hash1"),
            String::from("parent_hash1"),
            vec![],
        );
        block.update_status(BlockStatus::Confirmed);
        assert_eq!(block.status, BlockStatus::Confirmed);
    }

    /// Test thêm giao dịch
    #[test]
    fn test_add_transaction() {
        let mut block = Block::new(
            1,
            String::from("hash1"),
            String::from("parent_hash1"),
            vec![],
        );
        let tx_hash = String::from("tx_hash1");
        block.add_transaction(tx_hash);
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.transactions[0], tx_hash);
    }
} 
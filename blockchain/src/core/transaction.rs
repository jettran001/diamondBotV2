// External imports
use ethers::{
    core::types::{TransactionReceipt, H256, U256, Transaction, Block, BlockId, BlockNumber},
    providers::{Middleware, PendingTransaction},
};

// Standard library imports
use std::{
    sync::Arc,
    str::FromStr,
    time::Duration,
};

// Third party imports
use anyhow::{Result, anyhow, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::timeout;

/// Cấu trúc thông tin giao dịch mở rộng
#[derive(Debug, Clone)]
pub struct TransactionInfo {
    /// Hash giao dịch
    pub hash: H256,
    /// Biên lai giao dịch
    pub receipt: Option<TransactionReceipt>,
    /// Số xác nhận hiện tại
    pub confirmations: u64,
    /// Thời gian đợi (giây)
    pub wait_time: u64,
    /// Trạng thái
    pub status: TransactionStatus,
}

/// Trạng thái giao dịch
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Đang chờ
    Pending,
    /// Đã xác nhận
    Confirmed,
    /// Thất bại
    Failed,
    /// Timeout
    Timeout,
    /// Không tìm thấy
    NotFound,
}

impl TransactionInfo {
    /// Tạo thông tin giao dịch mới
    pub fn new(hash: H256) -> Self {
        Self {
            hash,
            receipt: None,
            confirmations: 0,
            wait_time: 0,
            status: TransactionStatus::Pending,
        }
    }
    
    /// Kiểm tra giao dịch đã thành công
    pub fn is_success(&self) -> bool {
        if let Some(receipt) = &self.receipt {
            if let Some(status) = receipt.status {
                return status.as_u64() == 1;
            }
        }
        false
    }
    
    /// Kiểm tra giao dịch đã thất bại
    pub fn is_failed(&self) -> bool {
        if let Some(receipt) = &self.receipt {
            if let Some(status) = receipt.status {
                return status.as_u64() == 0;
            }
        }
        self.status == TransactionStatus::Failed
    }
    
    /// Kiểm tra giao dịch đã timeout
    pub fn is_timeout(&self) -> bool {
        self.status == TransactionStatus::Timeout
    }
    
    /// Kiểm tra giao dịch đã hoàn thành (thành công hoặc thất bại)
    pub fn is_completed(&self) -> bool {
        self.status == TransactionStatus::Confirmed || 
        self.status == TransactionStatus::Failed
    }
    
    /// Lấy gas đã sử dụng
    pub fn gas_used(&self) -> Option<U256> {
        self.receipt.as_ref().and_then(|r| r.gas_used)
    }
    
    /// Lấy block hash chứa giao dịch
    pub fn block_hash(&self) -> Option<H256> {
        self.receipt.as_ref().and_then(|r| r.block_hash)
    }
    
    /// Lấy block number chứa giao dịch
    pub fn block_number(&self) -> Option<U256> {
        self.receipt.as_ref().and_then(|r| r.block_number)
    }
}

/// Trait cung cấp các phương thức xử lý giao dịch
#[async_trait]
pub trait TransactionHandler: Send + Sync + 'static {
    /// Lấy biên lai giao dịch
    async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>>;
    
    /// Đợi giao dịch được xác nhận
    async fn wait_for_transaction(&self, tx_hash: &str, timeout_secs: u64) -> Result<Option<TransactionReceipt>>;
    
    /// Lấy thông tin giao dịch đầy đủ
    async fn get_transaction(&self, tx_hash: &str) -> Result<Option<Transaction>>;
    
    /// Lấy thông tin giao dịch mở rộng
    async fn get_transaction_info(&self, tx_hash: &str, timeout_secs: u64) -> Result<TransactionInfo> {
        let hash = H256::from_str(tx_hash)
            .with_context(|| format!("Định dạng transaction hash không hợp lệ: {}", tx_hash))?;
        
        let mut tx_info = TransactionInfo::new(hash);
        
        match self.wait_for_transaction(tx_hash, timeout_secs).await {
            Ok(Some(receipt)) => {
                tx_info.receipt = Some(receipt);
                if tx_info.is_success() {
                    tx_info.status = TransactionStatus::Confirmed;
                } else {
                    tx_info.status = TransactionStatus::Failed;
                }
            },
            Ok(None) => {
                tx_info.status = TransactionStatus::NotFound;
            },
            Err(e) => {
                if e.to_string().contains("timed out") {
                    tx_info.status = TransactionStatus::Timeout;
                } else {
                    tx_info.status = TransactionStatus::Failed;
                }
            }
        }
        
        Ok(tx_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transaction_info() {
        let hash = H256::from_str("0x1234567890123456789012345678901234567890123456789012345678901234").unwrap();
        let tx_info = TransactionInfo::new(hash);
        
        assert_eq!(tx_info.hash, hash);
        assert_eq!(tx_info.confirmations, 0);
        assert_eq!(tx_info.wait_time, 0);
        assert_eq!(tx_info.status, TransactionStatus::Pending);
        assert!(tx_info.receipt.is_none());
        
        assert!(!tx_info.is_success());
        assert!(!tx_info.is_failed());
        assert!(!tx_info.is_timeout());
        assert!(!tx_info.is_completed());
    }
} 
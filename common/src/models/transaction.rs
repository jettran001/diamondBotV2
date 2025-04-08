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
use ethers::types::{
    Transaction as EthTransaction,
    TransactionReceipt,
    Bytes,
};

/// Trạng thái giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Đang chờ
    Pending,
    /// Đã xác nhận
    Confirmed,
    /// Thất bại
    Failed,
    /// Đã hủy
    Cancelled,
    /// Đã thay thế
    Replaced,
}

impl Display for TransactionStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionStatus::Pending => write!(f, "Pending"),
            TransactionStatus::Confirmed => write!(f, "Confirmed"),
            TransactionStatus::Failed => write!(f, "Failed"),
            TransactionStatus::Cancelled => write!(f, "Cancelled"),
            TransactionStatus::Replaced => write!(f, "Replaced"),
        }
    }
}

/// Loại giao dịch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionType {
    /// Chuyển ETH
    Transfer,
    /// Gọi hợp đồng
    Contract,
    /// Triển khai hợp đồng
    Deploy,
}

impl Display for TransactionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionType::Transfer => write!(f, "Transfer"),
            TransactionType::Contract => write!(f, "Contract"),
            TransactionType::Deploy => write!(f, "Deploy"),
        }
    }
}

/// Giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// ID giao dịch
    pub id: Uuid,
    /// Hash giao dịch
    pub hash: String,
    /// Địa chỉ người gửi
    pub from: String,
    /// Địa chỉ người nhận
    pub to: Option<String>,
    /// Giá trị giao dịch (wei)
    pub value: U256,
    /// Nonce
    pub nonce: u64,
    /// Dữ liệu giao dịch
    pub data: Vec<u8>,
    /// Gas price (wei)
    pub gas_price: U256,
    /// Gas limit
    pub gas_limit: U256,
    /// Gas đã sử dụng
    pub gas_used: Option<U256>,
    /// Loại giao dịch
    pub transaction_type: Option<u64>,
    /// Trạng thái
    pub status: TransactionStatus,
    /// Block number
    pub block_number: Option<u64>,
    /// Block hash
    pub block_hash: Option<String>,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Transaction index
    pub transaction_index: Option<u64>,
    /// Access list
    pub access_list: Option<Vec<(String, Vec<String>)>>,
    /// Max priority fee per gas
    pub max_priority_fee_per_gas: Option<U256>,
    /// Max fee per gas
    pub max_fee_per_gas: Option<U256>,
    /// Chain ID
    pub chain_id: Option<u64>,
    /// Signature v
    pub v: u64,
    /// Signature r
    pub r: U256,
    /// Signature s
    pub s: U256,
    /// Input data
    pub input: Vec<u8>,
    /// Contract address
    pub contract_address: Option<String>,
    /// Logs
    pub logs: Option<Vec<String>>,
    /// Cumulative gas used
    pub cumulative_gas_used: Option<U256>,
    /// Effective gas price
    pub effective_gas_price: Option<U256>,
    /// Root
    pub root: Option<String>,
    /// Type
    pub type_: Option<u64>,
}

impl Transaction {
    /// Tạo giao dịch mới
    pub fn new(hash: H256, from: Address, to: Option<Address>, value: U256) -> Self {
        Self {
            id: Uuid::new_v4(),
            hash: hash.to_string(),
            from: from.to_string(),
            to: to.map(|t| t.to_string()),
            value,
            nonce: 0,
            data: vec![],
            gas_price: U256::zero(),
            gas_limit: U256::zero(),
            gas_used: None,
            transaction_type: None,
            status: TransactionStatus::Pending,
            block_number: None,
            block_hash: None,
            transaction_index: None,
            access_list: None,
            max_priority_fee_per_gas: None,
            max_fee_per_gas: None,
            chain_id: None,
            v: 0,
            r: U256::zero(),
            s: U256::zero(),
            input: vec![],
            contract_address: None,
            logs: None,
            cumulative_gas_used: None,
            effective_gas_price: None,
            root: None,
            type_: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Tạo từ giao dịch Ethereum
    pub fn from_eth_transaction(tx: EthTransaction) -> Self {
        Self {
            id: Uuid::new_v4(),
            hash: tx.hash.to_string(),
            from: tx.from.to_string(),
            to: tx.to.map(|addr| addr.to_string()),
            value: tx.value,
            nonce: tx.nonce.as_u64(),
            data: tx.input.to_vec(),
            gas_price: tx.gas_price.unwrap_or_default(),
            gas_limit: tx.gas,
            gas_used: None,
            transaction_type: tx.transaction_type.map(|t| t.as_u64()),
            status: TransactionStatus::Pending,
            block_number: tx.block_number.map(|n| n.as_u64()),
            block_hash: tx.block_hash.map(|h| h.to_string()),
            transaction_index: tx.transaction_index.map(|i| i.as_u64()),
            access_list: tx.access_list.map(|list| {
                list.0.into_iter()
                    .map(|item| (item.address.to_string(), item.storage_keys.iter().map(|k| k.to_string()).collect()))
                    .collect()
            }),
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            chain_id: tx.chain_id.map(|id| id.as_u64()),
            v: tx.v.as_u64(),
            r: tx.r,
            s: tx.s,
            input: tx.input.to_vec(),
            contract_address: None,
            logs: None,
            cumulative_gas_used: None,
            effective_gas_price: None,
            root: None,
            type_: tx.transaction_type.map(|t| t.as_u64()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Cập nhật từ receipt
    pub fn update_from_receipt(&mut self, receipt: TransactionReceipt) -> Result<()> {
        self.gas_used = receipt.gas_used;
        self.block_number = receipt.block_number.map(|n| n.as_u64());
        self.block_hash = receipt.block_hash.map(|h| h.to_string());
        self.transaction_index = Some(receipt.transaction_index.as_u64());
        self.contract_address = receipt.contract_address.map(|a| a.to_string());
        self.logs = Some(receipt.logs.iter().map(|l| format!("{:?}", l)).collect());
        self.cumulative_gas_used = Some(receipt.cumulative_gas_used);
        self.effective_gas_price = receipt.effective_gas_price;
        self.root = receipt.root.map(|r| r.to_string());
        self.type_ = receipt.transaction_type.map(|t| t.as_u64());
        
        self.status = if let Some(status) = receipt.status {
            if status.as_u64() == 1 {
                TransactionStatus::Confirmed
            } else {
                TransactionStatus::Failed
            }
        } else {
            return Err(anyhow::anyhow!("No status in receipt"));
        };

        self.updated_at = Utc::now();
        Ok(())
    }

    /// Hủy giao dịch
    pub fn cancel(&mut self) {
        self.status = TransactionStatus::Cancelled;
        self.updated_at = Utc::now();
    }

    /// Thay thế giao dịch
    pub fn replace(&mut self) {
        self.status = TransactionStatus::Replaced;
        self.updated_at = Utc::now();
    }

    /// Tính phí giao dịch
    pub fn calculate_fee(&self) -> Option<U256> {
        self.gas_used.map(|used| used * self.gas_price)
    }

    /// Kiểm tra giao dịch đã hoàn thành
    pub fn is_completed(&self) -> bool {
        matches!(self.status, TransactionStatus::Confirmed | TransactionStatus::Failed)
    }

    /// Cập nhật nonce
    pub fn update_nonce(&mut self, nonce: u64) {
        self.nonce = nonce;
        self.updated_at = Utc::now();
    }

    /// Cập nhật dữ liệu
    pub fn update_data(&mut self, data: Vec<u8>) {
        self.data = data;
        self.updated_at = Utc::now();
    }

    /// Cập nhật gas
    pub fn update_gas(&mut self, gas_price: U256, gas_limit: U256) {
        self.gas_price = gas_price;
        self.gas_limit = gas_limit;
        self.updated_at = Utc::now();
    }

    /// Cập nhật trạng thái
    pub fn update_status(&mut self, status: TransactionStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Tạo từ receipt
    pub fn from_receipt(receipt: TransactionReceipt) -> Self {
        let block_number = receipt.block_number.map(|bn| bn.as_u64());
        let block_hash = receipt.block_hash.map(|bh| bh.to_string());
        let transaction_index = Some(receipt.transaction_index.as_u64());
        let contract_address = receipt.contract_address.map(|ca| ca.to_string());
        let root = receipt.root.map(|r| r.to_string());
        let transaction_type = receipt.transaction_type.map(|tt| tt.as_u64());
        
        let status = if let Some(s) = receipt.status {
            if s.as_u64() == 1 {
                TransactionStatus::Confirmed
            } else {
                TransactionStatus::Failed
            }
        } else {
            TransactionStatus::Pending
        };
        
        Self {
            id: Uuid::new_v4(),
            hash: receipt.transaction_hash.to_string(),
            from: Address::zero().to_string(), // Không có thông tin from trong receipt
            to: None,
            value: U256::zero(),
            nonce: 0,
            data: vec![],
            gas_price: U256::zero(),
            gas_limit: U256::zero(),
            gas_used: receipt.gas_used,
            transaction_type,
            status,
            block_number,
            block_hash,
            transaction_index,
            access_list: None,
            max_priority_fee_per_gas: None,
            max_fee_per_gas: None,
            chain_id: None,
            v: 0,
            r: U256::zero(),
            s: U256::zero(),
            input: vec![],
            contract_address,
            logs: Some(receipt.logs.iter().map(|l| format!("{:?}", l)).collect()),
            cumulative_gas_used: Some(receipt.cumulative_gas_used),
            effective_gas_price: receipt.effective_gas_price,
            root,
            type_: transaction_type,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

impl From<EthTransaction> for Transaction {
    fn from(tx: EthTransaction) -> Self {
        Self::from_eth_transaction(tx)
    }
}

impl From<TransactionReceipt> for Transaction {
    fn from(receipt: TransactionReceipt) -> Self {
        Self::from_receipt(receipt)
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test tạo giao dịch
    #[test]
    fn test_create_transaction() {
        let tx = Transaction::new(
            H256::random(),
            Address::random(),
            Address::random().map(|t| t.unwrap_or_default()),
            U256::from(1000000000000000000u64),
        );
        assert_eq!(tx.status, TransactionStatus::Pending);
        assert_eq!(tx.nonce, 0);
        assert!(tx.data.is_empty());
        assert_eq!(tx.gas_price, U256::zero());
        assert_eq!(tx.gas_limit, U256::zero());
    }

    /// Test cập nhật nonce
    #[test]
    fn test_update_nonce() {
        let mut tx = Transaction::new(
            H256::random(),
            Address::random(),
            Address::random().map(|t| t.unwrap_or_default()),
            U256::from(1000000000000000000u64),
        );
        tx.update_nonce(1);
        assert_eq!(tx.nonce, 1);
    }

    /// Test cập nhật dữ liệu
    #[test]
    fn test_update_data() {
        let mut tx = Transaction::new(
            H256::random(),
            Address::random(),
            Address::random().map(|t| t.unwrap_or_default()),
            U256::from(1000000000000000000u64),
        );
        tx.update_data(vec![1, 2, 3]);
        assert_eq!(tx.data, vec![1, 2, 3]);
    }

    /// Test cập nhật gas
    #[test]
    fn test_update_gas() {
        let mut tx = Transaction::new(
            H256::random(),
            Address::random(),
            Address::random().map(|t| t.unwrap_or_default()),
            U256::from(1000000000000000000u64),
        );
        tx.update_gas(
            U256::from(1000000000),
            U256::from(21000),
        );
        assert_eq!(tx.gas_price, U256::from(1000000000));
        assert_eq!(tx.gas_limit, U256::from(21000));
    }

    /// Test cập nhật trạng thái
    #[test]
    fn test_update_status() {
        let mut tx = Transaction::new(
            H256::random(),
            Address::random(),
            Address::random().map(|t| t.unwrap_or_default()),
            U256::from(1000000000000000000u64),
        );
        tx.update_status(TransactionStatus::Confirmed);
        assert_eq!(tx.status, TransactionStatus::Confirmed);
    }
} 
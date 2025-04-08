// External imports
use ethers::types::{
    Address,
    Block as EthBlock,
    H256,
    TransactionReceipt as EthTransactionReceipt,
    U256,
};

// Standard library imports
use std::{
    fmt::Debug,
    sync::Arc,
    time::SystemTime,
};

// Third party imports
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Chain adapter trait
#[async_trait]
pub trait ChainAdapter: Send + Sync + Debug + 'static {
    /// Lấy chain ID
    async fn get_chain_id(&self) -> Result<u64>;

    /// Lấy số block
    async fn get_block_number(&self) -> Result<U256>;

    /// Lấy số dư
    async fn get_balance(&self, address: Address) -> Result<U256>;

    /// Lấy nonce
    async fn get_nonce(&self, address: Address) -> Result<U256>;

    /// Gửi giao dịch
    async fn send_transaction(&self, tx: Vec<u8>) -> Result<H256>;

    /// Lấy biên lai giao dịch
    async fn get_transaction_receipt(&self, hash: H256) -> Result<Option<EthTransactionReceipt>>;

    /// Lấy block theo số
    async fn get_block_by_number(&self, number: U256) -> Result<Option<EthBlock<Transaction>>>;

    /// Lấy block theo hash
    async fn get_block_by_hash(&self, hash: H256) -> Result<Option<EthBlock<Transaction>>>;
}

/// Biên lai giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Hash giao dịch
    pub transaction_hash: H256,
    /// Hash block
    pub block_hash: H256,
    /// Số block
    pub block_number: U256,
    /// Địa chỉ người gửi
    pub from: Address,
    /// Địa chỉ người nhận
    pub to: Option<Address>,
    /// Gas used
    pub gas_used: U256,
    /// Status
    pub status: U256,
    /// Logs
    pub logs: Vec<Log>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    /// Địa chỉ
    pub address: Address,
    /// Topics
    pub topics: Vec<H256>,
    /// Dữ liệu
    pub data: Vec<u8>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Hash block
    pub hash: H256,
    /// Hash block cha
    pub parent_hash: H256,
    /// Số block
    pub number: U256,
    /// Thời gian tạo
    pub timestamp: U256,
    /// Địa chỉ người tạo
    pub miner: Address,
    /// Độ khó
    pub difficulty: U256,
    /// Gas limit
    pub gas_limit: U256,
    /// Gas used
    pub gas_used: U256,
    /// Danh sách giao dịch
    pub transactions: Vec<Transaction>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Hash giao dịch
    pub hash: H256,
    /// Địa chỉ người gửi
    pub from: Address,
    /// Địa chỉ người nhận
    pub to: Option<Address>,
    /// Giá trị
    pub value: U256,
    /// Gas
    pub gas: U256,
    /// Gas price
    pub gas_price: U256,
    /// Nonce
    pub nonce: U256,
    /// Dữ liệu
    pub data: Vec<u8>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// EVM adapter
#[derive(Debug, Clone)]
pub struct EVMAdapter {
    client: Arc<dyn ChainAdapter>,
}

impl EVMAdapter {
    /// Tạo EVM adapter mới
    pub fn new(client: Arc<dyn ChainAdapter>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ChainAdapter for EVMAdapter {
    async fn get_chain_id(&self) -> Result<u64> {
        self.client.get_chain_id().await
    }

    async fn get_block_number(&self) -> Result<U256> {
        self.client.get_block_number().await
    }

    async fn get_balance(&self, address: Address) -> Result<U256> {
        self.client.get_balance(address).await
    }

    async fn get_nonce(&self, address: Address) -> Result<U256> {
        self.client.get_nonce(address).await
    }

    async fn send_transaction(&self, tx: Vec<u8>) -> Result<H256> {
        self.client.send_transaction(tx).await
    }

    async fn get_transaction_receipt(&self, hash: H256) -> Result<Option<EthTransactionReceipt>> {
        self.client.get_transaction_receipt(hash).await
    }

    async fn get_block_by_number(&self, number: U256) -> Result<Option<EthBlock<Transaction>>> {
        self.client.get_block_by_number(number).await
    }

    async fn get_block_by_hash(&self, hash: H256) -> Result<Option<EthBlock<Transaction>>> {
        self.client.get_block_by_hash(hash).await
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test TransactionReceipt
    #[test]
    fn test_transaction_receipt() {
        let receipt = TransactionReceipt {
            transaction_hash: H256::zero(),
            block_hash: H256::zero(),
            block_number: U256::from(1000),
            from: Address::zero(),
            to: Some(Address::zero()),
            gas_used: U256::from(1000),
            status: U256::from(1),
            logs: vec![],
            created_at: SystemTime::now(),
        };
        assert_eq!(receipt.block_number, U256::from(1000));
        assert_eq!(receipt.gas_used, U256::from(1000));
    }

    /// Test Block
    #[test]
    fn test_block() {
        let block = Block {
            hash: H256::zero(),
            parent_hash: H256::zero(),
            number: U256::from(1000),
            timestamp: U256::from(1000),
            miner: Address::zero(),
            difficulty: U256::from(1000),
            gas_limit: U256::from(1000),
            gas_used: U256::from(1000),
            transactions: vec![],
            created_at: SystemTime::now(),
        };
        assert_eq!(block.number, U256::from(1000));
        assert_eq!(block.timestamp, U256::from(1000));
    }

    /// Test Transaction
    #[test]
    fn test_transaction() {
        let tx = Transaction {
            hash: H256::zero(),
            from: Address::zero(),
            to: Some(Address::zero()),
            value: U256::from(1000),
            gas: U256::from(21000),
            gas_price: U256::from(100),
            nonce: U256::from(0),
            data: vec![],
            created_at: SystemTime::now(),
        };
        assert_eq!(tx.value, U256::from(1000));
        assert_eq!(tx.gas, U256::from(21000));
    }

    /// Test Log
    #[test]
    fn test_log() {
        let log = Log {
            address: Address::zero(),
            topics: vec![],
            data: vec![],
            created_at: SystemTime::now(),
        };
        assert_eq!(log.address, Address::zero());
    }

    /// Test EVMAdapter
    #[test]
    fn test_evm_adapter() {
        let adapter = EVMAdapter::new(Arc::new(TestChainAdapter));
        assert!(adapter.client.get_chain_id().is_ok());
    }

    /// Test chain adapter
    struct TestChainAdapter;

    #[async_trait]
    impl ChainAdapter for TestChainAdapter {
        async fn get_chain_id(&self) -> Result<u64> {
            Ok(1)
        }

        async fn get_block_number(&self) -> Result<U256> {
            Ok(U256::from(1000))
        }

        async fn get_balance(&self, _address: Address) -> Result<U256> {
            Ok(U256::from(1000))
        }

        async fn get_nonce(&self, _address: Address) -> Result<U256> {
            Ok(U256::from(0))
        }

        async fn send_transaction(&self, _tx: Vec<u8>) -> Result<H256> {
            Ok(H256::zero())
        }

        async fn get_transaction_receipt(&self, _hash: H256) -> Result<Option<EthTransactionReceipt>> {
            Ok(None)
        }

        async fn get_block_by_number(&self, _number: U256) -> Result<Option<EthBlock<Transaction>>> {
            Ok(None)
        }

        async fn get_block_by_hash(&self, _hash: H256) -> Result<Option<EthBlock<Transaction>>> {
            Ok(None)
        }
    }
} 
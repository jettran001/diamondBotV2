// External imports
use anyhow::{Result, Context};
use ethers::core::types::{Address, H256, U256};
use ethers::core::types::U64;

// Standard library imports
use std::{
    fmt::{self, Display, Formatter},
    collections::HashMap,
    time::Duration,
};

// Third party imports
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Loại chain
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChainType {
    /// Mainnet
    Mainnet,
    /// Testnet
    Testnet,
    /// Private
    Private,
}

impl Display for ChainType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChainType::Mainnet => write!(f, "Mainnet"),
            ChainType::Testnet => write!(f, "Testnet"),
            ChainType::Private => write!(f, "Private"),
        }
    }
}

/// Trạng thái chain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainStatus {
    /// Đang chạy
    Running,
    /// Đã dừng
    Stopped,
    /// Đang bảo trì
    Maintenance,
    /// Đã lỗi
    Error,
}

impl Display for ChainStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChainStatus::Running => write!(f, "Running"),
            ChainStatus::Stopped => write!(f, "Stopped"),
            ChainStatus::Maintenance => write!(f, "Maintenance"),
            ChainStatus::Error => write!(f, "Error"),
        }
    }
}

/// Thông tin chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    /// ID chain
    pub id: Uuid,
    /// Tên chain
    pub name: String,
    /// Chain ID
    pub chain_id: u64,
    /// URL RPC
    pub rpc_url: String,
    /// URL WSS
    pub wss_url: String,
    /// Giá gas
    pub gas_price: U256,
    /// Giới hạn gas
    pub gas_limit: U256,
    /// Thời gian chờ giao dịch
    pub transaction_timeout: Duration,
    /// Thời gian chờ block
    pub block_timeout: Duration,
    /// Số xác nhận tối thiểu
    pub min_confirmations: u64,
    /// Danh sách hợp đồng
    pub contracts: Vec<Address>,
    /// Trạng thái chain
    pub status: ChainStatus,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Loại chain
    pub chain_type: ChainType,
    /// Explorer URL
    pub explorer_url: String,
    /// Native token symbol
    pub native_token_symbol: String,
    /// Native token decimals
    pub native_token_decimals: u8,
    /// Block time
    pub block_time: Duration,
    /// Max priority fee per gas
    pub max_priority_fee_per_gas: U256,
    /// Max fee per gas
    pub max_fee_per_gas: U256,
    /// Chain metadata
    pub metadata: HashMap<String, String>,
}

impl Chain {
    /// Tạo chain mới
    pub fn new(
        name: String,
        chain_id: u64,
        rpc_url: String,
        wss_url: String,
        gas_price: U256,
        gas_limit: U256,
        transaction_timeout: Duration,
        block_timeout: Duration,
        min_confirmations: u64,
        contracts: Vec<Address>,
        chain_type: ChainType,
        explorer_url: String,
        native_token_symbol: String,
        native_token_decimals: u8,
        block_time: Duration,
        max_priority_fee_per_gas: U256,
        max_fee_per_gas: U256,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            chain_id,
            rpc_url,
            wss_url,
            gas_price,
            gas_limit,
            transaction_timeout,
            block_timeout,
            min_confirmations,
            contracts,
            status: ChainStatus::Running,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            chain_type,
            explorer_url,
            native_token_symbol,
            native_token_decimals,
            block_time,
            max_priority_fee_per_gas,
            max_fee_per_gas,
            metadata: HashMap::new(),
        }
    }

    /// Cập nhật thông tin chain
    pub fn update_info(
        &mut self,
        name: String,
        rpc_url: String,
        wss_url: String,
        gas_price: U256,
        gas_limit: U256,
        transaction_timeout: Duration,
        block_timeout: Duration,
        min_confirmations: u64,
        explorer_url: String,
        native_token_symbol: String,
        native_token_decimals: u8,
        block_time: Duration,
        max_priority_fee_per_gas: U256,
        max_fee_per_gas: U256,
    ) {
        self.name = name;
        self.rpc_url = rpc_url;
        self.wss_url = wss_url;
        self.gas_price = gas_price;
        self.gas_limit = gas_limit;
        self.transaction_timeout = transaction_timeout;
        self.block_timeout = block_timeout;
        self.min_confirmations = min_confirmations;
        self.explorer_url = explorer_url;
        self.native_token_symbol = native_token_symbol;
        self.native_token_decimals = native_token_decimals;
        self.block_time = block_time;
        self.max_priority_fee_per_gas = max_priority_fee_per_gas;
        self.max_fee_per_gas = max_fee_per_gas;
        self.updated_at = Utc::now();
    }

    /// Cập nhật trạng thái
    pub fn update_status(&mut self, status: ChainStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Thêm hợp đồng
    pub fn add_contract(&mut self, contract: Address) {
        self.contracts.push(contract);
        self.updated_at = Utc::now();
    }

    /// Thêm metadata
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
        self.updated_at = Utc::now();
    }

    /// Xóa metadata
    pub fn remove_metadata(&mut self, key: &str) {
        self.metadata.remove(key);
        self.updated_at = Utc::now();
    }

    /// Lấy metadata
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test tạo chain
    #[test]
    fn test_create_chain() {
        let chain = Chain::new(
            "Test Chain".to_string(),
            1,
            "http://localhost:8545".to_string(),
            "ws://localhost:8546".to_string(),
            U256::from(1000000000),
            U256::from(8000000),
            Duration::from_secs(60),
            Duration::from_secs(30),
            12,
            vec![],
            ChainType::Mainnet,
            "https://etherscan.io".to_string(),
            "ETH".to_string(),
            18,
            Duration::from_secs(12),
            U256::from(2000000000),
            U256::from(3000000000),
        );
        assert_eq!(chain.name, "Test Chain");
        assert_eq!(chain.chain_id, 1);
        assert_eq!(chain.status, ChainStatus::Running);
        assert!(chain.contracts.is_empty());
        assert_eq!(chain.chain_type, ChainType::Mainnet);
        assert_eq!(chain.native_token_symbol, "ETH");
        assert_eq!(chain.native_token_decimals, 18);
    }

    /// Test cập nhật thông tin
    #[test]
    fn test_update_info() {
        let mut chain = Chain::new(
            "Test Chain".to_string(),
            1,
            "http://localhost:8545".to_string(),
            "ws://localhost:8546".to_string(),
            U256::from(1000000000),
            U256::from(8000000),
            Duration::from_secs(60),
            Duration::from_secs(30),
            12,
            vec![],
            ChainType::Mainnet,
            "https://etherscan.io".to_string(),
            "ETH".to_string(),
            18,
            Duration::from_secs(12),
            U256::from(2000000000),
            U256::from(3000000000),
        );
        chain.update_info(
            "Updated Chain".to_string(),
            "http://localhost:8547".to_string(),
            "ws://localhost:8548".to_string(),
            U256::from(2000000000),
            U256::from(9000000),
            Duration::from_secs(120),
            Duration::from_secs(60),
            24,
            "https://etherscan.io/updated".to_string(),
            "ETH2".to_string(),
            9,
            Duration::from_secs(15),
            U256::from(3000000000),
            U256::from(4000000000),
        );
        assert_eq!(chain.name, "Updated Chain");
        assert_eq!(chain.rpc_url, "http://localhost:8547");
        assert_eq!(chain.native_token_symbol, "ETH2");
        assert_eq!(chain.native_token_decimals, 9);
    }

    /// Test cập nhật trạng thái
    #[test]
    fn test_update_status() {
        let mut chain = Chain::new(
            "Test Chain".to_string(),
            1,
            "http://localhost:8545".to_string(),
            "ws://localhost:8546".to_string(),
            U256::from(1000000000),
            U256::from(8000000),
            Duration::from_secs(60),
            Duration::from_secs(30),
            12,
            vec![],
            ChainType::Mainnet,
            "https://etherscan.io".to_string(),
            "ETH".to_string(),
            18,
            Duration::from_secs(12),
            U256::from(2000000000),
            U256::from(3000000000),
        );
        chain.update_status(ChainStatus::Maintenance);
        assert_eq!(chain.status, ChainStatus::Maintenance);
    }

    /// Test thêm hợp đồng
    #[test]
    fn test_add_contract() {
        let mut chain = Chain::new(
            "Test Chain".to_string(),
            1,
            "http://localhost:8545".to_string(),
            "ws://localhost:8546".to_string(),
            U256::from(1000000000),
            U256::from(8000000),
            Duration::from_secs(60),
            Duration::from_secs(30),
            12,
            vec![],
            ChainType::Mainnet,
            "https://etherscan.io".to_string(),
            "ETH".to_string(),
            18,
            Duration::from_secs(12),
            U256::from(2000000000),
            U256::from(3000000000),
        );
        let contract = Address::zero();
        chain.add_contract(contract);
        assert_eq!(chain.contracts.len(), 1);
        assert_eq!(chain.contracts[0], contract);
    }

    /// Test metadata
    #[test]
    fn test_metadata() {
        let mut chain = Chain::new(
            "Test Chain".to_string(),
            1,
            "http://localhost:8545".to_string(),
            "ws://localhost:8546".to_string(),
            U256::from(1000000000),
            U256::from(8000000),
            Duration::from_secs(60),
            Duration::from_secs(30),
            12,
            vec![],
            ChainType::Mainnet,
            "https://etherscan.io".to_string(),
            "ETH".to_string(),
            18,
            Duration::from_secs(12),
            U256::from(2000000000),
            U256::from(3000000000),
        );
        chain.add_metadata("key1".to_string(), "value1".to_string());
        assert_eq!(chain.get_metadata("key1"), Some(&"value1".to_string()));
        chain.remove_metadata("key1");
        assert_eq!(chain.get_metadata("key1"), None);
    }
} 
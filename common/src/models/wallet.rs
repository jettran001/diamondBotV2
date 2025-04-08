// External imports
use anyhow::{Result, Context};

// Standard library imports
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
    collections::HashMap,
};

// Third party imports
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use ethers::{
    core::types::{Address, U256},
    signers::{LocalWallet, Signer},
};

/// Loại ví
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WalletType {
    /// Ví thường
    Standard,
    /// Ví đa chữ ký
    MultiSig,
    /// Ví phần cứng
    Hardware,
}

impl Display for WalletType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            WalletType::Standard => write!(f, "Standard"),
            WalletType::MultiSig => write!(f, "MultiSig"),
            WalletType::Hardware => write!(f, "Hardware"),
        }
    }
}

/// Trạng thái ví
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WalletStatus {
    /// Đang hoạt động
    Active,
    /// Đã khóa
    Locked,
    /// Đã xóa
    Deleted,
}

impl Display for WalletStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            WalletStatus::Active => write!(f, "Active"),
            WalletStatus::Locked => write!(f, "Locked"),
            WalletStatus::Deleted => write!(f, "Deleted"),
        }
    }
}

/// Thông tin ví
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    /// ID ví
    pub id: Uuid,
    /// Địa chỉ ví
    pub address: Address,
    /// Loại ví
    pub wallet_type: WalletType,
    /// Trạng thái
    pub status: WalletStatus,
    /// Số dư
    pub balance: U256,
    /// Nonce hiện tại
    pub nonce: U256,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Private key đã mã hóa (nếu có)
    #[serde(skip)]
    pub encrypted_private_key: Option<String>,
    /// Tên ví
    pub name: String,
    /// Mô tả ví
    pub description: Option<String>,
    /// Danh sách token
    pub tokens: HashMap<String, U256>,
    /// Danh sách hợp đồng
    pub contracts: Vec<Address>,
    /// Metadata
    pub metadata: HashMap<String, String>,
    /// Chain ID
    pub chain_id: u64,
    /// Gas price
    pub gas_price: U256,
    /// Gas limit
    pub gas_limit: U256,
}

impl Wallet {
    /// Tạo ví mới
    pub fn new(
        address: Address,
        wallet_type: WalletType,
        name: String,
        description: Option<String>,
        chain_id: u64,
        gas_price: U256,
        gas_limit: U256,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            address,
            wallet_type,
            status: WalletStatus::Active,
            balance: U256::zero(),
            nonce: U256::zero(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            encrypted_private_key: None,
            name,
            description,
            tokens: HashMap::new(),
            contracts: Vec::new(),
            metadata: HashMap::new(),
            chain_id,
            gas_price,
            gas_limit,
        }
    }

    /// Tạo ví mới từ private key
    pub fn from_private_key(
        private_key: &str,
        name: String,
        description: Option<String>,
        chain_id: u64,
        gas_price: U256,
        gas_limit: U256,
    ) -> Result<Self> {
        let wallet = LocalWallet::from_str(private_key)
            .context("Failed to create wallet from private key")?;
        let address = wallet.address();
        Ok(Self::new(
            address,
            WalletType::Standard,
            name,
            description,
            chain_id,
            gas_price,
            gas_limit,
        ))
    }

    /// Cập nhật số dư
    pub fn update_balance(&mut self, balance: U256) {
        self.balance = balance;
        self.updated_at = Utc::now();
    }

    /// Cập nhật nonce
    pub fn update_nonce(&mut self, nonce: U256) {
        self.nonce = nonce;
        self.updated_at = Utc::now();
    }

    /// Khóa ví
    pub fn lock(&mut self) {
        self.status = WalletStatus::Locked;
        self.updated_at = Utc::now();
    }

    /// Mở khóa ví
    pub fn unlock(&mut self) {
        self.status = WalletStatus::Active;
        self.updated_at = Utc::now();
    }

    /// Xóa ví
    pub fn delete(&mut self) {
        self.status = WalletStatus::Deleted;
        self.updated_at = Utc::now();
    }

    /// Kiểm tra ví có đang hoạt động
    pub fn is_active(&self) -> bool {
        self.status == WalletStatus::Active
    }

    /// Kiểm tra ví có đủ số dư
    pub fn has_sufficient_balance(&self, amount: U256) -> bool {
        self.balance >= amount
    }

    /// Thêm token
    pub fn add_token(&mut self, symbol: String, balance: U256) {
        self.tokens.insert(symbol, balance);
        self.updated_at = Utc::now();
    }

    /// Xóa token
    pub fn remove_token(&mut self, symbol: &str) {
        self.tokens.remove(symbol);
        self.updated_at = Utc::now();
    }

    /// Thêm hợp đồng
    pub fn add_contract(&mut self, contract: Address) {
        self.contracts.push(contract);
        self.updated_at = Utc::now();
    }

    /// Xóa hợp đồng
    pub fn remove_contract(&mut self, contract: Address) {
        self.contracts.retain(|&c| c != contract);
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

    /// Cập nhật gas
    pub fn update_gas(&mut self, gas_price: U256, gas_limit: U256) {
        self.gas_price = gas_price;
        self.gas_limit = gas_limit;
        self.updated_at = Utc::now();
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test tạo ví mới
    #[test]
    fn test_new_wallet() {
        let address = Address::random();
        let wallet = Wallet::new(
            address,
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        assert_eq!(wallet.address, address);
        assert_eq!(wallet.wallet_type, WalletType::Standard);
        assert_eq!(wallet.status, WalletStatus::Active);
        assert_eq!(wallet.balance, U256::zero());
        assert_eq!(wallet.nonce, U256::zero());
        assert!(wallet.encrypted_private_key.is_none());
        assert_eq!(wallet.name, "Test Wallet");
        assert_eq!(wallet.description, Some("Test Description".to_string()));
        assert!(wallet.tokens.is_empty());
        assert!(wallet.contracts.is_empty());
        assert!(wallet.metadata.is_empty());
        assert_eq!(wallet.chain_id, 1);
        assert_eq!(wallet.gas_price, U256::from(1000000000));
        assert_eq!(wallet.gas_limit, U256::from(8000000));
    }

    /// Test tạo ví từ private key
    #[test]
    fn test_from_private_key() {
        let private_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let wallet = Wallet::from_private_key(
            private_key,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        ).unwrap();

        assert_eq!(wallet.wallet_type, WalletType::Standard);
        assert_eq!(wallet.status, WalletStatus::Active);
        assert_eq!(wallet.balance, U256::zero());
        assert_eq!(wallet.nonce, U256::zero());
        assert!(wallet.encrypted_private_key.is_none());
        assert_eq!(wallet.name, "Test Wallet");
        assert_eq!(wallet.description, Some("Test Description".to_string()));
        assert!(wallet.tokens.is_empty());
        assert!(wallet.contracts.is_empty());
        assert!(wallet.metadata.is_empty());
        assert_eq!(wallet.chain_id, 1);
        assert_eq!(wallet.gas_price, U256::from(1000000000));
        assert_eq!(wallet.gas_limit, U256::from(8000000));
    }

    /// Test cập nhật số dư và nonce
    #[test]
    fn test_update_balance_and_nonce() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        let balance = U256::from(1000);
        wallet.update_balance(balance);
        assert_eq!(wallet.balance, balance);

        let nonce = U256::from(5);
        wallet.update_nonce(nonce);
        assert_eq!(wallet.nonce, nonce);
    }

    /// Test trạng thái ví
    #[test]
    fn test_wallet_status() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        wallet.lock();
        assert_eq!(wallet.status, WalletStatus::Locked);
        assert!(!wallet.is_active());

        wallet.unlock();
        assert_eq!(wallet.status, WalletStatus::Active);
        assert!(wallet.is_active());

        wallet.delete();
        assert_eq!(wallet.status, WalletStatus::Deleted);
        assert!(!wallet.is_active());
    }

    /// Test kiểm tra số dư
    #[test]
    fn test_balance_check() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        wallet.update_balance(U256::from(1000));
        assert!(wallet.has_sufficient_balance(U256::from(500)));
        assert!(!wallet.has_sufficient_balance(U256::from(1500)));
    }

    /// Test token và hợp đồng
    #[test]
    fn test_tokens_and_contracts() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        wallet.add_token("ETH".to_string(), U256::from(1000));
        assert_eq!(wallet.tokens.get("ETH"), Some(&U256::from(1000)));

        wallet.remove_token("ETH");
        assert!(wallet.tokens.get("ETH").is_none());

        let contract = Address::random();
        wallet.add_contract(contract);
        assert!(wallet.contracts.contains(&contract));

        wallet.remove_contract(contract);
        assert!(!wallet.contracts.contains(&contract));
    }

    /// Test metadata
    #[test]
    fn test_metadata() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        wallet.add_metadata("key1".to_string(), "value1".to_string());
        assert_eq!(wallet.get_metadata("key1"), Some(&"value1".to_string()));

        wallet.remove_metadata("key1");
        assert!(wallet.get_metadata("key1").is_none());
    }

    /// Test gas
    #[test]
    fn test_gas() {
        let mut wallet = Wallet::new(
            Address::random(),
            WalletType::Standard,
            "Test Wallet".to_string(),
            Some("Test Description".to_string()),
            1,
            U256::from(1000000000),
            U256::from(8000000),
        );

        wallet.update_gas(U256::from(2000000000), U256::from(9000000));
        assert_eq!(wallet.gas_price, U256::from(2000000000));
        assert_eq!(wallet.gas_limit, U256::from(9000000));
    }
} 
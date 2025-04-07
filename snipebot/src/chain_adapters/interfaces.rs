use std::sync::Arc;
use std::fmt::Debug;
use ethers::types::{
    Address, BlockId, Bytes, Filter, Log, Transaction, TransactionReceipt,
    TransactionRequest, U256, H256
};
use async_trait::async_trait;
use anyhow::Result;
use ethers::providers::PendingTransaction;
use serde::{Serialize, Deserialize};
use thiserror::Error;

/// TokenDetails chứa thông tin về một token ERC20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDetails {
    /// Địa chỉ token
    pub address: Address,
    /// Tên token
    pub name: String,
    /// Symbol
    pub symbol: String,
    /// Số decimals
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U256,
}

/// ChainError định nghĩa các loại lỗi có thể xảy ra trong quá trình tương tác với blockchain
#[derive(Error, Debug, Clone)]
pub enum ChainError {
    /// Lỗi kết nối đến RPC
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Lỗi timeout khi chờ kết quả
    #[error("Request timed out after {0} ms")]
    TimeoutError(u64),
    
    /// Lỗi quá giới hạn tần suất của RPC
    #[error("Rate limit exceeded for endpoint: {0}")]
    RateLimitExceeded(String),
    
    /// Lỗi giao dịch (tổng quát)
    #[error("Transaction error: {0}")]
    TransactionError(String),
    
    /// Lỗi ước tính gas
    #[error("Gas estimation failed: {0}")]
    GasEstimationError(String),
    
    /// Lỗi nonce không hợp lệ
    #[error("Nonce error: {0}")]
    NonceError(String),
    
    /// Lỗi chainId không hợp lệ
    #[error("Invalid chain ID: {0}")]
    InvalidChainId(u64),
    
    /// Lỗi phương thức không được hỗ trợ
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),
    
    /// Lỗi gọi contract
    #[error("Contract call error: {0}")]
    ContractCallError(String),
    
    /// Lỗi liên quan đến token
    #[error("Token error: {0}")]
    TokenError(String),
    
    /// Lỗi decode ABI 
    #[error("ABI decode error: {0}")]
    ABIDecodeError(String),
    
    /// Lỗi circuit breaker được kích hoạt
    #[error("Circuit breaker triggered: {0}")]
    CircuitBreakerTriggered(String),
    
    /// Lỗi provider không khả dụng
    #[error("Provider not available")]
    ProviderNotAvailable,
    
    /// Lỗi wallet chưa được cấu hình
    #[error("Wallet not configured")]
    WalletNotConfigured,
    
    /// Lỗi người dùng hủy
    #[error("Cancelled by user")]
    UserCancelled,
    
    /// Lỗi chain không được hỗ trợ
    #[error("Unsupported chain: {0}")]
    UnsupportedChain(u64),
    
    /// Lỗi địa chỉ không hợp lệ
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    
    /// Lỗi contract
    #[error("Contract error: {0}")]
    ContractError(String),
    
    /// Lỗi phê duyệt token
    #[error("Approval error: {0}")]
    ApprovalError(String),
    
    /// Lỗi thiếu số dư
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),
    
    /// Lỗi chữ ký không hợp lệ
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    
    /// Lỗi không tìm thấy block
    #[error("Block not found: {0}")]
    BlockNotFound(String),
    
    /// Lỗi không tìm thấy receipt của giao dịch
    #[error("Transaction receipt not found: {0}")]
    ReceiptNotFound(H256),
    
    /// Lỗi gas không đủ
    #[error("Insufficient gas: {0}")]
    InsufficientGas(String),
    
    /// Lỗi giao dịch bị revert
    #[error("Transaction reverted: {0}")]
    Revert(String),
    
    /// Lỗi giá gas quá thấp
    #[error("Transaction underpriced")]
    Underpriced,
    
    /// Lỗi giá gas vượt quá giới hạn
    #[error("Gas price exceeds cap")]
    GasCap,
    
    /// Lỗi không tìm thấy giao dịch
    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),
    
    /// Lỗi không tìm thấy contract
    #[error("Contract not found: {0}")]
    ContractNotFound(String),
    
    /// Lỗi đã đạt số lần retry tối đa
    #[error("Maximum retry attempts reached: {0}")]
    MaxRetryReached(String),
    
    /// Lỗi không xác định
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl ChainError {
    /// Chuyển đổi từ anyhow::Error sang ChainError
    pub fn from_anyhow(err: anyhow::Error) -> Self {
        // Kiểm tra nếu error đã là ChainError
        if let Ok(chain_err) = err.downcast::<ChainError>() {
            return chain_err;
        }
        
        let err_string = err.to_string();
        
        // Kiểm tra các loại lỗi phổ biến từ ethers provider
        if err_string.contains("could not connect to server") || err_string.contains("connection error") {
            return ChainError::ConnectionError(err_string);
        } else if err_string.contains("rate limit") {
            return ChainError::RateLimitExceeded(err_string);
        } else if err_string.contains("nonce too low") || err_string.contains("nonce") {
            return ChainError::NonceError(err_string);
        } else if err_string.contains("gas required exceeds") || err_string.contains("gas price") {
            return ChainError::GasEstimationError(err_string);
        } else if err_string.contains("timeout") {
            return ChainError::TimeoutError(30000); // Giả định timeout sau 30s
        } else if err_string.contains("transaction") {
            return ChainError::TransactionError(err_string);
        } else if err_string.contains("contract call") || err_string.contains("execution reverted") {
            return ChainError::ContractCallError(err_string);
        } else if err_string.contains("token") {
            return ChainError::TokenError(err_string);
        } else if err_string.contains("decode") || err_string.contains("encode") {
            return ChainError::ABIDecodeError(err_string);
        } else if err_string.contains("unsupported chain") {
            // Cố gắng trích xuất ID chuỗi nếu có
            if let Some(chain_id) = extract_chain_id(&err_string) {
                return ChainError::UnsupportedChain(chain_id);
            }
            return ChainError::UnsupportedOperation(err_string);
        } else if err_string.contains("invalid address") {
            return ChainError::InvalidAddress(err_string);
        } else if err_string.contains("insufficient funds") || err_string.contains("insufficient balance") {
            return ChainError::InsufficientBalance(err_string);
        } else if err_string.contains("approval") {
            return ChainError::ApprovalError(err_string);
        } else if err_string.contains("signature") {
            return ChainError::InvalidSignature(err_string);
        } else if err_string.contains("block not found") {
            return ChainError::BlockNotFound(err_string);
        } else if err_string.contains("receipt not found") {
            // Cố gắng trích xuất hash giao dịch nếu có
            if let Some(hash_str) = err_string.split("receipt not found for ").nth(1) {
                if let Ok(hash) = hash_str.parse::<H256>() {
                    return ChainError::ReceiptNotFound(hash);
                }
            }
            return ChainError::TransactionError(err_string);
        } else if err_string.contains("reverted") {
            return ChainError::Revert(err_string);
        } else if err_string.contains("underpriced") {
            return ChainError::Underpriced;
        } else if err_string.contains("gas required exceeds") {
            return ChainError::InsufficientGas(err_string);
        } else if err_string.contains("gas cap") {
            return ChainError::GasCap;
        } else if err_string.contains("max retry") {
            return ChainError::MaxRetryReached(err_string);
        } else if err_string.contains("rpc") {
            return ChainError::ConnectionError(err_string);
        }
        
        ChainError::Unknown(err_string)
    }
    
    /// Kiểm tra xem lỗi có liên quan đến gas không
    pub fn is_gas_related(&self) -> bool {
        matches!(
            self,
            ChainError::GasEstimationError(_) | 
            ChainError::InsufficientGas(_) | 
            ChainError::Underpriced | 
            ChainError::GasCap
        )
    }
    
    /// Kiểm tra xem lỗi có phải là tạm thời và có thể thử lại không
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ChainError::ConnectionError(_) |
            ChainError::TimeoutError(_) |
            ChainError::RateLimitExceeded(_) |
            ChainError::ProviderNotAvailable |
            ChainError::Underpriced
        )
    }
    
    /// Lấy gợi ý khắc phục lỗi
    pub fn recovery_suggestion(&self) -> String {
        match self {
            Self::ConnectionError(_) => 
                "Kiểm tra kết nối mạng hoặc thử đổi RPC endpoint khác".to_string(),
            Self::TimeoutError(_) => 
                "Tăng thời gian chờ hoặc thử lại sau khi mạng ổn định hơn".to_string(),
            Self::RateLimitExceeded(_) => 
                "Giảm tần suất request hoặc sử dụng RPC endpoint khác".to_string(),
            Self::InsufficientGas(_) => 
                "Tăng gas limit hoặc gas price và thử lại".to_string(),
            Self::Revert(_) => 
                "Kiểm tra lại logic contract và tham số gọi".to_string(),
            Self::NonceError(_) => 
                "Đợi transaction trước hoàn tất hoặc reset nonce".to_string(),
            Self::Underpriced => 
                "Tăng gas price và thử lại".to_string(),
            Self::GasCap => 
                "Giảm gas price xuống dưới giới hạn của mạng".to_string(),
            Self::UnsupportedOperation(_) => 
                "Sử dụng tính năng thay thế hoặc chờ cập nhật".to_string(),
            Self::BlockNotFound(_) => 
                "Kiểm tra lại số block hoặc hash".to_string(),
            Self::TransactionNotFound(_) => 
                "Kiểm tra lại transaction hash hoặc đợi transaction được xác nhận".to_string(),
            Self::ContractNotFound(_) => 
                "Kiểm tra địa chỉ contract trên block explorer".to_string(),
            Self::CircuitBreakerTriggered(_) => 
                "Đợi circuit breaker reset hoặc sử dụng endpoint khác".to_string(),
            Self::MaxRetryReached(_) => 
                "Tăng số lần retry hoặc kiểm tra lỗi gốc".to_string(),
            Self::TransactionError(_) => 
                "Kiểm tra lại thông số transaction và định dạng dữ liệu".to_string(),
            _ => "Kiểm tra logs chi tiết và liên hệ hỗ trợ kỹ thuật".to_string(),
        }
    }
}

/// Hàm trích xuất chain_id từ một chuỗi lỗi
fn extract_chain_id(err_string: &str) -> Option<u64> {
    // Tìm các pattern như "chain id 1", "chain: 1", "chainId: 1"
    if let Some(id_str) = err_string
        .split("chain id ")
        .nth(1)
        .or_else(|| err_string.split("chain: ").nth(1))
        .or_else(|| err_string.split("chainId: ").nth(1))
    {
        if let Some(id_end) = id_str.find(|c: char| !c.is_digit(10)) {
            if let Ok(id) = id_str[..id_end].parse::<u64>() {
                return Some(id);
            }
        } else if let Ok(id) = id_str.parse::<u64>() {
            return Some(id);
        }
    }
    None
}

/// Thông tin chi tiết về gas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasInfo {
    /// Gas price (wei)
    pub gas_price: U256,
    /// Max fee per gas (cho EIP-1559)
    pub max_fee_per_gas: Option<U256>,
    /// Max priority fee per gas (cho EIP-1559)
    pub max_priority_fee_per_gas: Option<U256>,
    /// Gas limit
    pub gas_limit: U256,
    /// Có hỗ trợ EIP-1559 không
    pub eip1559_supported: bool,
}

impl GasInfo {
    /// Tạo mới với gas price đơn giản
    pub fn new_legacy(gas_price: U256, gas_limit: U256) -> Self {
        Self {
            gas_price,
            max_fee_per_gas: None,
            max_priority_fee_per_gas: None,
            gas_limit,
            eip1559_supported: false,
        }
    }
    
    /// Tạo mới với thông tin EIP-1559
    pub fn new_eip1559(
        max_fee_per_gas: U256,
        max_priority_fee_per_gas: U256,
        gas_limit: U256,
    ) -> Self {
        Self {
            gas_price: max_fee_per_gas, // Fallback for legacy
            max_fee_per_gas: Some(max_fee_per_gas),
            max_priority_fee_per_gas: Some(max_priority_fee_per_gas),
            gas_limit,
            eip1559_supported: true,
        }
    }
    
    /// Tăng gas price theo tỷ lệ phần trăm
    pub fn increase_by_percent(&self, percent: f64) -> Self {
        let multiplier = (100.0 + percent) / 100.0;
        
        if self.eip1559_supported {
            let new_max_fee = self.max_fee_per_gas
                .map(|fee| (fee.as_u128() as f64 * multiplier) as u128)
                .map(U256::from);
                
            let new_priority_fee = self.max_priority_fee_per_gas
                .map(|fee| (fee.as_u128() as f64 * multiplier) as u128)
                .map(U256::from);
                
            Self {
                gas_price: self.gas_price,
                max_fee_per_gas: new_max_fee,
                max_priority_fee_per_gas: new_priority_fee,
                gas_limit: self.gas_limit,
                eip1559_supported: true,
            }
        } else {
            let new_gas_price = (self.gas_price.as_u128() as f64 * multiplier) as u128;
            
            Self {
                gas_price: U256::from(new_gas_price),
                max_fee_per_gas: None,
                max_priority_fee_per_gas: None,
                gas_limit: self.gas_limit,
                eip1559_supported: false,
            }
        }
    }
    
    /// Áp dụng thông tin gas vào TransactionRequest
    pub fn apply_to_tx(&self, tx: &mut TransactionRequest) {
        tx.gas(self.gas_limit);
        
        if self.eip1559_supported {
            if let Some(max_fee) = self.max_fee_per_gas {
                tx.max_fee_per_gas(max_fee);
            }
            
            if let Some(priority_fee) = self.max_priority_fee_per_gas {
                tx.max_priority_fee_per_gas(priority_fee);
            }
        } else {
            tx.gas_price(self.gas_price);
        }
    }
}

/// Thông tin về loại transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// Transaction thông thường (legacy)
    Legacy = 0,
    /// Transaction theo EIP-2930
    EIP2930 = 1,
    /// Transaction theo EIP-1559
    EIP1559 = 2,
}

/// Thông tin block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    /// Block number
    pub number: u64,
    /// Block hash
    pub hash: String,
    /// Timestamp (Unix timestamp)
    pub timestamp: u64,
    /// Số transaction trong block
    pub transaction_count: usize,
    /// Gas limit của block
    pub gas_limit: U256,
    /// Gas đã sử dụng
    pub gas_used: U256,
    /// Base fee per gas (chỉ có với EIP-1559)
    pub base_fee_per_gas: Option<U256>,
}

/// Thông tin node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Phiên bản client
    pub client_version: String,
    /// Chain ID
    pub chain_id: u64,
    /// Là node đồng bộ hay không
    pub is_syncing: bool,
    /// Block number hiện tại
    pub current_block: u64,
    /// Block number cao nhất
    pub highest_block: u64,
}

/// Trạng thái của token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenState {
    /// Địa chỉ token
    pub address: Address,
    /// Số dư
    pub balance: U256,
    /// Allowance với contract
    pub allowance: U256,
    /// Giá (USD)
    pub price_usd: Option<f64>,
}

/// Interface chung cho tất cả chain adapter
#[async_trait]
pub trait ChainAdapter: Send + Sync + Debug {
    /// Lấy số block hiện tại
    async fn get_block_number(&self) -> Result<u64, ChainError>;
    
    /// Lấy gas price hiện tại
    async fn get_gas_price(&self) -> Result<U256, ChainError>;
    
    /// Lấy chain ID
    fn get_chain_id(&self) -> u64;
    
    /// Lấy loại adapter
    fn get_type(&self) -> String;
    
    /// Lấy thông tin block
    async fn get_block(&self, block_id: BlockId) -> Result<Option<BlockInfo>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy thông tin gas
    async fn get_gas_info(&self) -> Result<GasInfo, ChainError> {
        let gas_price = self.get_gas_price().await?;
        Ok(GasInfo::new_legacy(gas_price, U256::from(21000)))
    }
    
    /// Gửi transaction raw
    async fn send_raw_transaction(&self, tx_bytes: Bytes) -> Result<PendingTransaction<'static, ethers::providers::Provider<ethers::providers::Http>>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Gửi transaction
    async fn send_transaction(&self, tx: &TransactionRequest) -> Result<PendingTransaction<'static, ethers::providers::Provider<ethers::providers::Http>>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy transaction receipt
    async fn get_transaction_receipt(&self, tx_hash: ethers::types::H256) -> Result<Option<TransactionReceipt>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy transaction
    async fn get_transaction(&self, tx_hash: ethers::types::H256) -> Result<Option<Transaction>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy số dư ETH
    async fn get_eth_balance(&self, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy số dư token
    async fn get_token_balance(&self, token: Address, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy thông tin token
    async fn get_token_details(&self, token: Address) -> Result<TokenDetails, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy allowance
    async fn get_token_allowance(&self, token: Address, owner: Address, spender: Address) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy logs
    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy nonce
    async fn get_transaction_count(&self, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Ước tính gas
    async fn estimate_gas(&self, tx: &TransactionRequest) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Call (không thay đổi state)
    async fn call(&self, tx: &TransactionRequest, block: Option<BlockId>) -> Result<Bytes, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Chờ transaction được confirm
    async fn wait_for_transaction_receipt(
        &self,
        tx_hash: ethers::types::H256,
        confirmations: usize,
        timeout: std::time::Duration,
    ) -> Result<TransactionReceipt, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    /// Lấy thông tin node
    async fn get_node_info(&self) -> Result<NodeInfo, ChainError> {
        Err(ChainError::NotImplemented)
    }
}

/// Interface cho watcher
#[async_trait]
pub trait ChainWatcher: Send + Sync + Debug {
    /// Bắt đầu theo dõi address cụ thể
    async fn watch_address(&self, address: Address) -> Result<(), ChainError>;
    
    /// Dừng theo dõi address
    async fn unwatch_address(&self, address: Address) -> Result<(), ChainError>;
    
    /// Bắt đầu theo dõi event cụ thể
    async fn watch_event(&self, filter: Filter) -> Result<String, ChainError>;
    
    /// Dừng theo dõi event
    async fn unwatch_event(&self, id: &str) -> Result<(), ChainError>;
    
    /// Bắt đầu theo dõi transaction
    async fn watch_transaction(&self, tx_hash: ethers::types::H256) -> Result<(), ChainError>;
    
    /// Dừng theo dõi transaction
    async fn unwatch_transaction(&self, tx_hash: ethers::types::H256) -> Result<(), ChainError>;
}

/// Hàm trích xuất chain_id từ một chuỗi lỗi
fn extract_chain_id(err_string: &str) -> Option<u64> {
    // Tìm các pattern như "chain id 1", "chain: 1", "chainId: 1"
    if let Some(id_str) = err_string
        .split("chain id ")
        .nth(1)
        .or_else(|| err_string.split("chain: ").nth(1))
        .or_else(|| err_string.split("chainId: ").nth(1))
    {
        if let Some(id_end) = id_str.find(|c: char| !c.is_digit(10)) {
            if let Ok(id) = id_str[..id_end].parse::<u64>() {
                return Some(id);
            }
        } else if let Ok(id) = id_str.parse::<u64>() {
            return Some(id);
        }
    }
    None
} 
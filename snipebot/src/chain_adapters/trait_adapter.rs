use async_trait::async_trait;
use ethers::types::{Address, U256, TransactionRequest, TransactionReceipt};
use ethers::{
    providers::{Provider, Http, Middleware},
    signers::LocalWallet,
    abi::Token,
};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use crate::error::TransactionError;
use std::fmt::Debug;
use ethers::core::types::BlockNumber;
use tokio::task::JoinHandle;
use std::sync::{Arc, RwLock};

/// Trait định nghĩa các chức năng không async của một blockchain adapter
pub trait ChainAdapter: Send + Sync + Debug + 'static {
    /// Lấy config của chain
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig;
    
    /// Lấy provider
    fn get_provider(&self) -> &Provider<Http>;
    
    /// Lấy ví (nếu có)
    fn get_wallet(&self) -> Option<&LocalWallet>;
    
    /// Đặt ví 
    fn set_wallet(&mut self, wallet: LocalWallet);
    
    /// Lấy gas optimizer (nếu có)
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer>;
    
    /// Decode input của router
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>>;
    
    /// Lấy path từ ETH -> Token
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>>;
    
    /// Lấy path từ Token -> ETH
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>>;
    
    // Các helper không cần async
    fn get_chain_id(&self) -> u64 {
        self.get_config().chain_id
    }
    
    fn get_chain_name(&self) -> &str {
        &self.get_config().name
    }
    
    fn get_explorer_url(&self) -> &str {
        &self.get_config().explorer_url
    }
    
    fn get_transaction_url(&self, tx_hash: &str) -> String {
        format!("{}/tx/{}", self.get_explorer_url(), tx_hash)
    }
    
    fn get_address_url(&self, address: &str) -> String {
        format!("{}/address/{}", self.get_explorer_url(), address)
    }
    
    fn get_token_url(&self, token_address: &str) -> String {
        format!("{}/token/{}", self.get_explorer_url(), token_address)
    }
}

/// Trait cho các chức năng async của một blockchain adapter
#[async_trait]
pub trait AsyncChainAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy số dư native token
    async fn get_native_balance(&self, address: &str) -> Result<U256>;
    
    /// Lấy số dư token
    async fn get_token_balance(&self, token_address: &str, wallet_address: &str) -> Result<U256>;
    
    /// Phê duyệt token
    async fn approve_token(&self, token_address: &str, spender_address: &str, amount: U256) -> Result<Option<TransactionReceipt>>;
    
    /// Swap ETH -> Token
    async fn swap_exact_eth_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>>;
    
    /// Swap Token -> ETH
    async fn swap_exact_tokens_for_eth(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>>;
    
    /// Lấy số lượng token khi swap
    async fn get_amounts_out(&self, amount_in: U256, path: Vec<Address>) -> Result<Vec<U256>>;
    
    /// Lấy thông tin cặp token
    async fn get_pair(&self, token_a: &str, token_b: &str) -> Result<Option<String>>;
    
    /// Gửi giao dịch với retry
    async fn send_transaction_with_retry(
        &self,
        tx: ethers::types::transaction::eip2718::TypedTransaction,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
        operation_name: &str,
    ) -> Result<TransactionReceipt, TransactionError>;
    
    /// Lấy provider với rotation
    async fn get_provider_with_rotation(&self) -> Result<Provider<Http>>;
    
    /// Swap ETH -> Token với slippage
    async fn swap_eth_for_tokens(
        &self,
        amount: U256,
        token_address: &str,
        slippage: f64
    ) -> Result<String>;
    
    /// Swap Token -> ETH với slippage  
    async fn swap_tokens_for_eth(
        &self,
        token_address: &str,
        amount: U256,
        slippage: f64
    ) -> Result<String>;
    
    /// Tạo Flashbots bundle
    async fn create_flashbots_bundle(&self, txs: Vec<TransactionRequest>) -> Result<()>;
}

// Tạo trait cho các chức năng liên quan đến callback để xử lý riêng
#[async_trait]
pub trait ChainWatcher: Send + Sync + Debug {
    /// Theo dõi giao dịch đang chờ xử lý
    async fn watch_pending_transactions<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(Transaction) + Send + Sync + 'static;
    
    /// Theo dõi các giao dịch liên quan đến token cụ thể
    async fn watch_token_transactions<F>(&self, token_address: &str, callback: F) -> Result<JoinHandle<()>>
    where
        F: Fn(Transaction) + Send + Sync + 'static;
    
    /// Theo dõi cơ hội sandwich attack
    async fn watch_for_sandwich_opportunities<F>(&self, token_address: &str, min_amount: U256, callback: F) -> Result<JoinHandle<()>>
    where
        F: Fn(Transaction, U256) + Send + Sync + 'static;
}

/// Enum bọc cho ChainWatcher để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum ChainWatcherEnum {
    Default(Arc<dyn ChainAdapter + Send + Sync>),
    Custom(Arc<dyn ChainWatcher + Send + Sync>),
}

#[async_trait]
impl ChainWatcher for ChainWatcherEnum {
    /// Theo dõi giao dịch đang chờ xử lý
    async fn watch_pending_transactions<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(Transaction) + Send + Sync + 'static
    {
        match self {
            ChainWatcherEnum::Default(adapter) => {
                // Cast adapter to ChainWatcher if possible, otherwise error
                if let Some(watcher) = adapter.as_any().downcast_ref::<dyn ChainWatcher>() {
                    watcher.watch_pending_transactions(callback).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ChainWatcher"))
                }
            },
            ChainWatcherEnum::Custom(watcher) => watcher.watch_pending_transactions(callback).await,
        }
    }
    
    /// Theo dõi các giao dịch liên quan đến token cụ thể
    async fn watch_token_transactions<F>(&self, token_address: &str, callback: F) -> Result<JoinHandle<()>>
    where
        F: Fn(Transaction) + Send + Sync + 'static
    {
        match self {
            ChainWatcherEnum::Default(adapter) => {
                if let Some(watcher) = adapter.as_any().downcast_ref::<dyn ChainWatcher>() {
                    watcher.watch_token_transactions(token_address, callback).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ChainWatcher"))
                }
            },
            ChainWatcherEnum::Custom(watcher) => watcher.watch_token_transactions(token_address, callback).await,
        }
    }
    
    /// Theo dõi cơ hội sandwich attack
    async fn watch_for_sandwich_opportunities<F>(&self, token_address: &str, min_amount: U256, callback: F) -> Result<JoinHandle<()>>
    where
        F: Fn(Transaction, U256) + Send + Sync + 'static
    {
        match self {
            ChainWatcherEnum::Default(adapter) => {
                if let Some(watcher) = adapter.as_any().downcast_ref::<dyn ChainWatcher>() {
                    watcher.watch_for_sandwich_opportunities(token_address, min_amount, callback).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ChainWatcher"))
                }
            },
            ChainWatcherEnum::Custom(watcher) => watcher.watch_for_sandwich_opportunities(token_address, min_amount, callback).await,
        }
    }
} 
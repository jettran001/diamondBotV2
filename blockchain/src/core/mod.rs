// External imports
use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    types::{Address, U256, Transaction, Filter, H256, BlockId, BlockNumber, Bytes},
    abi::{self, Token, Detokenize},
    contract::Contract,
};

// Standard library imports
use std::{
    collections::HashMap,
    sync::Arc,
    str::FromStr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    marker::Send,
};

// Third party imports
use anyhow::{Result, anyhow, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::sleep;
use serde::{Serialize, Deserialize};

// Internal imports
use crate::abi;
use common::cache::{Cache, CacheEntry, BasicCache, CacheConfig};

pub mod blockchain_monitor;
pub mod token_info;
pub mod transaction;
pub mod contract_manager;

pub use blockchain_monitor::*;
pub use token_info::*;
pub use transaction::*;
pub use contract_manager::*;

/// Cấu trúc thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Địa chỉ của token
    pub address: String,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số chữ số thập phân
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U256,
}

/// Tham số cấu hình blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainParams {
    /// Chain ID
    pub chain_id: u64,
    /// URL endpoint RPC
    pub rpc_url: String,
    /// URL explorer
    pub explorer_url: String,
}

/// Trait định nghĩa các phương thức cơ bản tương tác với blockchain
#[async_trait]
pub trait BlockchainService: Send + Sync + 'static {
    /// Lấy block mới nhất
    async fn get_latest_block(&self) -> Result<u64>;
    
    /// Lấy thông tin token từ địa chỉ
    async fn get_token_info(&self, token_address: &str) -> Result<TokenInfo>;
    
    /// Lấy biên lai giao dịch
    async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>>;
    
    /// Đợi giao dịch được xác nhận
    async fn wait_for_transaction(&self, tx_hash: &str, timeout_secs: u64) -> Result<Option<TransactionReceipt>>;
    
    /// Lấy giá gas hiện tại
    async fn get_gas_price(&self) -> Result<U256>;
    
    /// Lấy số dư ETH
    async fn get_eth_balance(&self, address: &str) -> Result<U256>;
    
    /// Gọi hàm của contract
    async fn call_contract_function<T: Detokenize + 'static>(
        &self,
        contract_address: &str, 
        function_name: &str, 
        function_params: Vec<Token>,
        abi_path: &str
    ) -> Result<T>;
}

// Import module con
mod blockchain_monitor {
    use super::*;
    
    /// Đối tượng giám sát blockchain
    pub struct BlockchainMonitor {
        /// Provider kết nối blockchain
        provider: Provider<Http>,
        /// Cấu hình
        config: BlockchainParams,
        /// Block mới nhất được truy vấn
        last_block: u64,
        /// Cache
        cache: Arc<BasicCache>,
    }
    
    impl BlockchainMonitor {
        /// Tạo một blockchain monitor mới
        pub fn new(params: BlockchainParams) -> Result<Self> {
            let provider = Provider::<Http>::try_from(&params.rpc_url)
                .with_context(|| format!("Không thể kết nối đến RPC URL: {}", params.rpc_url))?;
            
            let cache_config = CacheConfig {
                default_ttl: 30, // 30 giây mặc định
                max_size: 1000,  // Tối đa 1000 mục
                cleanup_interval: 300, // Dọn dẹp mỗi 5 phút
            };
            
            Ok(Self {
                provider,
                config: params,
                last_block: 0,
                cache: Arc::new(BasicCache::new(Some(cache_config))),
            })
        }
        
        /// Thiết lập cấu hình cache
        pub fn with_cache_config(&mut self, config: CacheConfig) -> &mut Self {
            self.cache = Arc::new(BasicCache::new(Some(config)));
            self
        }
        
        /// Thiết lập cache có sẵn
        pub fn with_cache(&mut self, cache: Arc<BasicCache>) -> &mut Self {
            self.cache = cache;
            self
        }
        
        /// Lấy provider
        pub fn get_provider(&self) -> Arc<Provider<Http>> {
            Arc::new(self.provider.clone())
        }
    }
    
    #[async_trait]
    impl BlockchainService for BlockchainMonitor {
        async fn get_latest_block(&self) -> Result<u64> {
            let block_number = self.provider.get_block_number().await
                .with_context(|| "Không thể lấy số block mới nhất")?;
            Ok(block_number.as_u64())
        }
        
        async fn get_token_info(&self, token_address: &str) -> Result<TokenInfo> {
            // Check cache first
            let cache_key = format!("token_info_{}", token_address);
            if let Ok(Some(token_info)) = self.cache.get_from_cache::<TokenInfo>(&cache_key).await {
                return Ok(token_info);
            }
            
            // Validate token address format
            let address = Address::from_str(token_address)
                .with_context(|| format!("Địa chỉ token không hợp lệ: {}", token_address))?;
            
            // Lấy ABI đã chuẩn hóa từ module abi
            let abi = abi::erc20_token::ERC20_ABI.clone();
            
            // Tạo đối tượng contract
            let token_contract = Contract::new(address, abi.clone(), Arc::new(self.provider.clone()));
            
            // Khai báo biến mặc định trong trường hợp gọi contract thất bại
            let mut name = "Unknown".to_string();
            let mut symbol = "UNK".to_string();
            let mut decimals: u8 = 18;
            let mut total_supply = U256::zero();
            
            // Gọi phương thức name() với xử lý lỗi
            match token_contract.method::<_, String>("name", ())
                    .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm name(): {}", e))?
                    .call().await {
                Ok(fetched_name) => name = fetched_name,
                Err(e) => {
                    warn!("Không thể lấy tên của token {}: {}", token_address, e);
                }
            }
            
            // Gọi phương thức symbol() với xử lý lỗi
            match token_contract.method::<_, String>("symbol", ())
                    .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm symbol(): {}", e))?
                    .call().await {
                Ok(fetched_symbol) => symbol = fetched_symbol,
                Err(e) => {
                    warn!("Không thể lấy ký hiệu của token {}: {}", token_address, e);
                }
            }
            
            // Gọi phương thức decimals() với xử lý lỗi
            match token_contract.method::<_, u8>("decimals", ())
                    .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm decimals(): {}", e))?
                    .call().await {
                Ok(fetched_decimals) => decimals = fetched_decimals,
                Err(e) => {
                    warn!("Không thể lấy decimals của token {}: {}", token_address, e);
                }
            }
            
            // Gọi phương thức totalSupply() với xử lý lỗi
            match token_contract.method::<_, U256>("totalSupply", ())
                    .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm totalSupply(): {}", e))?
                    .call().await {
                Ok(fetched_supply) => total_supply = fetched_supply,
                Err(e) => {
                    warn!("Không thể lấy tổng cung của token {}: {}", token_address, e);
                }
            }
            
            let token_info = TokenInfo {
                address: token_address.to_string(),
                name,
                symbol,
                decimals,
                total_supply,
            };
            
            // Update cache
            let _ = self.cache.store_in_cache(&cache_key, &token_info, 300).await; // Cache 5 phút
            
            Ok(token_info)
        }
        
        async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>> {
            let tx_hash = H256::from_str(tx_hash)
                .with_context(|| format!("Định dạng transaction hash không hợp lệ: {}", tx_hash))?;
            
            let receipt = self.provider.get_transaction_receipt(tx_hash).await
                .with_context(|| format!("Lỗi khi lấy transaction receipt cho hash {}", tx_hash))?;
            
            Ok(receipt)
        }
        
        async fn wait_for_transaction(&self, tx_hash: &str, timeout_secs: u64) -> Result<Option<TransactionReceipt>> {
            // Validate tx_hash input
            let tx_hash = H256::from_str(tx_hash)
                .with_context(|| format!("Định dạng transaction hash không hợp lệ: {}", tx_hash))?;
            
            // Kiểm tra giá trị timeout_secs hợp lý
            let timeout_duration = if timeout_secs == 0 || timeout_secs > 3600 {
                warn!("Giá trị timeout không hợp lệ ({}), sử dụng giá trị mặc định 60 giây", timeout_secs);
                Duration::from_secs(60) // Giá trị mặc định nếu timeout_secs không hợp lệ
            } else {
                Duration::from_secs(timeout_secs)
            };
            
            let pending_tx = PendingTransaction::new(tx_hash, &self.provider);
            
            // Sử dụng match để xử lý rõ ràng các trường hợp lỗi
            match tokio::time::timeout(
                timeout_duration,
                pending_tx.confirmations(1)
            ).await {
                Ok(result) => match result {
                    Ok(receipt) => {
                        // Kiểm tra xem giao dịch có thành công không (status = 1)
                        if let Some(receipt_with_status) = receipt.as_ref() {
                            if let Some(status) = receipt_with_status.status {
                                if status.as_u64() == 0 {
                                    warn!("Giao dịch {} đã được xác nhận nhưng thất bại (status=0)", tx_hash);
                                } else {
                                    info!("Giao dịch {} đã được xác nhận thành công", tx_hash);
                                }
                            }
                        }
                        Ok(receipt)
                    },
                    Err(e) => {
                        let error_msg = format!("Lỗi khi đợi giao dịch: {}", e);
                        error!("{}", error_msg);
                        Err(anyhow!(error_msg))
                    }
                },
                Err(_) => {
                    let error_msg = format!("Giao dịch không được xác nhận sau {} giây", timeout_secs);
                    warn!("{}", error_msg);
                    Err(anyhow!(error_msg))
                }
            }
        }
        
        async fn get_gas_price(&self) -> Result<U256> {
            self.provider.get_gas_price().await
                .with_context(|| "Không thể lấy giá gas hiện tại")
        }
        
        async fn get_eth_balance(&self, address: &str) -> Result<U256> {
            let address = Address::from_str(address)
                .with_context(|| format!("Địa chỉ không hợp lệ: {}", address))?;
            
            self.provider.get_balance(address, None).await
                .with_context(|| format!("Không thể lấy số dư ETH cho địa chỉ {}", address))
        }
        
        async fn call_contract_function<T: Detokenize + 'static>(
            &self,
            contract_address: &str, 
            function_name: &str, 
            function_params: Vec<Token>,
            abi_path: &str
        ) -> Result<T> {
            let contract_address = Address::from_str(contract_address)
                .with_context(|| format!("Địa chỉ contract không hợp lệ: {}", contract_address))?;
            
            // Sử dụng abi module để lấy ABI
            let abi = match abi_path {
                "erc20" => abi::erc20_token::ERC20_ABI.clone(),
                "factory" => abi::uniswap_v2_factory::UNIV2FACTORY_ABI.clone(),
                "pair" => abi::uniswap_v2_pair::UNIV2PAIR_ABI.clone(),
                "router" => abi::uniswap_v2_router::UNIV2ROUTER_ABI.clone(),
                _ => return Err(anyhow!("ABI không hỗ trợ: {}", abi_path))
            };
            
            let contract = Contract::new(
                contract_address,
                abi,
                Arc::new(self.provider.clone())
            );
            
            let result = contract
                .method::<_, T>(function_name, function_params)?
                .call()
                .await
                .with_context(|| format!("Lỗi khi gọi hàm {} của contract {}", function_name, contract_address))?;
            
            Ok(result)
        }
    }
}

/// Tạo một blockchain service instance mới
pub fn create_blockchain_service(params: BlockchainParams) -> Result<impl BlockchainService> {
    BlockchainMonitor::new(params)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_blockchain_monitor() {
        // Test sẽ được triển khai sau
        assert!(true);
    }
} 
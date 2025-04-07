use ethers::types::{Address, U256};
use std::time::Instant;
use crate::chain_adapters::interfaces::{GasInfo, TokenDetails, BlockInfo, NodeInfo};
use crate::chain_adapters::connection_pool::{get_or_create_pool, ProviderGuard};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tracing::{info, warn, error};
use crate::chain_adapters::{ChainAdapter, ChainError};
use std::collections::HashMap;
use async_trait::async_trait;
use tokio::sync::RwLock;
use ethers::types::{
    BlockId, Bytes, Filter, Log, Transaction, H256,
};
use ethers::providers::PendingTransaction;
use serde::{Serialize, Deserialize};

use crate::chain_adapters::{
    base::ChainConfig,
    retry_policy::{RetryPolicy, RetryPolicyEnum, RetryContext, create_default_retry_policy},
};

/// Adapter cho blockchain không phải là EVM (cho mục đích minh họa)
/// Trong thực tế, cần phải triển khai cụ thể cho từng loại blockchain
pub struct NonEVMAdapter {
    /// Định danh blockchain
    chain_id: u64,
    /// Tên blockchain
    chain_name: String,
    /// Thông tin cấu hình
    config: ChainConfig,
    /// Chính sách thử lại
    retry_policy: RetryPolicyEnum,
    /// Cache cho các kết quả truy vấn
    cache: RwLock<HashMap<String, (Instant, serde_json::Value)>>,
}

impl NonEVMAdapter {
    /// Tạo mới một NonEVMAdapter
    pub fn new(
        chain_id: u64,
        chain_name: String,
        config: ChainConfig,
        retry_policy: Option<RetryPolicyEnum>,
    ) -> Arc<Self> {
        Arc::new(Self {
            chain_id,
            chain_name,
            config,
            retry_policy: retry_policy.unwrap_or_else(create_default_retry_policy),
            cache: RwLock::new(HashMap::new()),
        })
    }
    
    /// Kiểm tra xem cache có quá hạn không
    async fn is_cache_valid(&self, key: &str, ttl_secs: u64) -> bool {
        let cache = self.cache.read().await;
        if let Some((timestamp, _)) = cache.get(key) {
            let elapsed = timestamp.elapsed();
            return elapsed.as_secs() < ttl_secs;
        }
        false
    }
    
    /// Lấy giá trị từ cache
    async fn get_from_cache<T>(&self, key: &str) -> Option<T> 
    where 
        T: for<'de> Deserialize<'de>
    {
        let cache = self.cache.read().await;
        if let Some((_, value)) = cache.get(key) {
            return serde_json::from_value(value.clone()).ok();
        }
        None
    }
    
    /// Lưu giá trị vào cache
    async fn set_cache<T>(&self, key: String, value: T) -> Result<()>
    where
        T: Serialize,
    {
        let mut cache = self.cache.write().await;
        let json_value = serde_json::to_value(value)?;
        cache.insert(key, (Instant::now(), json_value));
        Ok(())
    }
    
    /// Truy cập API của blockchain
    async fn api_request<T>(&self, endpoint: &str, method: &str, params: Vec<serde_json::Value>) -> Result<T>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        // Cache key từ endpoint và params
        let cache_key = format!("{}:{}:{}", endpoint, method, serde_json::to_string(&params)?);
        
        // Kiểm tra cache
        if self.is_cache_valid(&cache_key, 30).await {
            if let Some(cached) = self.get_from_cache(&cache_key).await {
                return Ok(cached);
            }
        }
        
        // Tạo JSON-RPC request
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });
        
        // Tạo context cho retry
        let context = RetryContext::new(
            method,
            endpoint,
            self.chain_id,
            None,
        );
        
        // Gửi request với retry
        let response = self.retry_policy.retry(
            || async {
                let client = reqwest::Client::new();
                let response = client.post(endpoint)
                    .json(&request)
                    .send()
                    .await?;
                
                if !response.status().is_success() {
                    return Err(anyhow!("API request failed with status: {}", response.status()));
                }
                
                let json: serde_json::Value = response.json().await?;
                
                if let Some(error) = json.get("error") {
                    return Err(anyhow!("API error: {}", error));
                }
                
                if let Some(result) = json.get("result") {
                    let typed_result: T = serde_json::from_value(result.clone())?;
                    return Ok(typed_result);
                }
                
                Err(anyhow!("Invalid API response format"))
            },
            &context
        ).await?;
        
        // Lưu vào cache
        self.set_cache(cache_key, &response).await?;
        
        Ok(response)
    }
}

#[async_trait]
impl ChainAdapter for NonEVMAdapter {
    async fn get_block_number(&self) -> Result<u64, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_block_number not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_gas_price(&self) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_gas_price not implemented for non-EVM chains".to_string()))
    }
    
    fn get_chain_id(&self) -> u64 {
        self.chain_id
    }
    
    fn get_type(&self) -> String {
        self.chain_name.clone()
    }
    
    async fn get_block(&self, _block_id: BlockId) -> Result<Option<BlockInfo>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_block not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_gas_info(&self) -> Result<GasInfo, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_gas_info not implemented for non-EVM chains".to_string()))
    }
    
    /// Gửi transaction raw
    async fn send_raw_transaction(&self, _tx_bytes: Bytes) -> Result<PendingTransaction<'static, Provider<Http>>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("send_raw_transaction not implemented for non-EVM chains".to_string()))
    }
    
    /// Gửi transaction
    async fn send_transaction(&self, _tx: &TransactionRequest) -> Result<PendingTransaction<'static, Provider<Http>>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("send_transaction not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_transaction_receipt(&self, _tx_hash: H256) -> Result<Option<TransactionReceipt>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_transaction_receipt not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_transaction(&self, _tx_hash: H256) -> Result<Option<Transaction>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_transaction not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_eth_balance(&self, _address: Address, _block: Option<BlockId>) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_eth_balance not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_token_balance(&self, _token: Address, _address: Address, _block: Option<BlockId>) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_token_balance not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_token_details(&self, _token: Address) -> Result<TokenDetails, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_token_details not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_token_allowance(&self, _token: Address, _owner: Address, _spender: Address) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_token_allowance not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_logs(&self, _filter: &Filter) -> Result<Vec<Log>, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_logs not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_transaction_count(&self, _address: Address, _block: Option<BlockId>) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_transaction_count not implemented for non-EVM chains".to_string()))
    }
    
    async fn estimate_gas(&self, _tx: &TransactionRequest) -> Result<U256, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("estimate_gas not implemented for non-EVM chains".to_string()))
    }
    
    async fn call(&self, _tx: &TransactionRequest, _block: Option<BlockId>) -> Result<Bytes, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("call not implemented for non-EVM chains".to_string()))
    }
    
    async fn wait_for_transaction_receipt(
        &self,
        _tx_hash: H256,
        _confirmations: usize,
        timeout: std::time::Duration,
    ) -> Result<TransactionReceipt, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("wait_for_transaction_receipt not implemented for non-EVM chains".to_string()))
    }
    
    async fn get_node_info(&self) -> Result<NodeInfo, ChainError> {
        // Triển khai theo giao thức của blockchain cụ thể
        Err(ChainError::NotImplemented("get_node_info not implemented for non-EVM chains".to_string()))
    }
}

impl std::fmt::Debug for NonEVMAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonEVMAdapter")
            .field("chain_id", &self.chain_id)
            .field("chain_name", &self.chain_name)
            .finish()
    }
} 
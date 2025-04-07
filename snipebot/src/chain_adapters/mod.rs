// External imports
use ethers::{
    types::{Address, U256, TransactionReceipt},
    providers::Middleware
};

// Standard library imports
use std::sync::Arc;
use std::str::FromStr;

// Third party imports
use anyhow::{Result, Context};
use tokio;
use tracing::{info, warn};

// Public modules
pub mod base;
pub mod trait_adapter;
pub mod retry_policy;
pub mod connection_pool;
pub mod chain_registry;
pub mod chain_adapter_impl;
pub mod non_evm_adapter;
pub mod wallet_integration;
pub mod nonce_manager;
pub mod error_handler;
pub mod interfaces;
pub mod chain_traits;
pub mod adapter_registry;
pub mod retry;
pub mod configs;

// Public re-exports
pub use {
    adapter_registry::{ADAPTER_REGISTRY, AdapterRegistry, get_chain_adapter, add_wallet_to_adapter},
    base::{BaseChainAdapter, ChainConfig},
    chain_adapter_impl::{ChainAdapterImpl, EVMChainAdapter},
    chain_registry::{get_adapter, get_chain_config, AdapterStatus, ChainAdapterInfo, factory},
    connection_pool::{ConnectionPool, get_pool, get_all_pools_info, EndpointInfo, EndpointStatus, ConnectionPoolConfig},
    interfaces::{ChainAdapter, ChainError, GasInfo, TokenDetails, BlockInfo, NodeInfo, TokenState},
    non_evm_adapter::NonEVMAdapter,
    retry_policy::{RetryPolicy, RetryContext, RetryStats, create_default_retry_policy},
    trait_adapter::{AsyncChainAdapter, ChainWatcher},
    wallet_integration::{WalletIntegration, TransactionManager, create_transaction_manager, get_wallet_balances, get_token_balances}
};

/// Khởi tạo tất cả các chain adapter được hỗ trợ
pub async fn init_adapters() -> Result<()> {
    // Lấy danh sách chain ID từ cấu hình
    let chain_configs = chain_registry::get_all_chain_configs();
    
    // Khởi tạo registry
    let registry = Arc::new(tokio::sync::RwLock::new(
        chain_registry::ChainRegistry::new()
    ));
    
    // Tạo và đăng ký từng adapter
    for config in chain_configs {
        if config.chain_type == "EVM" {
            // Tạo EVM adapter
            let adapter = EVMChainAdapter::from_config(config.clone()).await?;
            
            // Đăng ký vào registry
            let mut registry_guard = registry.write().await;
            registry_guard.register_adapter(config.chain_id, adapter.clone());
            
            info!("Đã khởi tạo EVM adapter cho chain {}: {}", config.chain_id, config.name);
        } else {
            // Xử lý các loại adapter khác khi cần
            warn!("Không hỗ trợ loại chain {}: {}", config.chain_type, config.name);
        }
    }
    
    // Lưu registry vào biến toàn cục
    *chain_registry::REGISTRY.write().await = registry.clone();
    
    Ok(())
}

/// Hàm xử lý transaction với logic send_raw và retry tích hợp
pub async fn send_transaction_with_retry(
    chain_id: u64,
    wallet_address: &str,
    to_address: &str,
    value: Option<U256>,
    data: Option<Vec<u8>>,
    gas_limit: Option<U256>,
    gas_price: Option<U256>
) -> Result<TransactionReceipt> {
    // Lấy registry
    let registry = chain_registry::REGISTRY.read().await.clone();
    
    // Lấy wallet storage
    let wallet_storage = wallet::secure_storage::WALLET_STORAGE.read().await.clone();
    
    // Tạo transaction manager
    let tx_manager = wallet_integration::create_transaction_manager(
        registry.clone(),
        wallet_storage
    ).await;
    
    // Gửi transaction
    let tx_hash = tx_manager.send_transaction(
        chain_id,
        wallet_address,
        to_address,
        value,
        data,
        gas_limit,
        gas_price,
        None // Tự động tính nonce
    ).await?;
    
    // Đợi transaction hoàn thành
    let receipt = tx_manager.wait_for_transaction(
        chain_id,
        tx_hash,
        2, // 2 confirmations
        120 // timeout 120 seconds
    ).await?;
    
    Ok(receipt)
}

/// Lấy số dư token và ETH cho một ví
pub async fn get_wallet_balances_with_tokens(
    wallet_address: &str,
    chain_ids: &[u64],
    token_addresses: &[(u64, &str)]
) -> Result<(Vec<(u64, U256)>, Vec<(u64, Address, U256)>)> {
    // Lấy registry
    let registry = chain_registry::REGISTRY.read().await.clone();
    
    // Lấy balance ETH
    let native_balances = wallet_integration::get_wallet_balances(
        registry.clone(),
        wallet_address,
        chain_ids
    ).await?;
    
    // Lấy balance token
    let token_balances = wallet_integration::get_token_balances(
        registry.clone(),
        wallet_address,
        token_addresses
    ).await?;
    
    Ok((native_balances, token_balances))
}

/// Lấy token details cho nhiều token
pub async fn get_token_details_batch(
    chain_id: u64,
    token_addresses: &[&str]
) -> Result<Vec<interfaces::TokenDetails>> {
    // Lấy adapter
    let adapter = chain_registry::get_adapter(chain_id)?;
    
    let mut results = Vec::new();
    
    // Lấy details cho từng token
    for &token_address in token_addresses {
        let address = Address::from_str(token_address)
            .context(format!("Invalid token address: {}", token_address))?;
            
        match adapter.get_token_details(address).await {
            Ok(details) => {
                results.push(details);
            },
            Err(e) => {
                warn!("Failed to get token details for {}: {:?}", token_address, e);
            }
        }
    }
    
    Ok(results)
}

/// Lấy gas price với rotation RPC nếu gặp lỗi
pub async fn get_gas_price_with_rotation(chain_id: u64) -> Result<U256> {
    // Lấy adapter
    let adapter = chain_registry::get_adapter(chain_id)?;
    
    // Lấy gas price
    let gas_price = adapter.get_gas_price().await?;
    
    Ok(gas_price)
}

/// Lấy thông tin sức khỏe tất cả RPC
pub async fn get_rpc_health() -> Result<serde_json::Value> {
    // Lấy thông tin tất cả pools
    let pools_info = connection_pool::get_all_pools_info();
    
    // Format thành JSON
    let json = serde_json::to_value(pools_info)?;
    
    Ok(json)
}

/// Lấy thống kê retry
pub async fn get_retry_stats() -> Result<serde_json::Value> {
    // Lấy thống kê
    let stats = retry_policy::get_retry_stats();
    
    // Format thành JSON
    let json = serde_json::to_value(stats)?;
    
    Ok(json)
}

/// Lấy tất cả chain đang được hỗ trợ
pub fn get_supported_chains() -> Vec<String> {
    chain_registry::get_supported_chains()
}

pub async fn get_token_balance(chain_id: u64, token_address: &str, wallet_address: &str) -> Result<U256> {
    let adapter = chain_registry::get_adapter(chain_id)?;
    if let Ok(async_adapter) = adapter.as_any().downcast_ref::<Box<dyn AsyncChainAdapter>>() {
        let balance = async_adapter.get_token_balance(token_address, wallet_address).await?;
        Ok(balance)
    } else {
        Err(anyhow::anyhow!("Không thể chuyển đổi adapter thành AsyncChainAdapter"))
    }
}

pub async fn get_native_balance(chain_id: u64, wallet_address: &str) -> Result<U256> {
    let adapter = chain_registry::get_adapter(chain_id)?;
    if let Ok(async_adapter) = adapter.as_any().downcast_ref::<Box<dyn AsyncChainAdapter>>() {
        let balance = async_adapter.get_native_balance(wallet_address).await?;
        Ok(balance)
    } else {
        Err(anyhow::anyhow!("Không thể chuyển đổi adapter thành AsyncChainAdapter"))
    }
}

pub async fn get_gas_price(chain_id: u64) -> Result<U256> {
    let adapter = chain_registry::get_adapter(chain_id)?;
    let gas_optimizer = adapter.get_gas_optimizer().ok_or_else(|| anyhow::anyhow!("Không tìm thấy gas optimizer"))?;
    gas_optimizer.get_optimal_gas_price(&*adapter).await
}

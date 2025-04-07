use ethers::types::{Address, U256};
use anyhow::{Result, anyhow};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use tracing::info;
use crate::chain_adapters::ChainAdapter;
use ethers::prelude::*;
use crate::chain_adapters::{
    ChainWatcher, 
    ChainError,
    retry_policy::{RetryPolicyEnum, create_default_retry_policy},
    connection_pool::{get_or_create_pool, ConnectionPoolConfig},
    base::ChainConfig,
    trait_adapter::{ChainAdapter as TraitChainAdapter, ChainWatcherEnum},
};
use once_cell::sync::Lazy;
use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use crate::chain_adapters::trait_adapter::RetryPolicy;
use crate::chain_adapters::retry_policy::RetryPolicyEnum;
use crate::chain_adapters::trait_adapter::{ChainWatcher, ChainWatcherEnum};

/// Trạng thái của một adapter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterStatus {
    /// Hoạt động bình thường
    Active,
    /// Đang gặp vấn đề nhưng vẫn hoạt động
    Degraded,
    /// Không hoạt động
    Inactive,
}

/// Thông tin về một chain adapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainAdapterInfo {
    /// Chain ID
    pub chain_id: u64,
    /// Tên chain
    pub name: String,
    /// Loại adapter
    pub adapter_type: String,
    /// Trạng thái hiện tại
    pub status: AdapterStatus,
    /// Thời gian từ lần cuối được khởi tạo (seconds)
    pub uptime: u64,
    /// Số lượng request thành công
    pub successful_requests: u64,
    /// Số lượng request thất bại
    pub failed_requests: u64,
    /// Số RPC endpoint khả dụng
    pub available_endpoints: usize,
    /// Số lượng request đang xử lý
    pub active_requests: usize,
    /// Thời gian phản hồi trung bình (ms)
    pub average_response_time: f64,
}

/// Cấu hình cho từng chain cụ thể
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain ID
    pub chain_id: u64,
    /// Tên chain
    pub name: String,
    /// Tên native token
    pub native_token_name: String,
    /// Symbol của native token
    pub native_token_symbol: String,
    /// Decimals của native token
    pub native_token_decimals: u8,
    /// Block time trung bình (seconds)
    pub avg_block_time: f64,
    /// Loại chain (EVM, Substrate, etc.)
    pub chain_type: String,
    /// Các RPC endpoint chính
    pub primary_rpc_urls: Vec<String>,
    /// Các RPC endpoint backup
    pub backup_rpc_urls: Vec<String>,
    /// Block explorer URL
    pub explorer_url: String,
    /// Router contracts
    pub router_contracts: HashMap<String, Address>,
    /// Factory contracts
    pub factory_contracts: HashMap<String, Address>,
    /// Cấu hình gas
    pub gas_config: GasConfig,
    /// Cấu hình connection pool
    pub connection_pool_config: Option<ConnectionPoolConfig>,
    /// Danh sách token phổ biến
    pub common_tokens: HashMap<String, TokenInfo>,
}

/// Thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Tên token
    pub name: String,
    /// Symbol token
    pub symbol: String,
    /// Contract address
    pub address: Address,
    /// Số decimals
    pub decimals: u8,
    /// Logo URL (nếu có)
    pub logo_url: Option<String>,
}

/// Cấu hình gas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    /// Gas limit mặc định
    pub default_gas_limit: u64,
    /// Gas price phổ biến (gwei)
    pub base_fee: f64,
    /// Priority fee phổ biến (gwei)
    pub priority_fee: f64,
    /// Giá gas an toàn để bảo đảm thành công (%)
    pub safe_gas_price_multiplier: f64,
    /// Giá gas tối đa có thể chấp nhận (gwei)
    pub max_gas_price: f64,
    /// Có hỗ trợ EIP-1559 không
    pub supports_eip1559: bool,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            default_gas_limit: 21000,
            base_fee: 20.0,
            priority_fee: 1.5,
            safe_gas_price_multiplier: 1.2,
            max_gas_price: 300.0,
            supports_eip1559: true,
        }
    }
}

/// Registry quản lý tất cả chain adapter
pub struct ChainRegistry {
    /// Map từ chain ID tới adapter
    adapters: HashMap<u64, Arc<dyn ChainAdapter>>,
    /// Map từ chain ID tới thông tin
    adapter_info: HashMap<u64, ChainAdapterInfo>,
    /// Map từ chain ID tới cấu hình
    chain_configs: HashMap<u64, ChainConfig>,
    /// Map từ chain ID tới watcher
    watchers: HashMap<u64, ChainWatcherEnum>,
    /// Policy retry mặc định
    default_retry_policy: RetryPolicyEnum,
}

impl ChainRegistry {
    /// Tạo mới registry
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            adapter_info: HashMap::new(),
            chain_configs: HashMap::new(),
            watchers: HashMap::new(),
            default_retry_policy: create_default_retry_policy(),
        }
    }
    
    /// Đăng ký adapter mới
    pub fn register_adapter(
        &mut self, 
        chain_id: u64, 
        adapter: Arc<dyn ChainAdapter>
    ) -> Result<()> {
        if self.adapters.contains_key(&chain_id) {
            return Err(anyhow!("Adapter for chain ID {} already registered", chain_id));
        }
        
        // Khởi tạo thông tin adapter
        let adapter_type = adapter.get_type();
        let chain_name = self.chain_configs
            .get(&chain_id)
            .map(|cfg| cfg.name.clone())
            .unwrap_or_else(|| format!("Chain-{}", chain_id));
            
        let info = ChainAdapterInfo {
            chain_id,
            name: chain_name,
            adapter_type,
            status: AdapterStatus::Active,
            uptime: 0,
            successful_requests: 0,
            failed_requests: 0,
            available_endpoints: 0,
            active_requests: 0,
            average_response_time: 0.0,
        };
        
        // Thêm vào registry
        self.adapters.insert(chain_id, adapter);
        self.adapter_info.insert(chain_id, info);
        
        info!("Registered chain adapter for chain ID {}: {}", chain_id, adapter_type);
        Ok(())
    }
    
    /// Đăng ký watcher mới
    pub fn register_watcher(
        &mut self, 
        chain_id: u64, 
        watcher: ChainWatcherEnum
    ) -> Result<()> {
        if self.watchers.contains_key(&chain_id) {
            return Err(anyhow!("Watcher for chain ID {} already registered", chain_id));
        }
        
        self.watchers.insert(chain_id, watcher);
        info!("Registered chain watcher for chain ID {}", chain_id);
        Ok(())
    }
    
    /// Thêm cấu hình chain mới
    pub fn add_chain_config(&mut self, config: ChainConfig) -> Result<()> {
        let chain_id = config.chain_id;
        
        // Cập nhật thông tin adapter nếu đã tồn tại
        if let Some(info) = self.adapter_info.get_mut(&chain_id) {
            info.name = config.name.clone();
        }
        
        self.chain_configs.insert(chain_id, config);
        info!("Added configuration for chain ID {}", chain_id);
        Ok(())
    }
    
    /// Lấy adapter theo chain ID
    pub fn get_adapter(&self, chain_id: u64) -> Result<Arc<dyn ChainAdapter>> {
        self.adapters.get(&chain_id)
            .cloned()
            .ok_or_else(|| anyhow!("No adapter registered for chain ID {}", chain_id))
    }
    
    /// Lấy watcher theo chain ID
    pub fn get_watcher(&self, chain_id: u64) -> Result<Option<ChainWatcherEnum>> {
        match self.watchers.get(&chain_id) {
            Some(watcher) => Ok(Some(watcher.clone())),
            None => Ok(None)
        }
    }
    
    /// Lấy cấu hình chain theo chain ID
    pub fn get_chain_config(&self, chain_id: u64) -> Result<&ChainConfig> {
        self.chain_configs.get(&chain_id)
            .ok_or_else(|| anyhow!("No configuration found for chain ID {}", chain_id))
    }
    
    /// Lấy thông tin tất cả adapter
    pub fn get_all_adapter_info(&self) -> Vec<&ChainAdapterInfo> {
        self.adapter_info.values().collect()
    }
    
    /// Lấy thông tin adapter theo chain ID
    pub fn get_adapter_info(&self, chain_id: u64) -> Result<&ChainAdapterInfo> {
        self.adapter_info.get(&chain_id)
            .ok_or_else(|| anyhow!("No adapter info found for chain ID {}", chain_id))
    }
    
    /// Cập nhật thông tin adapter
    pub fn update_adapter_info(&mut self, chain_id: u64, info: ChainAdapterInfo) -> Result<()> {
        if !self.adapter_info.contains_key(&chain_id) {
            return Err(anyhow!("No adapter info found for chain ID {}", chain_id));
        }
        
        self.adapter_info.insert(chain_id, info);
        Ok(())
    }
    
    /// Lấy danh sách chain ID đã đăng ký
    pub fn get_supported_chains(&self) -> Vec<u64> {
        self.adapters.keys().cloned().collect()
    }
    
    /// Gỡ bỏ adapter
    pub fn unregister_adapter(&mut self, chain_id: u64) -> Result<()> {
        if !self.adapters.contains_key(&chain_id) {
            return Err(anyhow!("No adapter registered for chain ID {}", chain_id));
        }
        
        self.adapters.remove(&chain_id);
        self.adapter_info.remove(&chain_id);
        
        info!("Unregistered chain adapter for chain ID {}", chain_id);
        Ok(())
    }
    
    /// Gỡ bỏ watcher
    pub fn unregister_watcher(&mut self, chain_id: u64) -> Result<()> {
        if !self.watchers.contains_key(&chain_id) {
            return Err(anyhow!("No watcher registered for chain ID {}", chain_id));
        }
        
        self.watchers.remove(&chain_id);
        info!("Unregistered chain watcher for chain ID {}", chain_id);
        Ok(())
    }
    
    /// Lấy retry policy mặc định
    pub fn get_default_retry_policy(&self) -> RetryPolicyEnum {
        self.default_retry_policy.clone()
    }
    
    /// Thay đổi retry policy mặc định
    pub fn set_default_retry_policy(&mut self, policy: RetryPolicyEnum) {
        self.default_retry_policy = policy;
    }
    
    /// Kiểm tra kết nối adapter
    pub async fn check_adapter_connection(&mut self, chain_id: u64) -> Result<()> {
        let adapter = self.get_adapter(chain_id)?;
        
        // Thử lấy block number để kiểm tra kết nối
        match adapter.get_block_number().await {
            Ok(_) => {
                // Cập nhật trạng thái
                if let Some(info) = self.adapter_info.get_mut(&chain_id) {
                    info.status = AdapterStatus::Active;
                    info.successful_requests += 1;
                }
                Ok(())
            },
            Err(e) => {
                // Cập nhật trạng thái
                if let Some(info) = self.adapter_info.get_mut(&chain_id) {
                    info.status = AdapterStatus::Degraded;
                    info.failed_requests += 1;
                }
                Err(anyhow!("Connection check failed: {}", e))
            }
        }
    }
    
    /// Khởi tạo adapter từ cấu hình
    pub async fn init_adapter_from_config(&mut self, chain_id: u64) -> Result<()> {
        let config = self.get_chain_config(chain_id)?.clone();
        
        // Tạo connection pool
        let pool = get_or_create_pool(
            chain_id,
            config.primary_rpc_urls.clone(),
            config.backup_rpc_urls.clone(),
            config.connection_pool_config.clone(),
        ).await?;
        
        // Tạo adapter (triển khai cụ thể cần được thêm vào)
        // Hiện tại chỉ là placeholder
        let adapter: Arc<dyn ChainAdapter> = Arc::new(PlaceholderAdapter {
            chain_id,
            name: config.name.clone(),
            config: config.clone(),
        });
        
        // Đăng ký adapter
        self.register_adapter(chain_id, adapter)?;
        
        Ok(())
    }
}

/// Registry toàn cục
static CHAIN_REGISTRY: Lazy<RwLock<ChainRegistry>> = Lazy::new(|| {
    RwLock::new(ChainRegistry::new())
});

/// Lấy registry toàn cục
pub fn get_registry() -> std::sync::RwLockReadGuard<'static, ChainRegistry> {
    CHAIN_REGISTRY.read().unwrap()
}

/// Lấy registry toàn cục để chỉnh sửa
pub fn get_registry_mut() -> std::sync::RwLockWriteGuard<'static, ChainRegistry> {
    CHAIN_REGISTRY.write().unwrap()
}

/// Lấy adapter dựa trên chain id
pub fn get_adapter(chain_id: u64) -> Result<Box<PlaceholderAdapter>> {
    // Kiểm tra chain ID
    if chain_id == 0 {
        return Err(anyhow!("Chain ID không hợp lệ"));
    }
    
    // Lấy cấu hình mặc định
    let config = get_chain_config(chain_id)?;
    
    // Tạo adapter
    let adapter = PlaceholderAdapter::new(config)?;
    
    Ok(Box::new(adapter))
}

/// Lấy cấu hình chain theo chain ID
pub fn get_chain_config(chain_id: u64) -> Result<ChainConfig> {
    get_registry().get_chain_config(chain_id).cloned()
}

/// Adapter placeholder
#[derive(Debug)]
struct PlaceholderAdapter {
    chain_id: u64,
    name: String,
    config: crate::chain_adapters::base::ChainConfig,
}

impl crate::chain_adapters::retry_policy::AsAny for PlaceholderAdapter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
impl ChainAdapter for PlaceholderAdapter {
    async fn get_block_number(&self) -> Result<u64, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    async fn get_gas_price(&self) -> Result<U256, ChainError> {
        Err(ChainError::NotImplemented)
    }
    
    fn get_chain_id(&self) -> u64 {
        self.chain_id
    }
    
    fn get_type(&self) -> String {
        "Placeholder".to_string()
    }
    
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        &self.config
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        unimplemented!("get_provider not implemented for PlaceholderAdapter")
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        None
    }
    
    fn set_wallet(&mut self, _wallet: LocalWallet) {
        // No-op for placeholder
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        None
    }
    
    fn decode_router_input(&self, _input: &[u8]) -> Result<Vec<Token>> {
        Err(anyhow!("decode_router_input not implemented for PlaceholderAdapter"))
    }
    
    fn get_native_to_token_path(&self, _token_address: &str) -> Result<Vec<Address>> {
        Err(anyhow!("get_native_to_token_path not implemented for PlaceholderAdapter"))
    }
    
    fn get_token_to_native_path(&self, _token_address: &str) -> Result<Vec<Address>> {
        Err(anyhow!("get_token_to_native_path not implemented for PlaceholderAdapter"))
    }
}

/// Triển khai factory để tạo các adapter
pub mod factory {
    use super::*;
    
    /// Tạo và đăng ký adapter mới
    pub async fn create_adapter(chain_id: u64) -> Result<Arc<dyn ChainAdapter>> {
        // Lấy cấu hình chain
        let config = get_registry().get_chain_config(chain_id)?.clone();
        
        // Kiểm tra và tạo adapter dựa trên chain_type
        match config.chain_type.as_str() {
            "evm" => {
                // Ví dụ code tạo adapter EVM
                // Thực tế cần phải implement chi tiết hơn
                unimplemented!("EVM adapter creation not implemented");
            },
            "non-evm" => {
                // Ví dụ code tạo adapter non-EVM
                unimplemented!("Non-EVM adapter creation not implemented");
            },
            _ => {
                return Err(anyhow!("Unsupported chain type: {}", config.chain_type));
            }
        }
    }
    
    /// Tạo và đăng ký watcher mới
    pub async fn create_watcher(chain_id: u64) -> Result<Option<ChainWatcherEnum>> {
        get_watcher(chain_id)
    }
}

/// Lấy watcher theo chain ID
pub fn get_watcher(chain_id: u64) -> Result<Option<ChainWatcherEnum>> {
    get_registry().get_watcher(chain_id)
} 
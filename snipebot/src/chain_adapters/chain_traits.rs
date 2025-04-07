use async_trait::async_trait;
use crate::chain_adapters::trait_adapter::ChainAdapter;
use anyhow::Result;
use ethers::types::{U256, TransactionReceipt};
use std::sync::Arc;
use ethers::types::Address;
use ethers::types::Token;
use ethers::types::LocalWallet;
use ethers::types::Provider;
use ethers::types::Http;

/// Trait bổ sung cho Ethereum
#[async_trait]
pub trait EthereumAdapter: ChainAdapter + Send + Sync + 'static {
    /// Gửi transaction lên mạng Ethereum
    async fn send_to_ethereum(&self, to: &str, value: U256) -> Result<Option<TransactionReceipt>>;
    
    /// Kiểm tra số lượng eth2 đã stake
    async fn get_eth2_staking_balance(&self, address: &str) -> Result<U256>;
}

/// Trait bổ sung cho BSC 
#[async_trait]
pub trait BSCAdapter: ChainAdapter + Send + Sync + 'static {
    /// Tương tác với PancakeSwap
    async fn swap_via_pancakeswap(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
    ) -> Result<Option<TransactionReceipt>>;
}

/// Trait bổ sung cho Avalanche
#[async_trait]
pub trait AvalancheAdapter: ChainAdapter + Send + Sync + 'static {
    /// Swap AVAX sử dụng tên hàm đặc thù của Avalanche
    async fn swap_exact_avax_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>>;
    
    /// Swap Token sang AVAX sử dụng tên hàm đặc thù của Avalanche
    async fn swap_exact_tokens_for_avax(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>>;
}

/// Trait bổ sung cho Optimism
#[async_trait]
pub trait OptimismAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy L1 gas price 
    async fn get_l1_gas_price(&self) -> Result<U256>;
    
    /// Ước tính phí L1
    async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256>;
}

/// Trait bổ sung cho Arbitrum
#[async_trait]
pub trait ArbitrumAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy L1 gas price
    async fn get_l1_gas_price(&self) -> Result<U256>;
    
    /// Ước tính phí L1
    async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256>;
}

/// Trait bổ sung cho Polygon
#[async_trait]
pub trait PolygonAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy thông tin gas của Polygon
    async fn get_polygon_gas_station(&self) -> Result<serde_json::Value>;
}

/// Trait bổ sung cho Base
#[async_trait]
pub trait BaseAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy L1 fee scale
    async fn get_l1_fee_scale(&self) -> Result<f64>;
}

/// Trait bổ sung cho Monad
#[async_trait]
pub trait MonadAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy thông tin hiệu suất của Monad
    async fn get_monad_performance(&self) -> Result<serde_json::Value>;
}

/// Trait dành cho các chain tùy chỉnh
#[async_trait]
pub trait CustomChainAdapter: ChainAdapter + Send + Sync + 'static {
    /// Lấy tên chain
    fn get_custom_chain_name(&self) -> &str;
    
    /// Lấy tên native token
    fn get_native_token_name(&self) -> &str;
    
    /// Lấy tên wrapped native token
    fn get_wrapped_native_token(&self) -> &str;
}

/// Enum bọc cho PolygonAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum PolygonAdapterEnum {
    Polygon(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for PolygonAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => {
                // Vì adapter là Arc, chúng ta cần xử lý đặc biệt với set_wallet
                // Đây chỉ là placeholder, cần thực hiện chi tiết hơn tùy theo thiết kế cụ thể
                unimplemented!("set_wallet for PolygonAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl PolygonAdapterEnum {
    /// Lấy thông tin gas của Polygon
    pub async fn get_polygon_gas_station(&self) -> Result<serde_json::Value> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => {
                // Cast adapter to PolygonAdapter
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn PolygonAdapter>() {
                    adapter.get_polygon_gas_station().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng PolygonAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl PolygonAdapter for PolygonAdapterEnum {
    async fn get_polygon_gas_station(&self) -> Result<serde_json::Value> {
        match self {
            PolygonAdapterEnum::Polygon(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn PolygonAdapter>() {
                    adapter.get_polygon_gas_station().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng PolygonAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho BaseAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum BaseAdapterEnum {
    Base(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for BaseAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            BaseAdapterEnum::Base(adapter) => {
                // Vì adapter là Arc, chúng ta cần xử lý đặc biệt với set_wallet
                unimplemented!("set_wallet for BaseAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            BaseAdapterEnum::Base(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl BaseAdapterEnum {
    /// Lấy L1 fee scale
    pub async fn get_l1_fee_scale(&self) -> Result<f64> {
        match self {
            BaseAdapterEnum::Base(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn BaseAdapter>() {
                    adapter.get_l1_fee_scale().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng BaseAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl BaseAdapter for BaseAdapterEnum {
    async fn get_l1_fee_scale(&self) -> Result<f64> {
        match self {
            BaseAdapterEnum::Base(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn BaseAdapter>() {
                    adapter.get_l1_fee_scale().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng BaseAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho MonadAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum MonadAdapterEnum {
    Monad(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for MonadAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            MonadAdapterEnum::Monad(adapter) => {
                // Vì adapter là Arc, chúng ta cần xử lý đặc biệt với set_wallet
                unimplemented!("set_wallet for MonadAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            MonadAdapterEnum::Monad(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl MonadAdapterEnum {
    /// Lấy thông tin hiệu suất của Monad
    pub async fn get_monad_performance(&self) -> Result<serde_json::Value> {
        match self {
            MonadAdapterEnum::Monad(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn MonadAdapter>() {
                    adapter.get_monad_performance().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng MonadAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl MonadAdapter for MonadAdapterEnum {
    async fn get_monad_performance(&self) -> Result<serde_json::Value> {
        match self {
            MonadAdapterEnum::Monad(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn MonadAdapter>() {
                    adapter.get_monad_performance().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng MonadAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho EthereumAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum EthereumAdapterEnum {
    Ethereum(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for EthereumAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => {
                unimplemented!("set_wallet for EthereumAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl EthereumAdapterEnum {
    /// Gửi transaction lên mạng Ethereum
    pub async fn send_to_ethereum(&self, to: &str, value: U256) -> Result<Option<TransactionReceipt>> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn EthereumAdapter>() {
                    adapter.send_to_ethereum(to, value).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng EthereumAdapter"))
                }
            }
        }
    }
    
    /// Kiểm tra số lượng eth2 đã stake
    pub async fn get_eth2_staking_balance(&self, address: &str) -> Result<U256> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn EthereumAdapter>() {
                    adapter.get_eth2_staking_balance(address).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng EthereumAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl EthereumAdapter for EthereumAdapterEnum {
    async fn send_to_ethereum(&self, to: &str, value: U256) -> Result<Option<TransactionReceipt>> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn EthereumAdapter>() {
                    adapter.send_to_ethereum(to, value).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng EthereumAdapter"))
                }
            }
        }
    }
    
    async fn get_eth2_staking_balance(&self, address: &str) -> Result<U256> {
        match self {
            EthereumAdapterEnum::Ethereum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn EthereumAdapter>() {
                    adapter.get_eth2_staking_balance(address).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng EthereumAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho BSCAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum BSCAdapterEnum {
    BSC(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for BSCAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            BSCAdapterEnum::BSC(adapter) => {
                unimplemented!("set_wallet for BSCAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            BSCAdapterEnum::BSC(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl BSCAdapterEnum {
    /// Tương tác với PancakeSwap
    pub async fn swap_via_pancakeswap(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            BSCAdapterEnum::BSC(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn BSCAdapter>() {
                    adapter.swap_via_pancakeswap(token_address, amount_in, min_amount_out).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng BSCAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl BSCAdapter for BSCAdapterEnum {
    async fn swap_via_pancakeswap(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            BSCAdapterEnum::BSC(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn BSCAdapter>() {
                    adapter.swap_via_pancakeswap(token_address, amount_in, min_amount_out).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng BSCAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho AvalancheAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum AvalancheAdapterEnum {
    Avalanche(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for AvalancheAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => {
                unimplemented!("set_wallet for AvalancheAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl AvalancheAdapterEnum {
    pub async fn swap_exact_avax_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn AvalancheAdapter>() {
                    adapter.swap_exact_avax_for_tokens(token_address, amount_in, min_amount_out, recipient, deadline).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng AvalancheAdapter"))
                }
            }
        }
    }
    
    pub async fn swap_exact_tokens_for_avax(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn AvalancheAdapter>() {
                    adapter.swap_exact_tokens_for_avax(token_address, amount_in, min_amount_out, recipient, deadline).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng AvalancheAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl AvalancheAdapter for AvalancheAdapterEnum {
    async fn swap_exact_avax_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn AvalancheAdapter>() {
                    adapter.swap_exact_avax_for_tokens(token_address, amount_in, min_amount_out, recipient, deadline).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng AvalancheAdapter"))
                }
            }
        }
    }
    
    async fn swap_exact_tokens_for_avax(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
    ) -> Result<Option<TransactionReceipt>> {
        match self {
            AvalancheAdapterEnum::Avalanche(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn AvalancheAdapter>() {
                    adapter.swap_exact_tokens_for_avax(token_address, amount_in, min_amount_out, recipient, deadline).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng AvalancheAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho OptimismAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum OptimismAdapterEnum {
    Optimism(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for OptimismAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => {
                unimplemented!("set_wallet for OptimismAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl OptimismAdapterEnum {
    pub async fn get_l1_gas_price(&self) -> Result<U256> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn OptimismAdapter>() {
                    adapter.get_l1_gas_price().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng OptimismAdapter"))
                }
            }
        }
    }
    
    pub async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn OptimismAdapter>() {
                    adapter.estimate_l1_fee(data_length).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng OptimismAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl OptimismAdapter for OptimismAdapterEnum {
    async fn get_l1_gas_price(&self) -> Result<U256> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn OptimismAdapter>() {
                    adapter.get_l1_gas_price().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng OptimismAdapter"))
                }
            }
        }
    }
    
    async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256> {
        match self {
            OptimismAdapterEnum::Optimism(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn OptimismAdapter>() {
                    adapter.estimate_l1_fee(data_length).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng OptimismAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho ArbitrumAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum ArbitrumAdapterEnum {
    Arbitrum(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for ArbitrumAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => {
                unimplemented!("set_wallet for ArbitrumAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl ArbitrumAdapterEnum {
    pub async fn get_l1_gas_price(&self) -> Result<U256> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn ArbitrumAdapter>() {
                    adapter.get_l1_gas_price().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ArbitrumAdapter"))
                }
            }
        }
    }
    
    pub async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn ArbitrumAdapter>() {
                    adapter.estimate_l1_fee(data_length).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ArbitrumAdapter"))
                }
            }
        }
    }
}

#[async_trait]
impl ArbitrumAdapter for ArbitrumAdapterEnum {
    async fn get_l1_gas_price(&self) -> Result<U256> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn ArbitrumAdapter>() {
                    adapter.get_l1_gas_price().await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ArbitrumAdapter"))
                }
            }
        }
    }
    
    async fn estimate_l1_fee(&self, data_length: usize) -> Result<U256> {
        match self {
            ArbitrumAdapterEnum::Arbitrum(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn ArbitrumAdapter>() {
                    adapter.estimate_l1_fee(data_length).await
                } else {
                    Err(anyhow::anyhow!("Adapter không hỗ trợ chức năng ArbitrumAdapter"))
                }
            }
        }
    }
}

/// Enum bọc cho CustomChainAdapter để có thể sử dụng như trait object
#[derive(Debug, Clone)]
pub enum CustomChainAdapterEnum {
    Custom(Arc<dyn ChainAdapter + Send + Sync>),
}

#[async_trait]
impl ChainAdapter for CustomChainAdapterEnum {
    fn get_config(&self) -> &crate::chain_adapters::base::ChainConfig {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_config(),
        }
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_provider(),
        }
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_wallet(),
        }
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                unimplemented!("set_wallet for CustomChainAdapterEnum is not implemented")
            }
        }
    }
    
    fn get_gas_optimizer(&self) -> Option<&crate::gas_optimizer::GasOptimizer> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_gas_optimizer(),
        }
    }
    
    fn decode_router_input(&self, input: &[u8]) -> Result<Vec<Token>> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.decode_router_input(input),
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_native_to_token_path(token_address),
        }
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => adapter.get_token_to_native_path(token_address),
        }
    }
}

impl CustomChainAdapterEnum {
    /// Lấy tên chain
    pub fn get_custom_chain_name(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_custom_chain_name()
                } else {
                    "Unknown Custom Chain"
                }
            }
        }
    }
    
    /// Lấy tên native token
    pub fn get_native_token_name(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_native_token_name()
                } else {
                    "Unknown Token"
                }
            }
        }
    }
    
    /// Lấy tên wrapped native token
    pub fn get_wrapped_native_token(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_wrapped_native_token()
                } else {
                    "0x0000000000000000000000000000000000000000"
                }
            }
        }
    }
}

impl CustomChainAdapter for CustomChainAdapterEnum {
    fn get_custom_chain_name(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_custom_chain_name()
                } else {
                    "Unknown Custom Chain"
                }
            }
        }
    }
    
    fn get_native_token_name(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_native_token_name()
                } else {
                    "Unknown Token"
                }
            }
        }
    }
    
    fn get_wrapped_native_token(&self) -> &str {
        match self {
            CustomChainAdapterEnum::Custom(adapter) => {
                if let Some(adapter) = adapter.as_any().downcast_ref::<dyn CustomChainAdapter>() {
                    adapter.get_wrapped_native_token()
                } else {
                    "0x0000000000000000000000000000000000000000"
                }
            }
        }
    }
} 
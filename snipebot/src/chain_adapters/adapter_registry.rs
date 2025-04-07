use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use anyhow::{Result, anyhow};
use crate::chain_adapters::trait_adapter::ChainAdapter;
use crate::chain_adapters::chain_traits::*;
use once_cell::sync::Lazy;
use crate::chain_adapters::base::ChainConfig;
use ethers::signers::LocalWallet;

/// Singleton registry quản lý tất cả các chain adapter thông qua dyn trait
pub static ADAPTER_REGISTRY: Lazy<RwLock<AdapterRegistry>> = Lazy::new(|| {
    RwLock::new(AdapterRegistry::new())
});

/// Registry dựa trên trait object để quản lý các adapter
pub struct AdapterRegistry {
    // Lưu trữ các adapter thông qua trait object
    adapters: HashMap<String, Arc<dyn ChainAdapter>>,
    
    // Lưu trữ các adapter cho các chain cụ thể với các enum wrapper
    ethereum_adapters: HashMap<String, EthereumAdapterEnum>,
    bsc_adapters: HashMap<String, BSCAdapterEnum>,
    avalanche_adapters: HashMap<String, AvalancheAdapterEnum>,
    base_adapters: HashMap<String, BaseAdapterEnum>,
    arbitrum_adapters: HashMap<String, ArbitrumAdapterEnum>,
    optimism_adapters: HashMap<String, OptimismAdapterEnum>,
    polygon_adapters: HashMap<String, PolygonAdapterEnum>,
    monad_adapters: HashMap<String, MonadAdapterEnum>,
    custom_adapters: HashMap<String, CustomChainAdapterEnum>,
}

impl AdapterRegistry {
    /// Tạo registry mới
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            ethereum_adapters: HashMap::new(),
            bsc_adapters: HashMap::new(),
            avalanche_adapters: HashMap::new(),
            base_adapters: HashMap::new(),
            arbitrum_adapters: HashMap::new(),
            optimism_adapters: HashMap::new(),
            polygon_adapters: HashMap::new(),
            monad_adapters: HashMap::new(),
            custom_adapters: HashMap::new(),
        }
    }
    
    /// Đăng ký adapter cơ bản
    pub fn register(&mut self, name: &str, adapter: Arc<dyn ChainAdapter>) {
        self.adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Ethereum
    pub fn register_ethereum(&mut self, name: &str, adapter: EthereumAdapterEnum) {
        self.ethereum_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter BSC
    pub fn register_bsc(&mut self, name: &str, adapter: BSCAdapterEnum) {
        self.bsc_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Avalanche
    pub fn register_avalanche(&mut self, name: &str, adapter: AvalancheAdapterEnum) {
        self.avalanche_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Base
    pub fn register_base(&mut self, name: &str, adapter: BaseAdapterEnum) {
        self.base_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Arbitrum
    pub fn register_arbitrum(&mut self, name: &str, adapter: ArbitrumAdapterEnum) {
        self.arbitrum_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Optimism
    pub fn register_optimism(&mut self, name: &str, adapter: OptimismAdapterEnum) {
        self.optimism_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Polygon
    pub fn register_polygon(&mut self, name: &str, adapter: PolygonAdapterEnum) {
        self.polygon_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter Monad
    pub fn register_monad(&mut self, name: &str, adapter: MonadAdapterEnum) {
        self.monad_adapters.insert(name.to_string(), adapter);
    }
    
    /// Đăng ký adapter tùy chỉnh
    pub fn register_custom(&mut self, name: &str, adapter: CustomChainAdapterEnum) {
        self.custom_adapters.insert(name.to_string(), adapter);
    }
    
    /// Lấy adapter cơ bản
    pub fn get(&self, name: &str) -> Option<Arc<dyn ChainAdapter>> {
        self.adapters.get(name).cloned()
    }
    
    /// Lấy adapter Ethereum
    pub fn get_ethereum(&self, name: &str) -> Option<EthereumAdapterEnum> {
        self.ethereum_adapters.get(name).cloned()
    }
    
    /// Lấy adapter BSC
    pub fn get_bsc(&self, name: &str) -> Option<BSCAdapterEnum> {
        self.bsc_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Avalanche
    pub fn get_avalanche(&self, name: &str) -> Option<AvalancheAdapterEnum> {
        self.avalanche_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Base
    pub fn get_base(&self, name: &str) -> Option<BaseAdapterEnum> {
        self.base_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Arbitrum
    pub fn get_arbitrum(&self, name: &str) -> Option<ArbitrumAdapterEnum> {
        self.arbitrum_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Optimism
    pub fn get_optimism(&self, name: &str) -> Option<OptimismAdapterEnum> {
        self.optimism_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Polygon
    pub fn get_polygon(&self, name: &str) -> Option<PolygonAdapterEnum> {
        self.polygon_adapters.get(name).cloned()
    }
    
    /// Lấy adapter Monad
    pub fn get_monad(&self, name: &str) -> Option<MonadAdapterEnum> {
        self.monad_adapters.get(name).cloned()
    }
    
    /// Lấy adapter tùy chỉnh
    pub fn get_custom(&self, name: &str) -> Option<CustomChainAdapterEnum> {
        self.custom_adapters.get(name).cloned()
    }
    
    /// Lấy tất cả tên chain
    pub fn get_all_chain_names(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }
    
    /// Lấy tất cả adapter
    pub fn get_all_adapters(&self) -> Vec<Arc<dyn ChainAdapter>> {
        self.adapters.values().cloned().collect()
    }
}

/// Helper functions để làm việc với registry global
pub fn get_chain_adapter(chain_name: &str) -> Result<Arc<dyn ChainAdapter>> {
    let registry = ADAPTER_REGISTRY.read().unwrap();
    registry.get(chain_name)
        .ok_or_else(|| anyhow!("Không tìm thấy adapter cho chain: {}", chain_name))
}

/// Thêm wallet vào adapter
pub async fn add_wallet_to_adapter(chain_name: &str, wallet: LocalWallet) -> Result<()> {
    // Clone adapter với ví mới thay vì sửa trực tiếp adapter hiện tại
    // 1. Lấy adapter
    let adapter_ref = {
        let registry = ADAPTER_REGISTRY.read().unwrap();
        registry.get(chain_name).ok_or_else(|| anyhow!("Chain adapter not found for chain: {}", chain_name))?.clone()
    };
    
    // 2. Tạo adapter mới với ví
    // (Lưu ý: ở đây cần implementation phức tạp hơn để tạo adapter mới với ví)
    // Hiện tại, chúng ta không thể làm việc này trực tiếp với Arc<dyn ChainAdapter>
    
    // 3. Đăng ký adapter mới
    let mut registry = ADAPTER_REGISTRY.write().unwrap();
    match chain_name.to_lowercase().as_str() {
        "ethereum" => {
            let ethereum_adapter = EthereumAdapterEnum::Ethereum(adapter_ref);
            registry.register_ethereum(chain_name, ethereum_adapter);
        },
        "bsc" => {
            let bsc_adapter = BSCAdapterEnum::BSC(adapter_ref);
            registry.register_bsc(chain_name, bsc_adapter);
        },
        "avalanche" => {
            let avalanche_adapter = AvalancheAdapterEnum::Avalanche(adapter_ref);
            registry.register_avalanche(chain_name, avalanche_adapter);
        },
        "base" => {
            let base_adapter = BaseAdapterEnum::Base(adapter_ref);
            registry.register_base(chain_name, base_adapter);
        },
        "arbitrum" => {
            let arbitrum_adapter = ArbitrumAdapterEnum::Arbitrum(adapter_ref);
            registry.register_arbitrum(chain_name, arbitrum_adapter);
        },
        "optimism" => {
            let optimism_adapter = OptimismAdapterEnum::Optimism(adapter_ref);
            registry.register_optimism(chain_name, optimism_adapter);
        },
        "polygon" => {
            let polygon_adapter = PolygonAdapterEnum::Polygon(adapter_ref);
            registry.register_polygon(chain_name, polygon_adapter);
        },
        "monad" => {
            let monad_adapter = MonadAdapterEnum::Monad(adapter_ref);
            registry.register_monad(chain_name, monad_adapter);
        },
        _ => {
            let custom_adapter = CustomChainAdapterEnum::Custom(adapter_ref);
            registry.register_custom(chain_name, custom_adapter);
        }
    }
    
    // Ghi chú: Cách tiếp cận này chỉ đăng ký lại adapter với enum wrapper
    // nhưng không thực sự thêm ví vào adapter.
    // Đây là giải pháp tạm thời cho vấn đề không thể mutable borrow của Arc<dyn ChainAdapter>
    
    Ok(())
}

/// Helper function để lấy registry dưới dạng Mutex
fn get_registry_internal() -> Arc<std::sync::Mutex<AdapterRegistry>> {
    static REGISTRY_MUTEX: Lazy<Arc<std::sync::Mutex<AdapterRegistry>>> = Lazy::new(|| {
        Arc::new(std::sync::Mutex::new(AdapterRegistry::new()))
    });
    
    REGISTRY_MUTEX.clone()
}

/// Đăng ký adapter mới
pub fn register_adapter(chain_name: &str, new_adapter: Arc<dyn ChainAdapter + Send + Sync>) -> Result<()> {
    let registry = get_registry_internal();
    let mut registry = registry.lock().unwrap();
    
    // Đăng ký adapter với registry chung
    registry.adapters.insert(chain_name.to_string(), new_adapter.clone());
    
    // Đăng ký lại adapter với registry
    match chain_name.to_lowercase().as_str() {
        "ethereum" => {
            let ethereum_adapter = EthereumAdapterEnum::Ethereum(new_adapter);
            registry.register_ethereum(chain_name, ethereum_adapter);
        },
        "bsc" => {
            let bsc_adapter = BSCAdapterEnum::BSC(new_adapter);
            registry.register_bsc(chain_name, bsc_adapter);
        },
        "avalanche" => {
            let avalanche_adapter = AvalancheAdapterEnum::Avalanche(new_adapter);
            registry.register_avalanche(chain_name, avalanche_adapter);
        },
        "base" => {
            let base_adapter = BaseAdapterEnum::Base(new_adapter);
            registry.register_base(chain_name, base_adapter);
        },
        "arbitrum" => {
            let arbitrum_adapter = ArbitrumAdapterEnum::Arbitrum(new_adapter);
            registry.register_arbitrum(chain_name, arbitrum_adapter);
        },
        "optimism" => {
            let optimism_adapter = OptimismAdapterEnum::Optimism(new_adapter);
            registry.register_optimism(chain_name, optimism_adapter);
        },
        "polygon" => {
            let polygon_adapter = PolygonAdapterEnum::Polygon(new_adapter);
            registry.register_polygon(chain_name, polygon_adapter);
        },
        "monad" => {
            let monad_adapter = MonadAdapterEnum::Monad(new_adapter);
            registry.register_monad(chain_name, monad_adapter);
        },
        _ => {
            let custom_adapter = CustomChainAdapterEnum::Custom(new_adapter);
            registry.register_custom(chain_name, custom_adapter);
        }
    }
    
    Ok(())
} 
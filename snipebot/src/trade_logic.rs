// Diamondchain - Copyright (c) 2023

// External imports
use ethers::{
    abi::{Abi, Function, Event},
    contract::Contract,
    middleware::Middleware,
    types::{Address, H256, U256, Bytes, Filter, Log, AccessList},
    utils::hex,
};

// Standard library imports
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    fmt::{self, Display, Formatter},
    error::Error,
};

// Internal imports
use crate::{
    chain_adapters::{ChainAdapter, AsyncChainAdapter},
    blockchain::{Blockchain, Transaction, TransactionReceipt},
    network::{Network, NetworkConfig},
    storage::{Storage, StorageConfig},
    risk_analyzer::{RiskAnalyzer, RiskConfig},
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::{timeout, sleep};

// Internal imports
use crate::{
    risk_analyzer::{RiskAnalyzer, TokenRiskAnalysis},
    gas_optimizer::GasOptimizer,
    token_status::{TokenStatusTracker, TokenStatus, TokenPriceAlert, PriceAlertType},
    error::{TransactionError, classify_blockchain_error, get_recovery_info, RecoveryAction},
    abi_utils,
    storage::Storage,
    types::{
        TradeConfig, TradeStats, TradeResult, TradeType, TradingPosition,
        ProfitTarget, StopLossConfig, AIConfig, AISuggestion, DCAStrategy,
        DCAInterval, MonteCarloConfig, MonteCarloResult
    },
    mempool::{MempoolTracker, MempoolTransaction, TransactionType},
};

lazy_static! {
    static ref TRADE_COUNTER: IntCounter = IntCounter::new(
        "trade_total",
        "Total number of trades executed"
    ).unwrap();

    static ref TRADE_SUCCESS_COUNTER: IntCounter = IntCounter::new(
        "trade_success_total",
        "Total number of successful trades"
    ).unwrap();

    static ref TRADE_ERROR_COUNTER: IntCounter = IntCounter::new(
        "trade_error_total",
        "Total number of failed trades"
    ).unwrap();

    static ref GAS_USED_HISTOGRAM: Histogram = Histogram::with_opts(
        opts!(
            "trade_gas_used",
            "Gas used per trade",
            vec![1000.0, 5000.0, 10000.0, 50000.0, 100000.0, 500000.0]
        )
    ).unwrap();

    static ref TRADE_AMOUNT_HISTOGRAM: Histogram = Histogram::with_opts(
        opts!(
            "trade_amount",
            "Trade amount in ETH",
            vec![0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0]
        )
    ).unwrap();

    static ref ACTIVE_TRADES_GAUGE: Gauge = Gauge::new(
        "active_trades",
        "Number of active trades"
    ).unwrap();
}

/// Các chiến lược giao dịch được hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TradingStrategy {
    /// Theo dõi chỉ đơn thuần, không giao dịch
    Monitor,
    /// Tích lũy dần theo thời gian
    Accumulate,
    /// Mua trước (front-running) dựa trên mempool
    MempoolFrontrun,
    /// Mua với số lượng giới hạn
    LimitedBuy,
    /// Sandwich attack (mua trước/bán sau)
    SandwichAttack,
    /// Arbitrage giữa các DEX
    Arbitrage,
}

/// Các loại lệnh giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Lệnh mua giới hạn
    BuyLimit,
    /// Lệnh bán giới hạn
    SellLimit,
    /// Lệnh mua thị trường
    BuyMarket,
    /// Lệnh bán thị trường
    SellMarket,
    /// Lệnh dừng lỗ
    StopLoss,
    /// Lệnh chốt lời
    TakeProfit,
    /// Lệnh dừng lỗ trượt
    TrailingStop,
}

/// Trạng thái của lệnh giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Lệnh đang hoạt động
    Active,
    /// Lệnh đã được thực hiện
    Filled, 
    /// Lệnh đã bị hủy
    Cancelled,
    /// Lệnh đã hết hạn
    Expired, 
    /// Lệnh đang chờ xử lý
    Pending,
    /// Lệnh đã bị từ chối
    Rejected,
}

/// Loại chiến lược giao dịch
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum StrategyType {
    /// Chiến lược snipe
    Snipe,
    /// Chiến lược sandwich
    Sandwich,
    /// Chiến lược frontrun
    Frontrun,
    /// Chiến lược tự động
    Auto,
}

/// Cấu hình giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeConfig {
    /// Giới hạn gas
    pub gas_limit: U256,
    /// Giá gas
    pub gas_price: U256,
    /// Độ trượt giá cho phép
    pub slippage: f64,
    /// Thời gian chờ tối đa
    pub timeout: u64,
    /// Tự động phê duyệt token
    pub auto_approve: bool,
    /// Sử dụng Flashbots
    pub use_flashbots: bool,
    /// Hệ số nhân gas cho bán khẩn cấp
    pub emergency_sell_gas_multiplier: f64,
    /// Địa chỉ router
    pub router_address: String,
    /// Địa chỉ wrapped native token
    pub wrapped_native_token: String,
    /// Độ trượt giá tối đa
    pub max_slippage: f64,
    /// Kích thước cửa sổ TWAP
    pub twap_window_size: usize,
    /// Số mẫu tối thiểu cho TWAP
    pub twap_min_samples: usize,
    /// Khoảng thời gian cập nhật TWAP
    pub twap_update_interval: u64,
}

/// Kết quả giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    /// Trạng thái thành công/thất bại
    pub success: bool,
    /// Hash của giao dịch
    pub tx_hash: Option<String>,
    /// Số lượng token vào
    pub amount_in: String,
    /// Số lượng token ra
    pub amount_out: Option<String>,
    /// Giá mỗi token
    pub price_per_token: Option<f64>,
    /// Gas đã sử dụng
    pub gas_used: Option<u64>,
    /// Giá gas
    pub gas_price: u64,
    /// Thời gian thực hiện
    pub timestamp: u64,
    /// Lỗi nếu có
    pub error: Option<String>,
    /// Loại giao dịch
    pub trade_type: TradeType,
    /// Địa chỉ token
    pub token_address: String,
    /// Hash giao dịch nạn nhân (nếu có)
    pub victim_tx_hash: Option<String>,
    /// Lợi nhuận USD (nếu có)
    pub profit_usd: Option<f64>,
}

/// Vị thế giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingPosition {
    /// Địa chỉ token
    pub token_address: String,
    /// Ký hiệu token
    pub token_symbol: String,
    /// Số thập phân của token
    pub token_decimals: u8,
    /// Số lượng token
    pub amount: String,
    /// Giá vốn (tính bằng ETH)
    pub cost_basis: f64,
    /// Giá vào (tính bằng ETH/token)
    pub entry_price: f64,
    /// Giá hiện tại
    pub current_price: Option<f64>,
    /// Lợi nhuận/lỗ chưa thực hiện (tính bằng USD)
    pub unrealized_profit_loss: Option<f64>,
    /// Phần trăm lợi nhuận/lỗ chưa thực hiện
    pub unrealized_profit_loss_percent: Option<f64>,
    /// Lợi nhuận/lỗ đã thực hiện (tính bằng USD)
    pub realized_profit_loss: f64,
    /// Thời gian mua
    pub buy_timestamp: u64,
    /// Thời gian cập nhật cuối
    pub last_updated: u64,
}

/// Cấu trúc lưu trữ dữ liệu cache
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    pub value: T,
    pub expires_at: Instant,
}

impl<T> CacheEntry<T> {
    /// Tạo entry cache mới
    pub fn new(value: T, ttl_seconds: u64) -> Self {
        Self {
            value,
            expires_at: Instant::now() + Duration::from_secs(ttl_seconds),
        }
    }

    /// Kiểm tra entry đã hết hạn chưa
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Cấu trúc quản lý giao dịch
#[derive(Debug)]
pub struct TradeManager<A: ChainAdapter + AsyncChainAdapter + Send + Sync + 'static> {
    /// Chain adapter
    chain_adapter: Arc<A>,
    
    /// Blockchain
    blockchain: Arc<Blockchain>,
    
    /// Network
    network: Arc<Network>,
    
    /// Storage
    storage: Arc<Storage>,
    
    /// Risk analyzer
    risk_analyzer: Arc<dyn RiskAnalyzer>,
    
    /// Mempool tracker
    mempool_tracker: Option<Arc<Mutex<dyn MempoolTracker>>>,
    
    /// TWAP calculator
    twap_calculator: Option<Arc<Mutex<dyn TWAPCalculator>>>,
    
    /// Monte Carlo simulator
    monte_carlo_simulator: Option<Arc<Mutex<dyn MonteCarloSimulator>>>,
    
    /// DCA strategies
    dca_strategies: HashMap<String, dyn DCAStrategy>,
    
    /// Cache
    cache: RwLock<HashMap<String, CacheEntry<Vec<u8>>>>,
    
    /// Trading position
    position: Arc<Mutex<TradingPosition>>,
    
    /// Callbacks
    callbacks: Arc<Mutex<Callbacks>>,
    
    /// Config
    config: TradeConfig,
}

impl<A: ChainAdapter + AsyncChainAdapter + Send + Sync + 'static> TradeManager<A> {
    /// Tạo TradeManager mới
    pub fn new(
        chain_adapter: Arc<A>,
        blockchain: Arc<Blockchain>,
        network: Arc<Network>,
        storage: Arc<Storage>,
        risk_analyzer: Arc<dyn RiskAnalyzer>,
        config: TradeConfig,
    ) -> Self {
        Self {
            chain_adapter,
            blockchain,
            network,
            storage,
            risk_analyzer,
            mempool_tracker: None,
            twap_calculator: None,
            monte_carlo_simulator: None,
            dca_strategies: HashMap::new(),
            cache: RwLock::new(HashMap::new()),
            position: Arc::new(Mutex::new(TradingPosition::default())),
            callbacks: Arc::new(Mutex::new(Callbacks::default())),
            config,
        }
    }

    /// Lấy giá trị từ cache
    pub async fn get_from_cache(&self, key: &str) -> Option<Vec<u8>> {
        let cache = self.cache.read().unwrap();
        if let Some(entry) = cache.get(key) {
            if !entry.is_expired() {
                return Some(entry.value.clone());
            }
        }
        None
    }

    /// Lưu giá trị vào cache
    pub async fn store_in_cache(&self, key: String, value: Vec<u8>, ttl: Duration) {
        let mut cache = self.cache.write().unwrap();
        cache.insert(key, CacheEntry::new(value, ttl.as_secs() as u64));
    }

    /// Xóa các entry cache đã hết hạn
    pub async fn cleanup_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.retain(|_, entry| !entry.is_expired());
    }

    /// Triển khai FlashbotsBundleProvider
    #[async_trait]
    impl FlashbotsBundleProvider for TradeManager<A> {
        async fn send_bundle(&self, bundle: FlashbotsBundle) -> Result<()> {
            if let Some(provider) = &self.flashbots_provider {
                let mut provider = provider.lock().await;
                provider.send_bundle(bundle).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn simulate_bundle(&self, bundle: FlashbotsBundle) -> Result<()> {
            if let Some(provider) = &self.flashbots_provider {
                let mut provider = provider.lock().await;
                provider.simulate_bundle(bundle).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundle(&self, bundle_hash: H256) -> Result<Option<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundle(bundle_hash).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles(&self) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles().await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_block(&self, block_number: u64) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_block(block_number).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_address(&self, address: Address) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_address(address).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_token(&self, token_address: Address) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_token(token_address).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_gas_price(&self, min_gas_price: U256) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_gas_price(min_gas_price).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_time(&self, start_time: u64, end_time: u64) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_time(start_time, end_time).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }

        async fn get_bundles_by_value(&self, min_value: U256) -> Result<Vec<FlashbotsBundle>> {
            if let Some(provider) = &self.flashbots_provider {
                let provider = provider.lock().await;
                provider.get_bundles_by_value(min_value).await
            } else {
                Err(anyhow::anyhow!("Flashbots provider not initialized"))
            }
        }
    }

    /// Triển khai TWAPCalculator
    impl TWAPCalculator for TradeManager<A> {
        fn update_twap(&mut self, price: U256) -> Result<()> {
            if let Some(calculator) = &self.twap_calculator {
                let mut calculator = calculator.lock().unwrap();
                calculator.update_twap(price)
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_twap(&self) -> Result<Option<U256>> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_twap()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_twap_prices(&self) -> Result<Vec<(U256, Instant)>> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_twap_prices()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_last_update_time(&self) -> Result<Option<Instant>> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_last_update_time()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_sample_count(&self) -> Result<usize> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_sample_count()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_time_window(&self) -> Result<Duration> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_time_window()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_min_samples(&self) -> Result<usize> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_min_samples()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_update_interval(&self) -> Result<Duration> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_update_interval()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_standard_deviation(&self) -> Result<f64> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_standard_deviation()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }

        fn get_relative_standard_deviation(&self) -> Result<f64> {
            if let Some(calculator) = &self.twap_calculator {
                let calculator = calculator.lock().unwrap();
                calculator.get_relative_standard_deviation()
            } else {
                Err(anyhow::anyhow!("TWAP calculator not initialized"))
            }
        }
    }

    /// Triển khai MonteCarloSimulator
    impl MonteCarloSimulator for TradeManager<A> {
        fn run_simulation(&mut self, token_address: &str, config: MonteCarloConfig) -> Result<()> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let mut simulator = simulator.lock().unwrap();
                simulator.run_simulation(token_address, config)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_simulation_result(&self, token_address: &str) -> Result<Option<MonteCarloResult>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_simulation_result(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_simulation_results(&self) -> Result<Vec<MonteCarloResult>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_simulation_results()
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_simulation_count(&self) -> Result<usize> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_simulation_count()
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_simulation_time(&self, token_address: &str) -> Result<Option<Duration>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_simulation_time(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_mean_value(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_mean_value(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_standard_deviation(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_standard_deviation(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_min_value(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_min_value(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_max_value(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_max_value(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_profit_probability(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_profit_probability(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_loss_probability(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_loss_probability(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_var_value(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_var_value(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }

        fn get_cvar_value(&self, token_address: &str) -> Result<Option<f64>> {
            if let Some(simulator) = &self.monte_carlo_simulator {
                let simulator = simulator.lock().unwrap();
                simulator.get_cvar_value(token_address)
            } else {
                Err(anyhow::anyhow!("Monte Carlo simulator not initialized"))
            }
        }
    }

    /// Triển khai DCAStrategy
    impl DCAStrategy for TradeManager<A> {
        fn add_dca_strategy(&mut self, token_address: &str, strategy: DCAStrategy) -> Result<()> {
            let mut strategies = self.dca_strategies.lock().unwrap();
            strategies.insert(token_address.to_string(), strategy);
            Ok(())
        }

        fn remove_dca_strategy(&mut self, token_address: &str) -> Result<()> {
            let mut strategies = self.dca_strategies.lock().unwrap();
            strategies.remove(token_address);
            Ok(())
        }

        fn get_dca_strategy(&self, token_address: &str) -> Result<Option<DCAStrategy>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).cloned())
        }

        fn get_dca_strategies(&self) -> Result<Vec<DCAStrategy>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.values().cloned().collect())
        }

        fn get_token_amount(&self, token_address: &str) -> Result<Option<U256>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).map(|s| s.amount))
        }

        fn get_interval(&self, token_address: &str) -> Result<Option<DCAInterval>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).map(|s| s.interval))
        }

        fn get_price_limit(&self, token_address: &str) -> Result<Option<U256>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).and_then(|s| s.max_price))
        }

        fn get_start_time(&self, token_address: &str) -> Result<Option<u64>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).and_then(|s| s.last_execution))
        }

        fn get_end_time(&self, token_address: &str) -> Result<Option<u64>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).and_then(|s| s.last_execution))
        }

        fn get_execution_count(&self, token_address: &str) -> Result<u32> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).map(|s| s.execution_count).unwrap_or(0))
        }

        fn get_remaining_count(&self, token_address: &str) -> Result<Option<u32>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).map(|s| s.remaining_count))
        }

        fn get_status(&self, token_address: &str) -> Result<Option<String>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).map(|s| s.status.clone()))
        }

        fn get_error(&self, token_address: &str) -> Result<Option<String>> {
            let strategies = self.dca_strategies.lock().unwrap();
            Ok(strategies.get(token_address).and_then(|s| s.error.clone()))
        }
    }

    /// Triển khai MempoolTracker
    #[async_trait]
    impl<A: ChainAdapter + AsyncChainAdapter + Send + Sync + 'static> MempoolTracker for TradeManager<A> {
        /// Thêm giao dịch vào mempool
        async fn add_transaction(&self, tx: MempoolTransaction) -> Result<()> {
            if let Some(tracker) = &self.mempool_tracker {
                let mut tracker = tracker.lock().await;
                tracker.add_transaction(tx).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Xóa giao dịch khỏi mempool
        async fn remove_transaction(&self, tx_hash: H256) -> Result<()> {
            if let Some(tracker) = &self.mempool_tracker {
                let mut tracker = tracker.lock().await;
                tracker.remove_transaction(tx_hash).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch từ mempool
        async fn get_transaction(&self, tx_hash: H256) -> Result<Option<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transaction(tx_hash).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy tất cả giao dịch trong mempool
        async fn get_transactions(&self) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions().await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch theo loại
        async fn get_transactions_by_type(&self, tx_type: TransactionType) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions_by_type(tx_type).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch theo token
        async fn get_transactions_by_token(&self, token_address: Address) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions_by_token(token_address).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch theo giá gas
        async fn get_transactions_by_gas_price(&self, min_gas_price: U256) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions_by_gas_price(min_gas_price).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch theo thời gian
        async fn get_transactions_by_time(&self, start_time: u64, end_time: u64) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions_by_time(start_time, end_time).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }

        /// Lấy giao dịch theo giá trị
        async fn get_transactions_by_value(&self, min_value: U256) -> Result<Vec<MempoolTransaction>> {
            if let Some(tracker) = &self.mempool_tracker {
                let tracker = tracker.lock().await;
                tracker.get_transactions_by_value(min_value).await
            } else {
                Err(anyhow::anyhow!("Mempool tracker not initialized"))
            }
        }
    }

    /// Triển khai RiskAnalyzer
    #[async_trait]
    impl<A: ChainAdapter + AsyncChainAdapter + Send + Sync + 'static> RiskAnalyzer for TradeManager<A> {
        /// Phân tích rủi ro token
        async fn analyze_token(&self, token_address: &str) -> Result<TokenRiskAnalysis> {
            if let Some(analyzer) = &self.risk_analyzer {
                let analyzer = analyzer.lock().await;
                analyzer.analyze_token(token_address).await
            } else {
                Err(anyhow::anyhow!("Risk analyzer not initialized"))
            }
        }

        /// Kiểm tra honeypot
        async fn check_honeypot(&self, token_address: &str) -> Result<bool> {
            if let Some(analyzer) = &self.risk_analyzer {
                let analyzer = analyzer.lock().await;
                analyzer.check_honeypot(token_address).await
            } else {
                Err(anyhow::anyhow!("Risk analyzer not initialized"))
            }
        }

        /// Đánh giá bảo mật contract
        async fn evaluate_contract_security(&self, token_address: &str) -> Result<ContractSecurity> {
            if let Some(analyzer) = &self.risk_analyzer {
                let analyzer = analyzer.lock().await;
                analyzer.evaluate_contract_security(token_address).await
            } else {
                Err(anyhow::anyhow!("Risk analyzer not initialized"))
            }
        }

        /// Tính điểm rủi ro
        async fn calculate_risk_score(&self, token_address: &str) -> Result<f64> {
            if let Some(analyzer) = &self.risk_analyzer {
                let analyzer = analyzer.lock().await;
                analyzer.calculate_risk_score(token_address).await
            } else {
                Err(anyhow::anyhow!("Risk analyzer not initialized"))
            }
        }
    }

    /// Triển khai GasOptimizer
    impl GasOptimizer for TradeManager<A> {
        fn optimize_gas_price(&mut self, tx: &Transaction) -> Result<U256> {
            if let Some(optimizer) = &self.gas_optimizer {
                let mut optimizer = optimizer.lock().unwrap();
                optimizer.optimize_gas_price(tx)
            } else {
                Err(anyhow::anyhow!("Gas optimizer not initialized"))
            }
        }

        fn estimate_gas_limit(&self, tx: &Transaction) -> Result<U256> {
            if let Some(optimizer) = &self.gas_optimizer {
                let optimizer = optimizer.lock().unwrap();
                optimizer.estimate_gas_limit(tx)
            } else {
                Err(anyhow::anyhow!("Gas optimizer not initialized"))
            }
        }

        fn get_gas_price_history(&self) -> Result<Vec<(U256, Instant)>> {
            if let Some(optimizer) = &self.gas_optimizer {
                let optimizer = optimizer.lock().unwrap();
                optimizer.get_gas_price_history()
            } else {
                Err(anyhow::anyhow!("Gas optimizer not initialized"))
            }
        }

        fn get_gas_price_prediction(&self) -> Result<U256> {
            if let Some(optimizer) = &self.gas_optimizer {
                let optimizer = optimizer.lock().unwrap();
                optimizer.get_gas_price_prediction()
            } else {
                Err(anyhow::anyhow!("Gas optimizer not initialized"))
            }
        }
    }

    /// Triển khai TokenStatusTracker
    impl TokenStatusTracker for TradeManager<A> {
        fn update_token_status(&mut self, token_address: &str, status: TokenStatus) -> Result<()> {
            if let Some(tracker) = &self.token_status_tracker {
                let mut tracker = tracker.lock().unwrap();
                tracker.update_token_status(token_address, status)
            } else {
                Err(anyhow::anyhow!("Token status tracker not initialized"))
            }
        }

        fn get_token_status(&self, token_address: &str) -> Result<Option<TokenStatus>> {
            if let Some(tracker) = &self.token_status_tracker {
                let tracker = tracker.lock().unwrap();
                tracker.get_token_status(token_address)
            } else {
                Err(anyhow::anyhow!("Token status tracker not initialized"))
            }
        }

        fn get_token_statuses(&self) -> Result<Vec<(String, TokenStatus)>> {
            if let Some(tracker) = &self.token_status_tracker {
                let tracker = tracker.lock().unwrap();
                tracker.get_token_statuses()
            } else {
                Err(anyhow::anyhow!("Token status tracker not initialized"))
            }
        }

        fn get_token_history(&self, token_address: &str) -> Result<Vec<(TokenStatus, Instant)>> {
            if let Some(tracker) = &self.token_status_tracker {
                let tracker = tracker.lock().unwrap();
                tracker.get_token_history(token_address)
            } else {
                Err(anyhow::anyhow!("Token status tracker not initialized"))
            }
        }
    }

    pub async fn execute_trade(&self, _trade_type: TradeType, _token_address: String, _amount: f64) -> Result<()> {
        // TODO: Implement trade execution logic
        Ok(())
    }

    pub async fn handle_error(&self, _error: String) {
        // TODO: Implement error handling logic
    }

    pub async fn handle_ai_suggestion(&self, _suggestion_type: AISuggestionType, _token_address: String, _price: f64) {
        // TODO: Implement AI suggestion handling logic
    }
}

/// Trait cho Flashbots Bundle Provider
#[async_trait]
pub trait FlashbotsBundleProvider: Send + Sync {
    /// Gửi bundle
    async fn send_bundle(&self, bundle: FlashbotsBundle) -> Result<()>;
    
    /// Mô phỏng bundle
    async fn simulate_bundle(&self, bundle: FlashbotsBundle) -> Result<()>;
    
    /// Lấy bundle theo hash
    async fn get_bundle(&self, bundle_hash: H256) -> Result<Option<FlashbotsBundle>>;
    
    /// Lấy tất cả bundle
    async fn get_bundles(&self) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo block
    async fn get_bundles_by_block(&self, block_number: u64) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo địa chỉ
    async fn get_bundles_by_address(&self, address: Address) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo token
    async fn get_bundles_by_token(&self, token_address: Address) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo giá gas
    async fn get_bundles_by_gas_price(&self, min_gas_price: U256) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo thời gian
    async fn get_bundles_by_time(&self, start_time: u64, end_time: u64) -> Result<Vec<FlashbotsBundle>>;
    
    /// Lấy bundle theo giá trị
    async fn get_bundles_by_value(&self, min_value: U256) -> Result<Vec<FlashbotsBundle>>;
}

/// Trait cho TWAP Calculator
pub trait TWAPCalculator: Send + Sync {
    /// Cập nhật TWAP với giá mới
    fn update_twap(&mut self, price: U256) -> Result<()>;
    
    /// Lấy giá TWAP hiện tại
    fn get_twap(&self) -> Result<Option<U256>>;
    
    /// Lấy danh sách giá trong cửa sổ TWAP
    fn get_twap_prices(&self) -> Result<Vec<(U256, Instant)>>;
    
    /// Lấy thời điểm cập nhật cuối cùng
    fn get_last_update_time(&self) -> Result<Option<Instant>>;
    
    /// Lấy số lượng mẫu hiện tại
    fn get_sample_count(&self) -> Result<usize>;
    
    /// Lấy kích thước cửa sổ thời gian
    fn get_time_window(&self) -> Result<Duration>;
    
    /// Lấy số mẫu tối thiểu
    fn get_min_samples(&self) -> Result<usize>;
    
    /// Lấy khoảng thời gian cập nhật
    fn get_update_interval(&self) -> Result<Duration>;
    
    /// Lấy độ lệch chuẩn của giá
    fn get_standard_deviation(&self) -> Result<f64>;
    
    /// Lấy độ lệch chuẩn tương đối
    fn get_relative_standard_deviation(&self) -> Result<f64>;
}

/// Trait cho Monte Carlo Simulator
pub trait MonteCarloSimulator: Send + Sync {
    /// Chạy mô phỏng Monte Carlo
    fn run_simulation(&mut self, token_address: &str, config: MonteCarloConfig) -> Result<()>;
    
    /// Lấy kết quả mô phỏng cho một token
    fn get_simulation_result(&self, token_address: &str) -> Result<Option<MonteCarloResult>>;
    
    /// Lấy tất cả kết quả mô phỏng
    fn get_simulation_results(&self) -> Result<Vec<MonteCarloResult>>;
    
    /// Lấy số lượng mô phỏng đã chạy
    fn get_simulation_count(&self) -> Result<usize>;
    
    /// Lấy thời gian mô phỏng
    fn get_simulation_time(&self, token_address: &str) -> Result<Option<Duration>>;
    
    /// Lấy giá trị trung bình
    fn get_mean_value(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy độ lệch chuẩn
    fn get_standard_deviation(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy giá trị nhỏ nhất
    fn get_min_value(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy giá trị lớn nhất
    fn get_max_value(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy xác suất lợi nhuận
    fn get_profit_probability(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy xác suất lỗ
    fn get_loss_probability(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy giá trị VaR
    fn get_var_value(&self, token_address: &str) -> Result<Option<f64>>;
    
    /// Lấy giá trị CVaR
    fn get_cvar_value(&self, token_address: &str) -> Result<Option<f64>>;
}

/// Trait cho DCA Strategy
pub trait DCAStrategy: Send + Sync {
    /// Thêm chiến lược DCA mới
    fn add_dca_strategy(&mut self, token_address: &str, strategy: DCAStrategy) -> Result<()>;
    
    /// Xóa chiến lược DCA
    fn remove_dca_strategy(&mut self, token_address: &str) -> Result<()>;
    
    /// Lấy chiến lược DCA
    fn get_dca_strategy(&self, token_address: &str) -> Result<Option<DCAStrategy>>;
    
    /// Lấy tất cả chiến lược DCA
    fn get_dca_strategies(&self) -> Result<Vec<DCAStrategy>>;
    
    /// Lấy số lượng token cho mỗi lần mua
    fn get_token_amount(&self, token_address: &str) -> Result<Option<U256>>;
    
    /// Lấy khoảng thời gian giữa các lần mua
    fn get_interval(&self, token_address: &str) -> Result<Option<DCAInterval>>;
    
    /// Lấy giới hạn giá
    fn get_price_limit(&self, token_address: &str) -> Result<Option<U256>>;
    
    /// Lấy thời điểm bắt đầu
    fn get_start_time(&self, token_address: &str) -> Result<Option<u64>>;
    
    /// Lấy thời điểm kết thúc
    fn get_end_time(&self, token_address: &str) -> Result<Option<u64>>;
    
    /// Lấy số lần thực hiện
    fn get_execution_count(&self, token_address: &str) -> Result<u32>;
    
    /// Lấy số lần còn lại
    fn get_remaining_count(&self, token_address: &str) -> Result<Option<u32>>;
    
    /// Lấy trạng thái
    fn get_status(&self, token_address: &str) -> Result<Option<String>>;
    
    /// Lấy lỗi nếu có
    fn get_error(&self, token_address: &str) -> Result<Option<String>>;
}

/// Cấu trúc Flashbots Bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundle {
    /// Hash của bundle
    pub hash: H256,
    /// Các giao dịch trong bundle
    pub transactions: Vec<Transaction>,
    /// Block target
    pub block_number: u64,
    /// Giá gas tối thiểu
    pub min_gas_price: U256,
    /// Giá trị tối thiểu
    pub min_value: U256,
    /// Thời gian tạo
    pub timestamp: u64,
}

/// Cấu trúc TWAP Config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TWAPConfig {
    /// Kích thước cửa sổ thời gian
    pub time_window: Duration,
    /// Số mẫu tối thiểu
    pub min_samples: usize,
    /// Khoảng thời gian cập nhật
    pub update_interval: Duration,
}

/// Cấu trúc Monte Carlo Config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    /// Số lần mô phỏng
    pub num_simulations: usize,
    /// Số bước thời gian
    pub time_steps: usize,
    /// Độ biến động
    pub volatility: f64,
    /// Lãi suất phi rủi ro
    pub risk_free_rate: f64,
    /// Thời gian mô phỏng
    pub simulation_time: Duration,
}

/// Cấu trúc Monte Carlo Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    /// Địa chỉ token
    pub token_address: String,
    /// Giá kỳ vọng
    pub expected_price: f64,
    /// Độ lệch chuẩn giá
    pub price_std_dev: f64,
    /// Các đường mô phỏng
    pub simulated_paths: Vec<Vec<f64>>,
    /// Xác suất lợi nhuận
    pub profit_probability: f64,
    /// Xác suất lỗ
    pub loss_probability: f64,
    /// Giá trị VaR
    pub var: f64,
    /// Giá trị CVaR
    pub cvar: f64,
    /// Số bước thời gian
    pub time_steps: u64,
}

/// Cấu trúc DCA Strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DCAStrategy {
    /// Địa chỉ token
    pub token_address: String,
    /// Số lượng token mỗi lần mua
    pub amount: U256,
    /// Khoảng thời gian giữa các lần mua
    pub interval: DCAInterval,
    /// Giá tối đa
    pub max_price: Option<U256>,
    /// Thời gian thực hiện cuối cùng
    pub last_execution: Option<u64>,
    /// Trạng thái kích hoạt
    pub enabled: bool,
}

/// Enum khoảng thời gian DCA
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DCAInterval {
    /// Theo giờ
    Hourly(u64),
    /// Theo ngày
    Daily(u64),
    /// Theo tuần
    Weekly(u64),
    /// Theo tháng
    Monthly(u64),
}

/// Cấu trúc MempoolTransaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolTransaction {
    /// Hash của giao dịch
    pub hash: H256,
    /// Địa chỉ người gửi
    pub from: Address,
    /// Địa chỉ người nhận
    pub to: Option<Address>,
    /// Giá trị giao dịch
    pub value: U256,
    /// Giá gas
    pub gas_price: U256,
    /// Giới hạn gas
    pub gas_limit: U256,
    /// Dữ liệu giao dịch
    pub data: Bytes,
    /// Nonce
    pub nonce: U256,
    /// Thời gian tạo
    pub timestamp: u64,
    /// Loại giao dịch
    pub tx_type: TransactionType,
    /// Trạng thái giao dịch
    pub status: TransactionStatus,
}

/// Enum loại giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionType {
    /// Giao dịch thông thường
    Normal,
    /// Giao dịch swap
    Swap,
    /// Giao dịch approve
    Approve,
    /// Giao dịch transfer
    Transfer,
    /// Giao dịch khác
    Other,
}

/// Enum trạng thái giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Đang chờ
    Pending,
    /// Đã được đưa vào block
    Included,
    /// Đã bị thay thế
    Replaced,
    /// Đã bị hủy
    Dropped,
}

/// Cấu trúc TokenRiskAnalysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRiskAnalysis {
    /// Địa chỉ token
    pub token_address: String,
    /// Điểm rủi ro
    pub risk_score: f64,
    /// Có phải honeypot không
    pub is_honeypot: bool,
    /// Đánh giá bảo mật contract
    pub contract_security: ContractSecurity,
    /// Các cảnh báo
    pub warnings: Vec<String>,
    /// Các lỗi
    pub errors: Vec<String>,
    /// Thời gian phân tích
    pub timestamp: u64,
}

/// Enum đánh giá bảo mật contract
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContractSecurity {
    /// Bảo mật cao
    High,
    /// Bảo mật trung bình
    Medium,
    /// Bảo mật thấp
    Low,
    /// Không an toàn
    Unsafe,
}

/// Cấu trúc GasOptimizerConfig
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasOptimizerConfig {
    /// Giá gas tối thiểu
    pub min_gas_price: U256,
    /// Giá gas tối đa
    pub max_gas_price: U256,
    /// Bước tăng giá gas
    pub gas_price_step: U256,
    /// Số lần thử tối đa
    pub max_retries: u32,
    /// Thời gian chờ giữa các lần thử
    pub retry_delay: Duration,
    /// Sử dụng EIP-1559
    pub use_eip1559: bool,
    /// Hệ số ưu tiên
    pub priority_fee_multiplier: f64,
}

/// Cấu trúc TokenStatus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatus {
    /// Địa chỉ token
    pub token_address: String,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số thập phân
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: U256,
    /// Giá hiện tại
    pub current_price: Option<f64>,
    /// Khối lượng giao dịch 24h
    pub volume_24h: Option<f64>,
    /// Thay đổi giá 24h
    pub price_change_24h: Option<f64>,
    /// Số người nắm giữ
    pub holders: Option<u64>,
    /// Trạng thái niêm yết
    pub is_listed: bool,
    /// Thời gian cập nhật
    pub last_updated: u64,
}

/// Cấu trúc TokenPriceAlert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceAlert {
    /// Địa chỉ token
    pub token_address: String,
    /// Loại cảnh báo
    pub alert_type: PriceAlertType,
    /// Giá mục tiêu
    pub target_price: f64,
    /// Đã kích hoạt chưa
    pub triggered: bool,
    /// Thời gian tạo
    pub created_at: u64,
    /// Thời gian kích hoạt
    pub triggered_at: Option<u64>,
}

/// Enum loại cảnh báo giá
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceAlertType {
    /// Giá tăng lên
    PriceUp,
    /// Giá giảm xuống
    PriceDown,
    /// Giá đạt mức
    PriceReached,
    /// Giá vượt mức
    PriceExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeType {
    Buy,
    Sell,
    Swap,
    Approve,
    Transfer
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AISuggestionType {
    Buy,
    Sell,
    Hold,
    StopLoss,
    TakeProfit
}

#[derive(Default)]
pub struct Callbacks {
    pub on_trade: Option<Box<dyn Fn(TradeType, String, f64) + Send + Sync>>,
    pub on_error: Option<Box<dyn Fn(String) + Send + Sync>>,
    pub on_ai_suggestion: Option<Box<dyn Fn(AISuggestionType, String, f64) + Send + Sync>>,
}

// ... existing code ...
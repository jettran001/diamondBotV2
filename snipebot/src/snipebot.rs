use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    signers::{LocalWallet, Signer},
    types::{Address, TransactionRequest, U256, H256, TransactionReceipt},
    utils as ethers_utils,
    contract::Contract
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use log::{info, error, warn, debug};
use super::config::Config;
use super::storage::Storage;
use diamond_wallet::{WalletManager, WalletInfo, SafeWalletView};
use std::time::SystemTime;
use std::sync::Mutex;
use crate::chain_adapters::base::ChainAdapterEnum;
use crate::trade::trade_logic::{TradeManager, TradeConfig, TradeResult};
use crate::risk_analyzer::{RiskAnalyzer, BasicRiskAnalyzer};
use crate::gas_optimizer::GasOptimizer;
use crate::token_status::{TokenStatusTracker, TokenStatus, TokenPriceAlert, TokenSafetyLevel};
use crate::mempool::{MempoolWatcher, MempoolTracker};
use std::sync::mpsc;
use crate::user_subscription::{SubscriptionLevel, SubscriptionTradeConfig};
use crate::error::{TransactionError, classify_blockchain_error, get_recovery_info};
use uuid::Uuid;
use std::collections::HashMap;
use crate::abi_utils;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use crate::utils;
use crate::types::{ServiceMessage, Subscription, SubscriptionLevel, TokenInfo};
use crate::module_manager::ModuleManager;
use crate::health_monitor::HealthMonitor;
use crate::trade_executor::TradeExecutor;
use crate::subscription_manager::SubscriptionManager;
use crate::mempool::mempool_monitor::MempoolMonitor;
use crate::ai::ai_coordinator::AICoordinator;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SnipeConfig {
    pub token_address: Address,
    pub amount_in: U256,
    pub amount_out_min: U256,
    pub deadline: u64,
    pub slippage: f64,
    pub gas_price: U256,
    pub gas_limit: U256,
    pub chain_id: u64,
    pub max_retries: u32,
    pub retry_delay: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipeResult {
    pub transaction_hash: H256,
    pub token_address: Address,
    pub amount_in: U256,
    pub amount_out: U256,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YellowTokenStrategy {
    pub buy_on_large_orders: bool,
    pub min_large_order_usd: f64,
    pub sell_after_minutes: u64,
    pub take_profit_percent: f64,
    pub stop_loss_percent: f64,
    pub use_sandwich_mode: bool,
    pub use_mempool_data: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GreenTokenStrategy {
    pub front_run_orders: bool,
    pub min_order_size_usd: f64,
    pub use_trailing_stop: bool,
    pub trailing_stop_percent: f64,
    pub take_profit_percent: f64,
    pub continue_buying_threshold_usd: f64,
    pub use_sandwich_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTradeConfig {
    pub enabled: bool,
    pub reserve_percent: f64,
    pub red_token_strategy: bool, // Luôn false - không giao dịch token đỏ
    pub yellow_token_strategy: YellowTokenStrategy,
    pub green_token_strategy: GreenTokenStrategy,
    pub sell_before_large_sells: bool,
    pub min_large_sell_usd: f64,
    pub rebuy_after_mempool_activity: bool,
    pub use_mev_strategies: bool,
    pub arbitrage_min_profit_percent: f64,
    pub max_gas_boost_percent: u64,
    pub ai_confidence_threshold: f64,
    pub cycle_interval_seconds: u64,
    pub auto_tuning_enabled: bool,
}

impl Default for AutoTradeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            reserve_percent: 25.0,
            red_token_strategy: false,
            yellow_token_strategy: YellowTokenStrategy {
                buy_on_large_orders: true,
                min_large_order_usd: 5000.0,
                sell_after_minutes: 5,
                take_profit_percent: 25.0,
                stop_loss_percent: 10.0,
                use_sandwich_mode: true,
                use_mempool_data: true,
            },
            green_token_strategy: GreenTokenStrategy {
                front_run_orders: true,
                min_order_size_usd: 10000.0,
                use_trailing_stop: true,
                trailing_stop_percent: 15.0,
                take_profit_percent: 30.0,
                continue_buying_threshold_usd: 10000.0,
                use_sandwich_mode: true,
            },
            sell_before_large_sells: true,
            min_large_sell_usd: 1000.0,
            rebuy_after_mempool_activity: true,
            use_mev_strategies: true,
            arbitrage_min_profit_percent: 1.0,
            max_gas_boost_percent: 200,
            ai_confidence_threshold: 0.6,
            cycle_interval_seconds: 3600,
            auto_tuning_enabled: true,
        }
    }
}

pub struct SnipeBot {
    config: Config,
    storage: Arc<Storage>,
    chain_adapter: ChainAdapterEnum,
    wallet_manager: Arc<Mutex<WalletManager>>,
    current_wallet_info: Option<WalletInfo>,
    trade_manager: RwLock<Option<Arc<Mutex<TradeManager<ChainAdapterEnum>>>>>,
    risk_analyzer: Option<Arc<dyn RiskAnalyzer>>,
    gas_optimizer: Option<Arc<GasOptimizer>>,
    token_status_tracker: RwLock<Option<Arc<Mutex<TokenStatusTracker>>>>,
    bot_mode: BotMode,
    mempool_watcher: Option<Arc<Mutex<MempoolWatcher>>>,
    auto_trade_config: Option<AutoTradeConfig>,
    subscription_config: SubscriptionTradeConfig,
    current_user_level: SubscriptionLevel,
    current_user: Option<WalletInfo>,
    current_wallet_address: Option<String>,
    status_update_sender: Option<tokio::sync::mpsc::Sender<StatusUpdate>>,
    status_update_receiver: Option<tokio::sync::mpsc::Receiver<StatusUpdate>>,
    mempool_tracker: Option<Arc<Mutex<MempoolTracker>>>,
    last_token_tracker_lock_time: AtomicU64,
    last_trade_manager_lock_time: AtomicU64,
    ai_module: Arc<RwLock<Option<AIModule>>>,
    auto_tuner: Arc<RwLock<AutoTuner>>,
    task_handles: RwLock<HashMap<String, JoinHandle<()>>>,
    
    module_manager: ModuleManager,
    health_monitor: HealthMonitor,
    trade_executor: TradeExecutor,
    subscription_manager: SubscriptionManager,
    
    mempool_monitor: Option<Arc<Mutex<MempoolMonitor>>>,
    ai_coordinator: Option<Arc<Mutex<AICoordinator>>>,
}

// Định nghĩa message cho channel
#[derive(Debug, Clone)]
pub enum StatusUpdate {
    TokenUpdate { token_address: String, new_status: TokenStatus },
    PriceAlert { token_address: String, alert: TokenPriceAlert },
    MempoolTransaction { tx_hash: String, token_address: Option<String> },
    RiskUpdate { token_address: String, risk_level: TokenSafetyLevel },
    TradeResult { result: TradeResult },
}

// Enum để xác định nguồn gây ra deadlock
#[derive(Debug, Clone, Copy)]
pub enum DeadlockSource {
    TokenTracker,
    TradeManager,
    MempoolWatcher,
    AIModule,
    MonteEquilibrium,
}

impl SnipeBot {
    pub async fn new(
        config: Config, 
        storage: Arc<Storage>,
        chain_adapter: ChainAdapterEnum
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Không cần tạo chain_adapter mới, sử dụng tham số đã được truyền vào
        
        // Khởi tạo wallet manager
        let wallet_manager = WalletManager::new(Arc::new(config.clone())).await?;
        let wallet_manager = Arc::new(Mutex::new(wallet_manager));
        
        // Lấy ví mặc định hoặc tạo mới nếu không có và auto_create_wallet=true
        let mut wallet_manager_lock = wallet_manager.lock().await;
        let wallet_info = match wallet_manager_lock.get_default_wallet() {
            Some(wallet) => wallet,
            None if config.auto_create_wallet => {
                info!("Không tìm thấy ví, tạo ví mới tự động");
                let new_wallet = wallet_manager_lock.create_new_wallet(None)?;
                wallet_manager_lock.save_wallets().await?;
                info!("Đã tạo ví mới: {}", new_wallet.address);
                new_wallet
            },
            None => {
                // Sử dụng private key từ config
                let wallet_info = WalletInfo {
                    address: "unknown".to_string(), // Sẽ được cập nhật sau
                    private_key: config.private_key.clone(),
                    mnemonic: None,
                    chain_id: config.chain_id,
                    created_at: 0,
                    last_used: 0,
                };
                
                // Tạo ví từ private key
                let wallet = ethers::signers::LocalWallet::from_str(&config.private_key)?
                    .with_chain_id(config.chain_id);
                    
                // Cập nhật địa chỉ
                let mut wallet_info = wallet_info;
                wallet_info.address = format!("{:?}", wallet.address());
                
                wallet_info
            }
        };
        
        // Tạo ví và thiết lập vào adapter
        let wallet = WalletManager::to_local_wallet(&wallet_info)?;
        chain_adapter.set_wallet(wallet);
        
        // Giải phóng mutex
        drop(wallet_manager_lock);
        
        // Tạo channel cho status updates
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        
        // Tạo instance mới với bot_mode mặc định là Manual
        let mut bot = Self {
            config,
            storage,
            chain_adapter,
            wallet_manager,
            current_wallet_info: Some(wallet_info),
            bot_mode: BotMode::Manual,
            risk_analyzer: None,
            token_status_tracker: RwLock::new(Some(Arc::new(Mutex::new(TokenStatusTracker::new(
                Arc::new(self.chain_adapter.get_provider().clone()),
                vec![self.config.router_address.clone()],
                vec![self.config.router_address.clone()],
                self.config.weth_address.clone(),
            )?)))),
            mempool_watcher: None,
            auto_trade_config: None,
            subscription_config: SubscriptionTradeConfig::default(),
            current_user_level: SubscriptionLevel::Free,
            current_user: None,
            current_wallet_address: None,
            status_update_sender: Some(tx),
            status_update_receiver: Some(rx),
            mempool_tracker: None,
            last_token_tracker_lock_time: AtomicU64::new(0),
            last_trade_manager_lock_time: AtomicU64::new(0),
            ai_module: Arc::new(RwLock::new(None)),
            auto_tuner: Arc::new(RwLock::new(AutoTuner::new())),
            task_handles: RwLock::new(HashMap::new()),
            
            module_manager: ModuleManager::new(),
            health_monitor: HealthMonitor::new(),
            trade_executor: TradeExecutor::new(),
            subscription_manager: SubscriptionManager::new(),
            
            mempool_monitor: None,
            ai_coordinator: None,
        };
        
        // Khởi tạo các thành phần cần thiết cho cả hai chế độ
        bot.initialize_components().await?;
        
        Ok(bot)
    }
    
    // Thêm phương thức để thay đổi chain
    pub async fn switch_chain(&mut self, chain_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut new_adapter = create_chain_adapter(chain_name).await?;
        
        // Chuyển ví hiện tại sang adapter mới
        if let Some(wallet_info) = &self.current_wallet_info {
            let wallet = WalletManager::to_local_wallet(&wallet_info)?;
            new_adapter.set_wallet(wallet);
        }
        
        // Cập nhật adapter
        self.chain_adapter = new_adapter;
        
        Ok(())
    }
    
    // Thêm phương thức để thay đổi ví đang sử dụng
    pub async fn switch_wallet(&mut self, address: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Xác thực địa chỉ ví trước khi tiếp tục
        self.validate_wallet_address(address)?;
        
        let wallet_manager = self.wallet_manager.lock().await;
        let wallet_info = wallet_manager.get_wallet_by_address(address)
            .ok_or_else(|| format!("Không tìm thấy ví với địa chỉ {}", address))?;
        
        // Tạo bản sao để tránh giữ lock quá lâu
        let wallet_info = wallet_info.clone();
        drop(wallet_manager);
        
        // Cập nhật ví hiện tại
        self.current_wallet_info = Some(wallet_info.clone());
        self.current_wallet_address = Some(address.to_string());
        
        // Cập nhật wallet cho adapter
        let wallet = self.to_local_wallet(&wallet_info)?;
        self.chain_adapter.set_wallet(wallet);
        
        Ok(())
    }
    
    // Tạo ví mới và chuyển đến ví đó
    pub async fn create_and_switch_wallet(&mut self) -> Result<SafeWalletView, Box<dyn std::error::Error>> {
        let mut wallet_manager = self.wallet_manager.lock().await;
        
        // Tạo ví mới (trả về view an toàn)
        let wallet_view = wallet_manager.create_new_wallet(None)?;
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        // Tìm ví thực để lấy thông tin
        let wallet_info = wallet_manager.get_wallet_by_address(&wallet_view.address)
            .ok_or_else(|| anyhow!("Không tìm thấy ví vừa tạo"))?;
        
        // Tạo ví từ thông tin
        let wallet = wallet_manager.to_local_wallet(wallet_info)?;
        self.chain_adapter.set_wallet(wallet);
        
        // Lưu địa chỉ hiện tại thay vì toàn bộ WalletInfo
        self.current_wallet_address = Some(wallet_view.address.clone());
        
        // Giải phóng mutex sớm
        drop(wallet_manager);
        
        Ok(wallet_view)
    }
    
    // Lấy danh sách ví (trả về danh sách an toàn)
    pub async fn get_wallet_list(&self) -> Result<Vec<SafeWalletView>, Box<dyn std::error::Error>> {
        let wallet_manager = self.wallet_manager.lock().await;
        Ok(wallet_manager.get_wallet_list())
    }
    
    // Lấy địa chỉ ví hiện tại
    pub fn get_current_wallet_address(&self) -> String {
        if let Some(wallet_info) = &self.current_wallet_info {
            wallet_info.address.clone()
        } else {
            match self.chain_adapter.get_wallet() {
                Some(wallet) => format!("{:?}", wallet.address()),
                None => "0x0000000000000000000000000000000000000000".to_string(),
            }
        }
    }
    
    // Import ví từ private key
    pub async fn import_wallet_from_private_key(&self, private_key: &str) -> Result<WalletInfo, Box<dyn std::error::Error>> {
        // Xác thực private key trước khi tiếp tục
        self.validate_private_key(private_key)?;
        
        let mut wallet_manager = self.wallet_manager.lock().await;
        let wallet_info = wallet_manager.import_from_private_key(private_key)?;
        
        // Mã hóa private key
        let mut wallet_info_mut = wallet_info.clone();
        wallet_info_mut.encrypt_private_key(private_key, &wallet_manager.encryption_key)?;
        
        // Cập nhật ví trong danh sách
        for wallet in &mut wallet_manager.wallets {
            if wallet.address == wallet_info.address {
                *wallet = wallet_info_mut.clone();
                break;
            }
        }
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallet_info_mut)
    }
    
    // Import ví từ mnemonic
    pub async fn import_wallet_from_mnemonic(&self, mnemonic: &str, passphrase: Option<&str>) -> Result<WalletInfo, Box<dyn std::error::Error>> {
        // Xác thực mnemonic trước khi tiếp tục
        self.validate_mnemonic(mnemonic)?;
        
        let mut wallet_manager = self.wallet_manager.lock().await;
        let wallet_info = wallet_manager.import_from_mnemonic(mnemonic, passphrase)?;
        
        // Mã hóa mnemonic
        let mut wallet_info_mut = wallet_info.clone();
        wallet_info_mut.encrypt_mnemonic(mnemonic, &wallet_manager.encryption_key)?;
        
        // Cập nhật ví trong danh sách
        for wallet in &mut wallet_manager.wallets {
            if wallet.address == wallet_info.address {
                *wallet = wallet_info_mut.clone();
                break;
            }
        }
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallet_info_mut)
    }
    
    // Lấy số dư token
    pub async fn get_token_balance(&self, token_address: &str) -> Result<U256, Box<dyn std::error::Error>> {
        let token_address = Address::from_str(token_address)?;
        let wallet_address = self.get_current_wallet_address();
        
        self.chain_adapter.get_token_balance(token_address.to_string().as_str(), &wallet_address)
    }
    
    // Lấy số dư native token (ETH, BNB, etc.)
    pub async fn get_native_balance(&self) -> Result<U256, Box<dyn std::error::Error>> {
        let wallet_address = self.get_current_wallet_address();
        self.chain_adapter.get_native_balance(wallet_address.as_str())
    }
    
    // Phê duyệt token cho router
    pub async fn approve_token(&self, token_address: &str, router: &str, amount: U256) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let token_address = Address::from_str(token_address)?;
        let router_address = Address::from_str(router)?;
        let wallet_address = self.get_current_wallet_address();
        
        self.chain_adapter.approve_token(token_address.to_string().as_str(), router_address.to_string().as_str(), amount)
    }
    
    // Thực hiện swap/snipe
    pub async fn snipe(
        &self,
        token_info: &TokenInfo,
        amount_in: U256,
        snipe_cfg: &SnipeConfig,
    ) -> Result<SnipeResult, Box<dyn std::error::Error>> {
        let token_address = &token_info.address;
        info!("Bắt đầu snipe token {} với {} ETH", token_address, ethers::utils::format_ether(amount_in));
        
        // Ước tính amount_out
        let path = self.chain_adapter.get_native_to_token_path(token_address);
        let amounts = self.chain_adapter.get_amounts_out(amount_in, path).await?;
        let amount_out = amounts.last().ok_or("Không thể ước tính amount_out")?;
        
        // Tính amount_out_min với slippage
        let slippage_factor = (1000.0 - snipe_cfg.slippage * 10.0) / 1000.0;
        let amount_out_min = (amount_out.as_u128() as f64 * slippage_factor) as u128;
        let amount_out_min = U256::from(amount_out_min);
        
        // Thiết lập deadline
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let deadline = now + snipe_cfg.timeout;
        
        // Lấy địa chỉ ví hiện tại
        let wallet_address = self.get_current_wallet_address()?;
        
        // Sử dụng retry_blockchain_tx thay vì tự xử lý retry
        let operation_name = format!("snipe_token_{}", token_address);
        
        let result = retry_blockchain_tx(
            move || {
                let chain_adapter = self.chain_adapter.clone();
                let token_address = token_address.clone();
                let amount_in = amount_in;
                let amount_out_min = amount_out_min;
                let wallet_address = wallet_address.clone();
                let deadline = deadline;
                let gas_limit = snipe_cfg.gas_limit;
                let gas_price_future = async {
                    if let Some(optimizer) = chain_adapter.get_gas_optimizer() {
                        match optimizer.get_optimal_gas_price(&*chain_adapter).await {
                            Ok(price) => {
                                info!("Sử dụng gas price tối ưu: {} Gwei", price.as_u128() as f64 / 1_000_000_000.0);
                                price
                            },
                            Err(e) => {
                                warn!("Không thể tối ưu gas price: {}, sử dụng giá mặc định", e);
                                U256::from(snipe_cfg.gas_price)
                            }
                        }
                    } else {
                        U256::from(snipe_cfg.gas_price)
                    }
                };
                
                async move {
                    // Lấy giá gas tối ưu
                    let gas_price = gas_price_future.await;
                    
                    // Thực hiện giao dịch swap
                    let receipt = chain_adapter.swap_exact_eth_for_tokens(
                        &token_address,
                        amount_in,
                        amount_out_min,
                        &wallet_address,
                        deadline,
                        Some(gas_limit),
                        Some(gas_price),
                    ).await?;
                    
                    if let Some(r) = receipt {
                        Ok(r)
                    } else {
                        Err(anyhow::anyhow!("Không nhận được transaction receipt"))
                    }
                }
            },
            &operation_name,
            Some(Box::new(move |attempt: u32| {
                // Điều chỉnh gas và slippage cho mỗi lần retry
                if attempt > 1 {
                    // Tăng gas các tham số theo số lần thử
                    let gas_multiplier = 1.0 + (attempt as f64 * 0.1); // +10% mỗi lần
                    snipe_cfg.gas_limit = (snipe_cfg.gas_limit as f64 * gas_multiplier) as u64;
                    snipe_cfg.gas_price = (snipe_cfg.gas_price as f64 * gas_multiplier) as u64;
                    
                    // Điều chỉnh slippage nếu cần
                    if attempt > 2 {
                        // Tăng slippage thêm 0.5% mỗi lần sau lần thứ 2
                        snipe_cfg.slippage += 0.5;
                        
                        // Tính lại amount_out_min
                        let new_slippage_factor = (1000.0 - snipe_cfg.slippage * 10.0) / 1000.0;
                        amount_out_min = (amount_out.as_u128() as f64 * new_slippage_factor) as u128;
                        amount_out_min = U256::from(amount_out_min);
                    }
                }
            })),
            None // Sử dụng config mặc định
        ).await;
        
        match result {
            Ok(receipt) => {
                // Xử lý kết quả thành công
                let tx_hash = receipt.transaction_hash.to_string();
                info!("Snipe thành công: {}", tx_hash);
                
                // Tạo kết quả
                let timestamp = match SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(duration) => duration.as_secs(),
                    Err(_) => {
                        warn!("Lỗi khi lấy thời gian hệ thống, sử dụng timestamp mặc định");
                        0 // Sử dụng giá trị mặc định cho timestamp
                    }
                };
                
                let result = SnipeResult {
                    transaction_hash: Some(tx_hash),
                    success: true,
                    token_address: token_address.clone(),
                    amount_in: ethers::utils::format_ether(amount_in),
                    estimated_amount_out: Some(amount_out.to_string()),
                    error: None,
                    timestamp,
                };
                
                // Lưu kết quả vào storage
                self.storage.add_transaction(result.clone().into());
                
                Ok(result)
            },
            Err(e) => {
                // Xử lý lỗi
                error!("Lỗi khi snipe token {}: {}", token_address, e);
                
                // Tạo kết quả thất bại
                let timestamp = match SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(duration) => duration.as_secs(),
                    Err(_) => {
                        warn!("Lỗi khi lấy thời gian hệ thống, sử dụng timestamp mặc định");
                        0 // Sử dụng giá trị mặc định cho timestamp
                    }
                };
                
                let result = SnipeResult {
                    transaction_hash: None,
                    success: false,
                    token_address: token_address.clone(),
                    amount_in: ethers::utils::format_ether(amount_in),
                    estimated_amount_out: Some(amount_out.to_string()),
                    error: Some(e.to_string()),
                    timestamp,
                };
                
                // Lưu kết quả thất bại vào storage
                self.storage.add_transaction(result.clone().into());
                
                Err(Box::new(e))
            }
        }
    }
    
    // Kiểm tra token đã được phê duyệt chưa
    pub async fn check_allowance(&self, token_address: &str, router: &str) -> Result<U256, Box<dyn std::error::Error>> {
        let token_address = Address::from_str(token_address)?;
        let router_address = Address::from_str(router)?;
        let wallet_address = self.get_current_wallet_address();
        
        let token_contract = Contract::new(
            token_address,
            self.chain_adapter.get_token_abi().clone(),
            Arc::new(self.chain_adapter.get_provider().clone())
        );
        
        let allowance: U256 = token_contract
            .method("allowance", (wallet_address, router_address))?
            .call()
            .await?;
            
        Ok(allowance)
    }
    
    // Thêm chức năng theo dõi token mới
    pub async fn monitor_tokens(&self, tokens: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        info!("Bắt đầu theo dõi {} tokens", tokens.len());
        
        for token_address in tokens {
            // Để đơn giản, chỉ in ra log
            info!("Đang theo dõi token: {}", token_address);
        }
        
        Ok(())
    }
    
    // Tạo nhiều ví HD từ một mnemonic
    pub async fn create_hd_wallets(&mut self, mnemonic: &str, count: usize, passphrase: Option<&str>) -> Result<Vec<WalletInfo>, Box<dyn std::error::Error>> {
        let mut wallet_manager = self.wallet_manager.lock().await;
        
        // Tạo ví HD
        let wallets = wallet_manager.create_hd_wallets(mnemonic, count, passphrase)?;
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallets)
    }
    
    // Xóa ví theo địa chỉ
    pub async fn remove_wallet(&mut self, address: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let mut wallet_manager = self.wallet_manager.lock().await;
        
        // Xóa ví
        let result = wallet_manager.remove_wallet(address);
        
        // Lưu thay đổi nếu có xóa
        if result {
            wallet_manager.save_wallets().await?;
        }
        
        Ok(result)
    }
    
    // Cập nhật số dư cho tất cả các ví
    pub async fn update_wallet_balances(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut wallet_manager = self.wallet_manager.lock().await;
        
        // Cập nhật số dư
        wallet_manager.update_balances(self.chain_adapter.get_provider().clone()).await?;
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(())
    }
    
    // Phương thức khởi tạo chế độ Auto
    async fn initialize_auto_mode(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Khởi tạo các thành phần cần thiết
        
        // Khởi tạo RiskAnalyzer
        let risk_analyzer = Arc::new(BasicRiskAnalyzer::new(
            chain_adapter.clone(),
            weth_address.clone()
        )?);
        self.risk_analyzer = Some(risk_analyzer.clone());
        
        // Khởi tạo GasOptimizer
        let gas_optimizer = Arc::new(GasOptimizer::new(
            U256::from(self.config.default_gas_price * 3), 
            50
        ));
        self.gas_optimizer = Some(gas_optimizer.clone());
        
        // Khởi tạo TokenStatusTracker TRƯỚC khi tạo TradeManager
        let token_status_tracker = Arc::new(Mutex::new(TokenStatusTracker::new(
            Arc::new(self.chain_adapter.get_provider().clone()),
            vec![self.config.router_address.clone()],
            vec![self.config.router_address.clone()],
            self.config.weth_address.clone(),
        )?));
        self.token_status_tracker = RwLock::new(Some(token_status_tracker.clone()));
        
        // Tạo cấu hình cho TradeManager
        let trade_config = TradeConfig {
            // Các thông số khác...
            max_slippage: self.config.default_slippage,
            risk_tolerance: self.config.max_risk_score,
            max_gas_price: self.config.default_gas_price * 3,
            auto_approve: true,
            use_flashbots: false,
            retry_count: self.config.auto_retry_count,
            retry_delay_ms: 1000,
            min_liquidity: 10000.0,
            min_market_cap: 50000.0,
            max_buy_tax: 10.0,
            max_sell_tax: 10.0,
            default_amount: "0.1".to_string(),
            default_sell_percent: 100,
            emergency_sell_gas_multiplier: 1.5,
        };
        
        // Khởi tạo TradeManager
        let mut trade_manager = TradeManager::new(Arc::new(self.chain_adapter.clone()), trade_config);
        
        // Thiết lập các thành phần cho TradeManager
        trade_manager.set_risk_analyzer(risk_analyzer.clone());
        trade_manager.set_gas_optimizer(gas_optimizer.clone());
        trade_manager.set_token_status_tracker(token_status_tracker.clone());
        
        // Lưu TradeManager
        self.trade_manager = RwLock::new(Some(Arc::new(Mutex::new(trade_manager))));
        
        // Khởi động các dịch vụ theo dõi
        self.start_monitoring_services().await?;
        
        // Bắt đầu xử lý các status update
        self.process_status_updates().await;
        
        Ok(())
    }
    
    // Khởi động các dịch vụ theo dõi
    async fn start_monitoring_services(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Chỉ chạy nếu là Auto mode
        if self.bot_mode != BotMode::Auto {
            return Ok(());
        }
        
        // Khởi động theo dõi token status nếu có
        if let Some(token_tracker) = &self.token_status_tracker {
            let token_tracker_weak = Arc::downgrade(token_tracker); // Sử dụng Weak thay vì Arc mạnh
            let trade_manager_weak = self.trade_manager.as_ref().map(Arc::downgrade); // Weak reference
            
            // Task theo dõi token status
            tokio::spawn(async move {
                let update_interval = tokio::time::Duration::from_secs(30);
                let mut interval = tokio::time::interval(update_interval);
                
                // Task ID để theo dõi
                let task_id = Uuid::new_v4().to_string();
                debug!("Khởi động task theo dõi token status với ID: {}", task_id);
                
                let mut last_success_time = std::time::Instant::now();
                let mut consecutive_failures = 0;
                
                loop {
                    interval.tick().await;
                    
                    // Chỉ tiếp tục nếu có thể nâng cấp weak reference
                    let token_tracker_arc = match token_tracker_weak.upgrade() {
                        Some(arc) => arc,
                        None => {
                            debug!("Task {} kết thúc: token_tracker đã bị giải phóng", task_id);
                            break;
                        }
                    };
                    
                    // Sử dụng try_lock với timeout để tránh deadlock
                    let token_tracker_result = tokio::time::timeout(
                        tokio::time::Duration::from_secs(5),
                        async {
                            token_tracker_arc.try_lock()
                        }
                    ).await;
                    
                    match token_tracker_result {
                        Ok(Ok(mut tracker)) => {
                            // Cập nhật trạng thái token
                            match tracker.update_all_tokens().await {
                                Ok(_) => {
                                    last_success_time = std::time::Instant::now();
                                    consecutive_failures = 0;
                                    debug!("Task {}: Cập nhật token status thành công", task_id);
                                },
                                Err(e) => {
                                    consecutive_failures += 1;
                                    error!("Task {}: Lỗi khi cập nhật token status: {}", task_id, e);
                                }
                            }
                            
                            // Giải phóng lock ngay sau khi sử dụng xong
                            drop(tracker);
                            
                            // Đợi một khoảng thời gian nhỏ để giảm áp lực lên các resource
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        },
                        _ => {
                            consecutive_failures += 1;
                            debug!("Task {}: Không thể lấy token tracker lock, đang bận", task_id);
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                    
                    // Xử lý Trade Manager trong task riêng biệt
                    if let Some(trade_mgr_weak) = &trade_manager_weak {
                        if let Some(trade_mgr) = trade_mgr_weak.upgrade() {
                            // Tạo một task riêng biệt để tránh xung đột khóa
                            tokio::spawn(async move {
                                // Tạo một timeout để tránh deadlock
                                let trade_mgr_result = tokio::time::timeout(
                                    tokio::time::Duration::from_secs(5),
                                    async {
                                        trade_mgr.try_lock()
                                    }
                                ).await;
                                
                                match trade_mgr_result {
                                    Ok(Ok(trade_mgr)) => {
                                        // Xử lý theo thông tin token status
                                        if let Err(e) = trade_mgr.run_continuous_risk_monitoring().await {
                                            error!("Lỗi khi theo dõi rủi ro liên tục: {}", e);
                                        }
                                        
                                        // Tách thành một task con khác để tránh giữ khóa quá lâu
                                        let trade_mgr_clone = trade_mgr.clone();
                                        drop(trade_mgr); // Giải phóng khóa trước khi tạo task mới
                                        
                                        // Tạo một task con để thực hiện chiến lược thoát
                                        tokio::spawn(async move {
                                            if let Err(e) = trade_mgr_clone.execute_dynamic_exit_strategies().await {
                                                error!("Lỗi khi thực hiện chiến lược thoát: {}", e);
                                            }
                                        });
                                    },
                                    _ => {
                                        debug!("Không thể lấy trade manager lock, đang bận");
                                    }
                                }
                            });
                        }
                    }
                    
                    // Kiểm tra sức khỏe của task
                    if consecutive_failures > 5 {
                        error!("Task {}: Phát hiện quá nhiều lỗi liên tiếp ({}), đang tự khởi động lại", 
                               task_id, consecutive_failures);
                        consecutive_failures = 0;
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    
                    if last_success_time.elapsed() > std::time::Duration::from_secs(300) {
                        warn!("Task {}: Không có cập nhật thành công trong 5 phút, kiểm tra hệ thống", task_id);
                    }
                }
            });
        }
        
        // Khởi động theo dõi mempool trong task riêng biệt
        if let Some(mempool_watcher) = &self.mempool_watcher {
            let mempool_weak = Arc::downgrade(mempool_watcher); // Sử dụng Weak reference
            let trade_manager_weak = self.trade_manager.as_ref().map(Arc::downgrade); // Weak reference
            
            // Task riêng biệt cho mempool monitoring
            tokio::spawn(async move {
                let update_interval = tokio::time::Duration::from_secs(10); // Mempool cập nhật nhanh hơn
                let mut interval = tokio::time::interval(update_interval);
                
                // Task ID để theo dõi
                let task_id = Uuid::new_v4().to_string();
                debug!("Khởi động task theo dõi mempool với ID: {}", task_id);
                
                let mut last_success_time = std::time::Instant::now();
                let mut consecutive_failures = 0;
                
                loop {
                    interval.tick().await;
                    
                    // Chỉ tiếp tục nếu có thể nâng cấp weak reference
                    let mempool_arc = match mempool_weak.upgrade() {
                        Some(arc) => arc,
                        None => {
                            debug!("Task {} kết thúc: mempool_watcher đã bị giải phóng", task_id);
                            break;
                        }
                    };
                    
                    // Sử dụng try_lock với timeout để tránh deadlock
                    let mempool_result = tokio::time::timeout(
                        tokio::time::Duration::from_secs(3),
                        async {
                            mempool_arc.try_lock()
                        }
                    ).await;
                    
                    match mempool_result {
                        Ok(Ok(watcher)) => {
                            // Xử lý mempool
                            match watcher.process_pending_transactions().await {
                                Ok(_) => {
                                    last_success_time = std::time::Instant::now();
                                    consecutive_failures = 0;
                                    debug!("Task {}: Xử lý mempool thành công", task_id);
                                },
                                Err(e) => {
                                    consecutive_failures += 1;
                                    error!("Task {}: Lỗi khi xử lý mempool: {}", task_id, e);
                                }
                            }
                            
                            // Giải phóng lock ngay sau khi sử dụng xong
                            drop(watcher);
                            
                            // Đợi một khoảng thời gian nhỏ để giảm áp lực
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        },
                        _ => {
                            consecutive_failures += 1;
                            debug!("Task {}: Không thể lấy mempool watcher lock, đang bận", task_id);
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                    
                    // Xử lý các cơ hội từ mempool trong task riêng biệt
                    if let Some(trade_mgr_weak) = &trade_manager_weak {
                        if let Some(trade_mgr) = trade_mgr_weak.upgrade() {
                            // Tạo một task riêng biệt để tránh xung đột khóa
                            tokio::spawn(async move {
                                let trade_mgr_result = tokio::time::timeout(
                                    tokio::time::Duration::from_secs(3),
                                    async {
                                        trade_mgr.try_lock()
                                    }
                                ).await;
                                
                                if let Ok(Ok(trade_mgr)) = trade_mgr_result {
                                    if let Err(e) = trade_mgr.process_mempool_opportunities().await {
                                        error!("Lỗi khi xử lý cơ hội từ mempool: {}", e);
                                    }
                                    drop(trade_mgr);
                                }
                            });
                        }
                    }
                    
                    // Kiểm tra sức khỏe của task
                    if consecutive_failures > 5 {
                        error!("Task {}: Phát hiện quá nhiều lỗi liên tiếp ({}), đang tự khởi động lại", 
                               task_id, consecutive_failures);
                        consecutive_failures = 0;
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    
                    if last_success_time.elapsed() > std::time::Duration::from_secs(300) {
                        warn!("Task {}: Không có cập nhật thành công trong 5 phút, kiểm tra hệ thống", task_id);
                    }
                }
            });
        }
        
        // Khởi động task theo dõi HealthCheck riêng biệt
        let token_tracker_weak = self.token_status_tracker.as_ref().map(Arc::downgrade);
        let mempool_weak = self.mempool_watcher.as_ref().map(Arc::downgrade);
        
        tokio::spawn(async move {
            let health_interval = tokio::time::Duration::from_secs(60); // Kiểm tra mỗi phút
            let mut interval = tokio::time::interval(health_interval);
            
            loop {
                interval.tick().await;
                
                // Kiểm tra TokenStatusTracker
                if let Some(tracker_weak) = &token_tracker_weak {
                    if tracker_weak.strong_count() == 0 {
                        warn!("TokenStatusTracker đã bị giải phóng, hệ thống có thể không hoạt động đúng");
                    }
                }
                
                // Kiểm tra MempoolWatcher
                if let Some(mempool_weak) = &mempool_weak {
                    if mempool_weak.strong_count() == 0 {
                        warn!("MempoolWatcher đã bị giải phóng, hệ thống có thể không theo dõi được mempool");
                    }
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
        
        Ok(())
    }
    
    // Chuyển đổi chế độ Bot
    pub async fn switch_mode(&mut self, mode: BotMode) -> Result<(), Box<dyn std::error::Error>> {
        if self.bot_mode == mode {
            return Ok(());
        }
        
        info!("Chuyển chế độ bot từ {:?} sang {:?}", self.bot_mode, mode);
        self.bot_mode = mode;
        
        Ok(())
    }
    
    // Lấy chế độ Bot hiện tại
    pub fn get_mode(&self) -> BotMode {
        self.bot_mode.clone()
    }
    
    // Khởi tạo các thành phần cần thiết
    async fn initialize_components(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Khởi tạo RiskAnalyzer
        let provider = Arc::new(self.chain_adapter.get_provider().clone());
        let risk_analyzer = Arc::new(RiskAnalyzer::new(
            provider.clone(),
            vec![self.config.router_address.clone()],
            vec![self.config.router_address.clone()],
            self.config.weth_address.clone(),
        )?);
        
        self.risk_analyzer = Some(risk_analyzer);
        
        // Khởi tạo TokenStatusTracker
        let token_tracker = Arc::new(Mutex::new(TokenStatusTracker::new(
            provider.clone(),
            vec![self.config.router_address.clone()],
            vec![self.config.router_address.clone()],
            self.config.weth_address.clone(),
        )?));
        
        self.token_status_tracker = RwLock::new(Some(token_tracker));
        
        // Khởi tạo MempoolWatcher nếu có
        if let Some(service_tx) = self.get_service_tx() {
            let mempool_watcher = Arc::new(Mutex::new(MempoolWatcher::new(
                self.config.clone(),
                service_tx,
            )));
            
            self.mempool_watcher = Some(mempool_watcher);
        }
        
        // Sửa đoạn khởi tạo TradeManager
        if let Ok(mut trade_manager_lock) = self.trade_manager.write() {
            if trade_manager_lock.is_none() {
                info!("Khởi tạo TradeManager...");
                
                // Sử dụng timeout khi khởi tạo các components
                match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    async {
                        let chain_adapter = Arc::new(self.chain_adapter.clone());
                        let config = self.config.clone();
                        
                        // Tạo TradeManager với cấu hình phù hợp
                        let trade_config = trade::trade_logic::TradeConfig {
                            gas_limit: U256::from(400000),
                            gas_price: U256::from(5000000000u64),
                            slippage: 0.5,
                            timeout: 30,
                            auto_approve: true,
                            use_flashbots: config.advanced.use_flashbots,
                            emergency_sell_gas_multiplier: 1.5,
                            router_address: config.router_address.clone(),
                            wrapped_native_token: config.wrapped_native_token.clone(),
                            max_slippage: 2.0,
                            twap_window_size: 10,
                            twap_min_samples: 5,
                            twap_update_interval: 60,
                        };
                        
                        let trade_manager = TradeManager::new(chain_adapter, trade_config);
                        
                        // Thiết lập tất cả các trường cần thiết
                        
                        // Trả về manager đã khởi tạo
                        Arc::new(Mutex::new(trade_manager))
                    }
                ).await {
                    Ok(manager) => {
                        *trade_manager_lock = Some(manager);
                        info!("TradeManager đã khởi tạo thành công.");
                    },
                    Err(_) => {
                        error!("Timeout khi khởi tạo TradeManager.");
                        return Err("Timeout khi khởi tạo TradeManager.".into());
                    }
                }
            }
        }
        
        Ok(())
    }
    
    // Lấy service_tx (placeholder - cần thay đổi theo implementation thực tế)
    fn get_service_tx(&self) -> Option<mpsc::Sender<ServiceMessage>> {
        None // Placeholder
    }
    
    // Phân tích token và trả về TokenInfo với TokenStatus
    pub async fn analyze_token(&self, token_address: &str) -> Result<(TokenInfo, TokenStatus, TokenRiskAnalysis), Box<dyn std::error::Error>> {
        // Lấy thông tin cơ bản về token
        let token_info = self.chain_adapter.get_token_info(token_address).await?;
        
        // Lấy thông tin trạng thái token
        let mut token_status = self.get_token_status(token_address).await?;
        
        // Lấy phân tích rủi ro
        let risk_analysis = if let Some(analyzer) = &self.risk_analyzer {
            let token_addr = Address::from_str(token_address).map_err(|e| format!("Địa chỉ token không hợp lệ: {}", e))?;
            analyzer.analyze_token(token_addr).await?
        } else {
            return Err("Risk Analyzer chưa được khởi tạo".into());
        };
        
        // Cập nhật thông tin tax
        let tax_info = TaxInfo {
            buy_tax: 0.0, // Cần cập nhật từ risk_analysis nếu có
            sell_tax: 0.0, // Cần cập nhật từ risk_analysis nếu có
            transfer_tax: 0.0,
            min_hold_time: None,
        };
        token_status.tax_info = Some(tax_info);
        
        // Cập nhật mức độ an toàn
        if let Some(tracker) = &self.token_status_tracker {
            let mut tracker = match tracker.lock().await {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Lỗi khi lấy lock cho token_tracker: {}", e);
                    return Err(format!("Lỗi khi lấy lock cho token_tracker: {}", e).into());
                }
            };
            token_status.safety_level = tracker.classify_token(&token_status, Some(&risk_analysis));
        } else {
            // Phân loại mặc định nếu không có tracker
            token_status.safety_level = if risk_analysis.base.risk_score < 60.0 { 
                TokenSafetyLevel::Green 
            } else if risk_analysis.base.risk_score < 80.0 { 
                TokenSafetyLevel::Yellow 
            } else { 
                TokenSafetyLevel::Red 
            };
        }
        
        // Cập nhật các thông tin khác
        token_status.is_contract_verified = risk_analysis.is_verified;
        token_status.has_dangerous_functions = risk_analysis.dangerous_functions.len() > 0;
        token_status.dangerous_functions = risk_analysis.dangerous_functions.clone();
        
        // Kiểm tra thanh khoản đã khóa
        if let Some(analyzer) = &self.risk_analyzer {
            token_status.liquidity_locked = Some(analyzer.check_liquidity_locked(token_address).await?);
        }
        
        // Cập nhật số lượng giao dịch đang chờ
        if let Some(mempool) = &self.mempool_watcher {
            let watcher = mempool.lock().await;
            token_status.pending_tx_count = watcher.count_pending_txs_for_token(token_address).await;
        }
        
        Ok((token_info, token_status, risk_analysis))
    }
    
    // Lấy thông tin trạng thái token
    pub async fn get_token_status(&self, token_address: &str) -> Result<TokenStatus, Box<dyn std::error::Error>> {
        if let Some(tracker) = &self.token_status_tracker {
            // Sử dụng try_lock() thay vì lock().unwrap()
            match tracker.try_lock() {
                Ok(mut lock) => {
                    // Kiểm tra xem token đã được theo dõi chưa
                    if lock.get_token(token_address).is_none() {
                        // Nếu chưa, thêm vào danh sách theo dõi
                        lock.add_token(token_address).await?;
                    }
                    
                    // Cập nhật thông tin mới nhất
                    if let Some(status) = lock.get_token(token_address) {
                        // Bây giờ thêm phân loại token an toàn
                        let mut status = status.clone();
                        
                        // Phân tích rủi ro để cập nhật chỉ số an toàn
                        if let Some(analyzer) = &self.risk_analyzer {
                            let token_addr = Address::from_str(token_address).map_err(|e| format!("Địa chỉ token không hợp lệ: {}", e))?;
                            let risk_analysis = analyzer.analyze_token(token_addr).await?;
                            
                            let safety_level = if risk_analysis.base.risk_score < 60.0 {
                                TokenSafetyLevel::Green
                            } else if risk_analysis.base.risk_score < 80.0 {
                                TokenSafetyLevel::Yellow
                            } else {
                                TokenSafetyLevel::Red
                            };
                            
                            // Cập nhật thông tin an toàn vào token status
                            status.safety_emoji = safety_level.to_string();
                            status.has_dangerous_functions = risk_analysis.dangerous_functions.len() > 0;
                            status.dangerous_functions = risk_analysis.dangerous_functions;
                            status.is_contract_verified = risk_analysis.is_verified;
                            
                            // Cập nhật thông tin thuế nếu có
                            if let Some(ref mut tax_info) = status.tax_info {
                                // Có thể cần cập nhật từ risk_analysis nếu có
                            } else {
                                let tax_info = TaxInfo {
                                    buy_tax: 0.0,
                                    sell_tax: 0.0,
                                    transfer_tax: 0.0,
                                    min_hold_time: None,
                                };
                                status.tax_info = Some(tax_info);
                            }
                        }
                        
                        return Ok(status);
                    }
                },
                Err(_) => {
                    // Không thể lấy lock, thử dùng timeout
                    match with_timeout(tracker.lock(), Duration::from_secs(5)).await {
                        Ok(mut lock) => {
                            // Đoạn code tương tự như trên
                            if lock.get_token(token_address).is_none() {
                                lock.add_token(token_address).await?;
                            }
                            
                            if let Some(status) = lock.get_token(token_address) {
                                let mut status = status.clone();
                                
                                if let Some(analyzer) = &self.risk_analyzer {
                                    let token_addr = Address::from_str(token_address).map_err(|e| format!("Địa chỉ token không hợp lệ: {}", e))?;
                                    let risk_analysis = analyzer.analyze_token(token_addr).await?;
                                    
                                    let safety_level = if risk_analysis.base.risk_score < 60.0 {
                                        TokenSafetyLevel::Green
                                    } else if risk_analysis.base.risk_score < 80.0 {
                                        TokenSafetyLevel::Yellow
                                    } else {
                                        TokenSafetyLevel::Red
                                    };
                                    
                                    status.safety_emoji = safety_level.to_string();
                                    status.has_dangerous_functions = risk_analysis.dangerous_functions.len() > 0;
                                    status.dangerous_functions = risk_analysis.dangerous_functions;
                                    status.is_contract_verified = risk_analysis.is_verified;
                                    
                                    // Cập nhật thông tin thuế nếu có
                                    if let Some(ref mut tax_info) = status.tax_info {
                                        // Có thể cần cập nhật từ risk_analysis nếu có
                                    } else {
                                        let tax_info = TaxInfo {
                                            buy_tax: 0.0,
                                            sell_tax: 0.0,
                                            transfer_tax: 0.0,
                                            min_hold_time: None,
                                        };
                                        status.tax_info = Some(tax_info);
                                    }
                                }
                                
                                return Ok(status);
                            }
                        },
                        Err(_) => {
                            return Err("Không thể lấy lock token tracker sau khi timeout, có thể deadlock".into());
                        }
                    }
                }
            }
        }
        
        Err("Không thể lấy thông tin trạng thái token".into())
    }

    // Thực hiện giao dịch mua trong chế độ Manual
    pub async fn manual_buy(&self, token_address: &str, amount: &str, gas_price_percent: u64) -> Result<SnipeResult, Box<dyn std::error::Error>> {
        // Chỉ cho phép ở chế độ Manual
        if self.bot_mode != BotMode::Manual {
            return Err("Chỉ có thể sử dụng manual_buy trong chế độ Manual".into());
        }
        
        // Parse lượng token đầu vào
        let amount_in = ethers::utils::parse_ether(amount)?;
        
        // Lấy thông tin gas hiện tại
        let gas_info = self.chain_adapter.get_gas_info().await?;
        
        // Tính gas price mới dựa trên phần trăm
        let gas_price = if gas_price_percent > 0 {
            gas_info.gas_price.saturating_mul(U256::from(gas_price_percent)).div(U256::from(100))
        } else {
            gas_info.gas_price
        };
        
        // Tạo thông tin token
        let token_info = TokenInfo {
            address: token_address.to_string(),
            symbol: "UNKNOWN".to_string(), // Sẽ được cập nhật sau
            decimals: 18,
            router: self.config.router_address.clone(),
            pair: None,
        };
        
        // Lấy thông tin chi tiết từ blockchain nếu có thể
        let token_info = match self.chain_adapter.get_token_info(token_address).await {
            Ok(info) => info,
            Err(_) => token_info,
        };
        
        // Tạo cấu hình snipe
        let snipe_config = SnipeConfig {
            token_address: token_info.address.clone(),
            amount_in: amount_in.clone(),
            amount_out_min: U256::from(0), // Tạm thời để 0, có thể cải thiện sau
            deadline: 0, // Tạm thời để 0, có thể cải thiện sau
            slippage: self.config.default_slippage,
            gas_price: gas_price.clone(),
            gas_limit: U256::from(0), // Tạm thời để 0, có thể cải thiện sau
            chain_id: self.config.chain_id,
            max_retries: 3, // Tạm thời để 3, có thể cải thiện sau
            retry_delay: 1000, // Tạm thời để 1000, có thể cải thiện sau
        };
        
        // Thực hiện snipe
        self.snipe(&token_info, amount_in, &snipe_config).await
    }
    
    // Thực hiện giao dịch bán trong chế độ Manual
    pub async fn manual_sell(&self, token_address: &str, amount_percent: u8, gas_price_percent: u64) -> Result<SnipeResult, Box<dyn std::error::Error>> {
        // Chỉ cho phép ở chế độ Manual
        if self.bot_mode != BotMode::Manual {
            return Err("Chỉ có thể sử dụng manual_sell trong chế độ Manual".into());
        }
        
        // Kiểm tra phần trăm hợp lệ
        if amount_percent == 0 || amount_percent > 100 {
            return Err("Phần trăm bán phải từ 1-100".into());
        }
        
        // Lấy số dư token
        let token_balance = match self.chain_adapter.get_token_balance(
            token_address, 
            &self.get_current_wallet_address()
        ).await {
            Ok(balance) => balance,
            Err(e) => return Err(format!("Không thể lấy số dư token: {}", e).into()),
        };
        
        if token_balance.is_zero() {
            return Err("Không có token để bán".into());
        }
        
        // Tính số lượng token cần bán
        let amount_to_sell = token_balance.saturating_mul(U256::from(amount_percent)).div(U256::from(100));
        
        // Lấy thông tin gas hiện tại
        let gas_info = self.chain_adapter.get_gas_info().await?;
        
        // Tính gas price mới dựa trên phần trăm
        let gas_price = if gas_price_percent > 0 {
            gas_info.gas_price.saturating_mul(U256::from(gas_price_percent)).div(U256::from(100))
        } else {
            gas_info.gas_price
        };
        
        // Thực hiện bán token
        let deadline = match SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() + 300, // 5 phút
            Err(_) => {
                // Nếu có lỗi với thời gian hệ thống, sử dụng giá trị mặc định
                warn!("Lỗi khi lấy thời gian hệ thống, sử dụng deadline mặc định");
                u64::MAX / 2 // Một giá trị đủ lớn, nhưng không gây tràn số
            }
        };
        
        // Kiểm tra nếu token đã được approve
        let router_address = &self.config.router_address;
        let approval_status = self.chain_adapter.check_token_allowance(
            token_address,
            router_address,
            &self.get_current_wallet_address(),
        ).await?;
        
        if approval_status < amount_to_sell {
            // Token cần được approve
            info!("Tự động approve token {} cho router {}", token_address, router_address);
            match self.chain_adapter.approve_token(
                token_address, 
                router_address, 
                U256::MAX, // Approve không giới hạn
                Some(self.config.default_gas_limit),
                Some(gas_price.as_u64()),
            ).await {
                Ok(_) => info!("Đã approve token thành công"),
                Err(e) => return Err(format!("Không thể approve token: {}", e).into()),
            }
        }
        
        let result = self.chain_adapter.swap_exact_tokens_for_eth(
            token_address,
            amount_to_sell,
            U256::zero(), // min_amount_out: tạm thời để 0, có thể cải thiện sau
            &self.get_current_wallet_address(),
            deadline,
            Some(self.config.default_gas_limit),
            Some(gas_price.as_u64()),
        ).await?;
        
        // Tạo kết quả
        let timestamp = match SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => {
                warn!("Lỗi khi lấy thời gian hệ thống, sử dụng timestamp mặc định");
                0 // Sử dụng giá trị mặc định cho timestamp
            }
        };
        
        let snipe_result = SnipeResult {
            transaction_hash: result.as_ref().map(|r| format!("{:?}", r.transaction_hash)),
            success: result.is_some(),
            token_address: token_address.to_string(),
            amount_in: amount_to_sell.to_string(),
            estimated_amount_out: None,
            error: if result.is_none() { Some("Giao dịch bán thất bại".into()) } else { None },
            timestamp,
        };
        
        // Lưu kết quả vào storage
        if let Err(e) = self.storage.add_transaction(snipe_result.clone().into()) {
            warn!("Không thể lưu giao dịch: {}", e);
        }
        
        Ok(snipe_result)
    }

    // Thêm auto trade config
    pub fn set_auto_trade_config(&mut self, config: AutoTradeConfig) {
        self.auto_trade_config = Some(config);
    }
    
    // Cập nhật phương thức auto_trade để hỗ trợ các chiến lược mới
    pub async fn auto_trade(&self, token_address: &str, amount: &str, action: &str) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Kiểm tra bot mode
        if self.bot_mode != BotMode::Auto {
            return Err("Bot không ở chế độ Auto".into());
        }
        
        // Phân tích token
        let (token_info, token_status, risk_analysis) = self.analyze_token(token_address).await?;
        
        // Kiểm tra giữ lại reserve
        if action == "buy" && !self.check_reserve_balance().await? {
            return Err("Không đủ reserve balance. Duy trì tối thiểu 25% số dư".into());
        }
        
        // Kiểm tra safety level và áp dụng chiến lược phù hợp
        match token_status.safety_level {
            TokenSafetyLevel::Red => {
                // Không giao dịch token đỏ
                return Err("Không giao dịch token có mức độ nguy hiểm cao".into());
            },
            TokenSafetyLevel::Yellow => {
                // Chiến lược cho token vàng
                if action == "buy" {
                    // Kiểm tra điều kiện mua dựa trên large orders
                    if let Some(config) = &self.auto_trade_config {
                        if !config.yellow_token_strategy.buy_on_large_orders {
                            return Err("Chiến lược mua token vàng không được kích hoạt".into());
                        }
                        
                        // Kiểm tra mempool xem có lệnh lớn không
                        let has_large_orders = self.check_large_orders_in_mempool(
                            token_address, 
                            config.yellow_token_strategy.min_large_order_usd
                        ).await?;
                        
                        if !has_large_orders {
                            return Err("Không phát hiện lệnh mua lớn trên mempool cho token này".into());
                        }
                    }
                } else if action == "sell" {
                    // Tự động bán sau 5 phút
                    // Logic đã được xử lý ở ServiceManager
                } else if action == "sandwich" {
                    // Thực hiện sandwich nếu được kích hoạt
                    if let Some(config) = &self.auto_trade_config {
                        if !config.yellow_token_strategy.use_sandwich_mode {
                            return Err("Chế độ sandwich không được kích hoạt cho token vàng".into());
                        }
                        
                        // Tìm cơ hội sandwich
                        return self.execute_sandwich_strategy(token_address).await;
                    }
                }
            },
            TokenSafetyLevel::Green => {
                // Chiến lược cho token xanh
                if action == "buy" {
                    if let Some(config) = &self.auto_trade_config {
                        if !config.green_token_strategy.front_run_orders {
                            return Err("Front-running không được kích hoạt cho token xanh".into());
                        }
                        
                        // Kiểm tra mempool cho lệnh lớn để front-run
                        let large_orders = self.get_large_orders_in_mempool(
                            token_address, 
                            config.green_token_strategy.min_order_size_usd
                        ).await?;
                        
                        if large_orders.is_empty() {
                            return Err("Không phát hiện lệnh lớn để front-run".into());
                        }
                        
                        // Front-run lệnh lớn nhất
                        return self.front_run_transaction(&large_orders[0], amount).await;
                    }
                } else if action == "tsl" {
                    // Kích hoạt trailing stop loss
                    if let Some(config) = &self.auto_trade_config {
                        if !config.green_token_strategy.use_trailing_stop {
                            return Err("Trailing stop loss không được kích hoạt cho token xanh".into());
                        }
                        
                        return self.activate_trailing_stop_loss(
                            token_address, 
                            config.green_token_strategy.trailing_stop_percent
                        ).await;
                    }
                } else if action == "sandwich" {
                    // Thực hiện sandwich nếu được kích hoạt
                    if let Some(config) = &self.auto_trade_config {
                        if !config.green_token_strategy.use_sandwich_mode {
                            return Err("Chế độ sandwich không được kích hoạt cho token xanh".into());
                        }
                        
                        // Tìm cơ hội sandwich
                        return self.execute_sandwich_strategy(token_address).await;
                    }
                }
            }
        }
        
        // Nếu không có chiến lược đặc biệt, thực hiện giao dịch thông thường
        match action {
            "buy" => {
                // Mua token
                let amount_in = ethers::utils::parse_ether(amount)
                    .map_err(|e| format!("Số lượng không hợp lệ: {}", e))?;
                
                self.buy_token(token_address, amount_in).await
            },
            "sell" => {
                // Bán token
                let percent = amount.parse::<u8>().unwrap_or(100);
                self.sell_token_percent(token_address, percent).await
            },
            "emergency_sell" => {
                // Bán khẩn cấp
                self.emergency_sell(token_address).await
            },
            _ => Err(format!("Hành động không hỗ trợ: {}", action).into()),
        }
    }
    
    // Kiểm tra số dư reserve
    async fn check_reserve_balance(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Lấy số dư native token
        let wallet_address = self.get_current_wallet_address();
        let balance = self.chain_adapter.get_native_balance(&wallet_address).await?;
        
        // Lấy số dư stablecoin (USDT/USDC)
        let stablecoin_balance = self.get_stablecoin_balance().await?;
        
        // Tính tổng số dư
        let native_in_usd = self.convert_native_to_usd(balance).await?;
        let total_balance = native_in_usd + stablecoin_balance;
        
        // Kiểm tra reserve percent
        let reserve_percent = if let Some(config) = &self.auto_trade_config {
            config.reserve_percent
        } else {
            25.0 // Mặc định
        };
        
        let current_reserve = stablecoin_balance / total_balance * 100.0;
        
        // Gửi thông báo nếu reserve thấp
        if current_reserve < reserve_percent {
            if let Some(tx_sender) = self.get_service_tx() {
                let _ = tx_sender.send(ServiceMessage::ReserveBalanceAlert { 
                    current_percent: current_reserve 
                }).await;
            }
            
            return false;
        }
        
        Ok(true)
    }
    
    // Lấy số dư stablecoin (USDT, USDC)
    async fn get_stablecoin_balance(&self) -> Result<f64, Box<dyn std::error::Error>> {
        let wallet_address = self.get_current_wallet_address();
        let mut total_balance = 0.0;
        
        // Danh sách stablecoin phổ biến
        let stablecoins = vec![
            "0xdAC17F958D2ee523a2206206994597C13D831ec7", // USDT (Ethereum)
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", // USDC (Ethereum)
            // Thêm các stablecoin khác theo chain
        ];
        
        for token in stablecoins {
            match self.chain_adapter.get_token_balance(token, &wallet_address).await {
                Ok(balance) => {
                    // Lấy decimals của token
                    let decimals = match self.chain_adapter.get_token_info(token).await {
                        Ok(info) => info.decimals,
                        Err(_) => 18, // Mặc định
                    };
                    
                    // Chuyển đổi sang số thập phân
                    let balance_f64 = balance.as_u128() as f64 / (10.0_f64.powi(decimals as i32));
                    total_balance += balance_f64;
                },
                Err(e) => {
                    warn!("Không thể lấy số dư token {}: {}", token, e);
                }
            }
        }
        
        Ok(total_balance)
    }
    
    // Chuyển đổi native token sang USD
    async fn convert_native_to_usd(&self, amount: U256) -> Result<f64, Box<dyn std::error::Error>> {
        // Lấy giá native token (ETH, BNB, ...) từ price oracle
        let native_price = match self.get_native_price().await {
            Ok(price) => price,
            Err(_) => 2000.0, // Giá mặc định nếu không lấy được
        };
        
        // Chuyển đổi amount sang số thập phân
        let amount_f64 = ethers::utils::format_ether(amount)
            .parse::<f64>()
            .unwrap_or(0.0);
            
        Ok(amount_f64 * native_price)
    }
    
    // Lấy giá native token
    async fn get_native_price(&self) -> Result<f64, Box<dyn std::error::Error>> {
        // Thực tế nên sử dụng price oracle hoặc API
        // Đây là hàm mẫu
        Ok(2000.0) // Giả định giá ETH là $2000
    }
    
    // Kiểm tra mempool để phát hiện lệnh lớn
    async fn check_large_orders_in_mempool(&self, token_address: &str, min_amount_usd: f64) -> Result<bool, Box<dyn std::error::Error>> {
        // Nếu không có mempool watcher
        if self.mempool_watcher.is_none() {
            return Ok(false);
        }
        
        // Lấy orders từ mempool watcher
        let orders = self.get_large_orders_in_mempool(token_address, min_amount_usd).await?;
        
        Ok(!orders.is_empty())
    }
    
    // Lấy danh sách các lệnh lớn từ mempool
    async fn get_large_orders_in_mempool(&self, token_address: &str, min_amount_usd: f64) -> Result<Vec<PendingSwap>, Box<dyn std::error::Error>> {
        // Truy cập mempool tracker thông qua watcher
        if let Some(mempool_watcher) = &self.mempool_watcher {
            let watcher = mempool_watcher.lock().await;
            
            // Lấy các giao dịch lớn đang chờ xử lý
            if let Some(mempool_tracker) = &watcher.mempool_tracker {
                let mut large_orders: Vec<PendingSwap> = Vec::new();
                
                // Lọc các giao dịch mua lớn
                if let Some(swaps) = mempool_tracker.pending_swaps.get(token_address) {
                    for swap in swaps {
                        if swap.is_buy && swap.amount_usd >= min_amount_usd {
                            large_orders.push(swap.clone());
                        }
                    }
                }
                
                return Ok(large_orders);
            }
        }
        
        Ok(Vec::new())
    }
    
    // Front-run một giao dịch
    async fn front_run_transaction(&self, pending_swap: &PendingSwap, amount: &str) -> Result<TradeResult, Box<dyn std::error::Error>> {
        info!("Front-running giao dịch: {}", pending_swap.tx_hash);
        
        // Tính gas cao hơn giao dịch gốc
        let original_gas = pending_swap.gas_price;
        let front_run_gas = original_gas * 110 / 100; // Tăng 10%
        
        // Chuyển đổi amount sang U256
        let amount_in = ethers::utils::parse_ether(amount)
            .map_err(|e| format!("Số lượng không hợp lệ: {}", e))?;
        
        // Sử dụng chain adapter để mua token với gas price cao hơn
        let token_address = &pending_swap.token_address;
        let mut slippage = 1.0; // 1% slippage
        
        // Tính toán amount_out_min
        let path = self.chain_adapter.get_native_to_token_path(token_address);
        let amounts = self.chain_adapter.get_amounts_out(amount_in, path).await?;
        let amount_out = amounts.last().ok_or("Không thể tính toán amount out")?;
        let amount_out_min = *amount_out * U256::from(100 - (slippage as u64)) / U256::from(100);
        
        // Deadline giao dịch (sau 5 phút)
        let deadline = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() + 300;
        
        // Địa chỉ ví
        let wallet_address = self.get_current_wallet_address();
        
        // Thực hiện giao dịch với gas cao hơn
        let receipt = self.chain_adapter.swap_exact_eth_for_tokens(
            token_address,
            amount_in,
            amount_out_min,
            &wallet_address,
            deadline as u64,
            None, // gas limit mặc định
            Some(front_run_gas.as_u64()),
        ).await?;
        
        // Tạo kết quả giao dịch
        let trade_result = TradeResult {
            success: receipt.is_some(),
            tx_hash: receipt.map(|r| format!("{:?}", r.transaction_hash)),
            amount_in: ethers::utils::format_ether(amount_in),
            amount_out: None, // Cần thêm xử lý để lấy amount out từ receipt
            price_per_token: None,
            gas_used: receipt.as_ref().map(|r| r.gas_used.as_u64()),
            gas_price: front_run_gas.as_u64(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            error: if receipt.is_none() { Some("Không nhận được receipt".to_string()) } else { None },
            trade_type: TradeType::Buy,
        };
        
        Ok(trade_result)
    }
    
    // Thực hiện sandwich attack
    async fn execute_sandwich_strategy(&self, token_address: &str) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Tìm cơ hội sandwich từ mempool
        let opportunity = self.find_sandwich_opportunity(token_address).await?;
        
        if opportunity.is_none() {
            return Err("Không tìm thấy cơ hội sandwich phù hợp".into());
        }
        
        let opportunity = opportunity.unwrap();
        
        info!("Thực hiện sandwich attack cho token: {}", token_address);
        
        // Bước 1: Front-run
        let front_run_amount = "0.05"; // 0.05 ETH
        let front_run_result = self.front_run_transaction(&opportunity.victim_swap, front_run_amount).await?;
        
        if !front_run_result.success {
            return Err("Front-run thất bại".into());
        }
        
        // Bước 2: Đợi victim transaction thực hiện
        // Trong thực tế, cần theo dõi mempool để biết khi nào victim tx được xác nhận
        // Ở đây chỉ là mô phỏng
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Bước 3: Back-run (bán token)
        let back_run_result = self.sell_token_percent(token_address, 100).await?;
        
        // Kết hợp kết quả
        let trade_result = TradeResult {
            success: front_run_result.success && back_run_result.success,
            tx_hash: Some(format!("front:{},back:{}", 
                                 front_run_result.tx_hash.unwrap_or_default(), 
                                 back_run_result.tx_hash.unwrap_or_default())),
            amount_in: front_run_result.amount_in,
            amount_out: back_run_result.amount_out,
            price_per_token: None,
            gas_used: Some(front_run_result.gas_used.unwrap_or(0) + back_run_result.gas_used.unwrap_or(0)),
            gas_price: (front_run_result.gas_price + back_run_result.gas_price) / 2,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            error: None,
            trade_type: TradeType::Buy,
        };
        
        Ok(trade_result)
    }
    
    // Tìm cơ hội sandwich từ mempool
    async fn find_sandwich_opportunity(&self, token_address: &str) -> Result<Option<SandwichOpportunity>, Box<dyn std::error::Error>> {
        // Chỉ hỗ trợ sandwich cho các ví gói cao cấp
        if self.current_user_level < SubscriptionLevel::Premium {
            return Ok(None);
        }
        
        debug!("Tìm cơ hội sandwich cho token {}", token_address);
        
        // Sử dụng nội bộ để chỉ định cơ hội
        let (tx, rx) = tokio::sync::oneshot::channel::<Option<SandwichOpportunity>>();
        
        // Lấy số dư hiện tại để xác định khoản có thể trade
        let balance = self.get_native_balance().await?;
        let min_amount = balance / U256::from(10); // Ít nhất 10% số dư
        
        // Chỉ sử dụng sandwich nếu số dư đủ lớn
        if min_amount.is_zero() {
            return Ok(None);
        }
        
        // Lấy địa chỉ ví
        let wallet_address = match &self.current_wallet_address {
            Some(addr) => addr.clone(),
            None => return Err("Không có ví được chọn để tìm cơ hội sandwich".into()),
        };
        
        // Theo dõi mempool cho token
        let callback = Box::new(move |transaction: Transaction, value: U256| {
            // Lọc giao dịch vào có giá trị đủ lớn
            if value >= min_amount {
                // Cơ hội sandwich tiềm năng
                let opportunity = SandwichOpportunity {
                    token_address: token_address.to_string(),
                    victim_swap: PendingSwap {
                        tx_hash: format!("{:?}", transaction.hash),
                        from: format!("{:?}", transaction.from),
                        to: format!("{:?}", transaction.to.unwrap_or_default()),
                        value: value.to_string(),
                        gas_price: transaction.gas_price.unwrap_or_default().to_string(),
                        input: format!("{:?}", transaction.input),
                    },
                    potential_profit: 0.0, // Sẽ tính toán sau
                    estimated_price_impact: 0.0, // Sẽ tính toán sau
                };
                
                // Gửi cơ hội qua channel
                let _ = tx.send(Some(opportunity));
            }
        });
        
        // Bắt đầu theo dõi với một minimum ETH amount
        let _ = self.chain_adapter.watch_for_sandwich_opportunities(
            token_address, 
            min_amount,
            callback
        ).await?;
        
        // Đặt timeout 5 giây để tìm cơ hội
        let timeout = tokio::time::Duration::from_secs(5);
        match tokio::time::timeout(timeout, rx).await {
            Ok(result) => match result {
                Ok(opportunity) => Ok(opportunity),
                Err(_) => Ok(None),
            },
            Err(_) => Ok(None), // Timeout
        }
    }
    
    // Kích hoạt trailing stop loss
    async fn activate_trailing_stop_loss(&self, token_address: &str, trailing_percent: f64) -> Result<TradeResult, Box<dyn std::error::Error>> {
        info!("Kích hoạt trailing stop loss cho token {} với mức {}%", token_address, trailing_percent);
        
        // Lấy thông tin token và giá hiện tại
        let (token_info, token_status, _) = self.analyze_token(token_address).await?;
        let current_price = token_status.price_usd;
        
        // Tạo trailing stop loss mới
        let trailing_stop = TrailingStopLoss {
            token_address: token_address.to_string(),
            activation_price: current_price,
            highest_price: current_price,
            trail_percent: trailing_percent,
            stop_price: current_price * (1.0 - trailing_percent / 100.0),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        // Lưu trailing stop loss
        if let Some(trade_manager) = &self.trade_manager {
            let mut manager = trade_manager.lock().await;
            manager.set_trailing_stop_loss(token_address, trailing_stop).await?;
        }
        
        // Trả về kết quả giả
        let trade_result = TradeResult {
            success: true,
            tx_hash: None,
            amount_in: "0".to_string(),
            amount_out: None,
            price_per_token: Some(current_price),
            gas_used: None,
            gas_price: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            error: None,
            trade_type: TradeType::Approve, // Sử dụng Approve vì không có TradeType phù hợp
        };
        
        Ok(trade_result)
    }
    
    // Tự động giao dịch dựa trên phân loại token
    pub async fn auto_trade_by_safety(&self, token_address: &str) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phân tích token
        let (token_info, token_status, risk_analysis) = self.analyze_token(token_address).await?;
        
        // Kiểm tra cấp độ người dùng
        match self.current_user_level {
            SubscriptionLevel::Free => {
                // Free user chỉ mua token 🟡 và 🟢, không mua token 🔴
                match token_status.safety_level {
                    TokenSafetyLevel::Red => {
                        return Err("Free user không thể giao dịch token nguy hiểm".into());
                    },
                    TokenSafetyLevel::Yellow | TokenSafetyLevel::Green => {
                        // Chỉ mua token an toàn và bán sau 30 phút
                        if let Some(config) = &self.auto_trade_config {
                            // Logic mua token an toàn
                            let amount = "0.05"; // Giới hạn số lượng nhỏ cho free user
                            let result = self.buy_token_with_amount(token_address, amount).await?;
                            
                            // Thiết lập bán tự động sau 30 phút
                            if result.success {
                                self.schedule_auto_sell(token_address, 30).await?;
                            }
                            
                            return Ok(result);
                        }
                    }
                }
            },
            SubscriptionLevel::Premium => {
                // Premium user chỉ mua token 🟡 và 🟢, không mua token 🔴
                match token_status.safety_level {
                    TokenSafetyLevel::Red => {
                        return Err("Premium user không thể giao dịch token nguy hiểm".into());
                    },
                    TokenSafetyLevel::Yellow | TokenSafetyLevel::Green => {
                        if let Some(config) = &self.auto_trade_config {
                            // Sử dụng gas optimizer
                            let optimized_gas = if self.gas_optimizer.is_some() {
                                Some(self.optimize_gas().await?)
                            } else {
                                None
                            };
                            
                            // Sử dụng AI để quyết định mua/bán
                            let ai_decision = self.get_ai_trade_decision(token_address).await?;
                            
                            if ai_decision.confidence >= 0.6 && ai_decision.should_buy {
                                let amount = "0.1"; // Số lượng lớn hơn cho premium user
                                let result = self.buy_token_with_optimized_params(
                                    token_address, 
                                    amount, 
                                    optimized_gas
                                ).await?;
                                
                                // Thiết lập take profit và stop loss tự động
                                if result.success {
                                    self.set_take_profit_stop_loss(
                                        token_address,
                                        config.yellow_token_strategy.take_profit_percent,
                                        config.yellow_token_strategy.stop_loss_percent
                                    ).await?;
                                }
                                
                                return Ok(result);
                            } else {
                                return Err(format!("AI quyết định không mua token (confidence {}%)", 
                                                 ai_decision.confidence * 100.0).into());
                            }
                        }
                    }
                }
            },
            SubscriptionLevel::VIP => {
                // VIP user có thể truy cập tất cả các tính năng, nhưng vẫn không mua token 🔴
                match token_status.safety_level {
                    TokenSafetyLevel::Red => {
                        return Err("VIP user không thể giao dịch token nguy hiểm".into());
                    },
                    TokenSafetyLevel::Yellow | TokenSafetyLevel::Green => {
                        if let Some(config) = &self.auto_trade_config {
                            // Sử dụng mempool watching
                            if config.yellow_token_strategy.use_mempool_data {
                                // Kiểm tra cơ hội front-run
                                let large_orders = self.get_large_orders_in_mempool(
                                    token_address, 
                                    config.green_token_strategy.min_order_size_usd
                                ).await?;
                                
                                if !large_orders.is_empty() && config.green_token_strategy.front_run_orders {
                                    // Thực hiện front-run
                                    let front_run_result = self.front_run_transaction(
                                        &large_orders[0], 
                                        "0.2"
                                    ).await?;
                                    
                                    if front_run_result.success && config.green_token_strategy.use_trailing_stop {
                                        // Kích hoạt trailing stop loss
                                        self.activate_trailing_stop_loss(
                                            token_address, 
                                            config.green_token_strategy.trailing_stop_percent
                                        ).await?;
                                    }
                                    
                                    return Ok(front_run_result);
                                }
                            }
                            
                            // Sử dụng AI phân tích nâng cao
                            let ai_decision = self.get_ai_trade_decision(token_address).await?;
                            
                            if ai_decision.confidence >= 0.7 {
                                if ai_decision.should_buy {
                                    // Mua với các tham số tối ưu
                                    let optimized_gas = self.optimize_gas().await?;
                                    let amount = "0.3"; // Số lượng lớn hơn cho VIP
                                    
                                    let result = self.buy_token_with_optimized_params(
                                        token_address, 
                                        amount, 
                                        Some(optimized_gas)
                                    ).await?;
                                    
                                    // Thiết lập trailing stop loss
                                    if result.success && config.green_token_strategy.use_trailing_stop {
                                        self.activate_trailing_stop_loss(
                                            token_address, 
                                            config.green_token_strategy.trailing_stop_percent
                                        ).await?;
                                    }
                                    
                                    return Ok(result);
                                } else if ai_decision.prediction == "pump_soon" {
                                    // Mua thêm khi AI dự đoán sắp pump
                                    let amount = "0.5"; // Mua nhiều hơn 
                                    return self.buy_token_with_amount(token_address, amount).await;
                                }
                            }
                            
                            // Kiểm tra cơ hội sandwich nếu được kích hoạt
                            if config.yellow_token_strategy.use_sandwich_mode || 
                               config.green_token_strategy.use_sandwich_mode {
                                // Tính toán phần trăm token sử dụng cho sandwich
                                let sandwich_percent = 30.0; // % số dư dùng cho sandwich
                                
                                // Tìm cơ hội sandwich
                                let sandwich_result = self.execute_sandwich_strategy_with_percent(
                                    token_address,
                                    sandwich_percent
                                ).await;
                                
                                if let Ok(result) = sandwich_result {
                                    return Ok(result);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Err("Không tìm thấy cơ hội giao dịch phù hợp với cấp độ người dùng".into())
    }
    
    // Lên lịch bán tự động sau một khoảng thời gian
    async fn schedule_auto_sell(&self, token_address: &str, minutes: u64) -> Result<(), Box<dyn std::error::Error>> {
        let token_address = token_address.to_string();
        let snipebot = self.clone();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(minutes * 60)).await;
            
            // Kiểm tra xem có còn sở hữu token không
            if let Ok(has_position) = snipebot.has_position(&token_address).await {
                if has_position {
                    match snipebot.sell_token(&token_address, None, Some(100), None, None).await {
                        Ok(result) => {
                            info!("Bán tự động token {} thành công: tx={}", 
                                 token_address, result.tx_hash.unwrap_or_default());
                        },
                        Err(e) => {
                            error!("Không thể bán tự động token {}: {}", token_address, e);
                        }
                    }
                }
            }
        });
        
        Ok(())
    }
    
    // Thiết lập take profit và stop loss
    async fn set_take_profit_stop_loss(&self, token_address: &str, take_profit_percent: f64, stop_loss_percent: f64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(trade_manager) = &self.trade_manager {
            let manager = trade_manager.lock().await;
            manager.set_profit_target(token_address, take_profit_percent)?;
            manager.set_stop_loss(token_address, stop_loss_percent)?;
        }
        
        Ok(())
    }
    
    // Thiết lập cấp độ người dùng
    pub fn set_user_level(&mut self, level: SubscriptionLevel) {
        self.current_user_level = level;
        
        // Cập nhật cấu hình auto trade dựa trên cấp độ người dùng
        self.update_auto_trade_config_for_subscription();
    }
    
    // Cập nhật cấu hình auto trade dựa trên cấp độ người dùng
    fn update_auto_trade_config_for_subscription(&mut self) {
        let mut auto_trade_config = AutoTradeConfig::default();
        
        match self.current_user_level {
            SubscriptionLevel::Free => {
                let free_config = &self.subscription_config.free_config;
                
                // Cập nhật cấu hình cho Free user
                auto_trade_config.enabled = free_config.enabled;
                auto_trade_config.red_token_strategy = false; // Không bao giờ giao dịch token đỏ
                
                // Chỉ mua token vàng và xanh, bán sau 30 phút
                auto_trade_config.yellow_token_strategy.buy_on_large_orders = free_config.allow_yellow_tokens;
                auto_trade_config.yellow_token_strategy.sell_after_minutes = free_config.auto_sell_minutes;
                auto_trade_config.yellow_token_strategy.use_sandwich_mode = false;
                auto_trade_config.yellow_token_strategy.use_mempool_data = false;
                
                // Vô hiệu hóa các chiến lược nâng cao
                auto_trade_config.green_token_strategy.front_run_orders = false;
                auto_trade_config.green_token_strategy.use_trailing_stop = false;
                auto_trade_config.green_token_strategy.use_sandwich_mode = false;
                
                // Vô hiệu hóa MEV
                auto_trade_config.use_mev_strategies = false;
            },
            SubscriptionLevel::Premium => {
                let premium_config = &self.subscription_config.premium_config;
                
                // Cập nhật cấu hình cho Premium user
                auto_trade_config.enabled = premium_config.enabled;
                auto_trade_config.red_token_strategy = false; // Không bao giờ giao dịch token đỏ
                
                // Mua token vàng và xanh với AI support
                auto_trade_config.yellow_token_strategy.buy_on_large_orders = premium_config.allow_yellow_tokens;
                auto_trade_config.yellow_token_strategy.sell_after_minutes = premium_config.auto_sell_minutes;
                auto_trade_config.yellow_token_strategy.take_profit_percent = premium_config.take_profit_percent;
                auto_trade_config.yellow_token_strategy.stop_loss_percent = premium_config.stop_loss_percent;
                auto_trade_config.yellow_token_strategy.use_sandwich_mode = false;
                auto_trade_config.yellow_token_strategy.use_mempool_data = true;
                
                // Gas optimizer cho Premium users
                if premium_config.use_gas_optimizer && self.gas_optimizer.is_some() {
                    auto_trade_config.max_gas_boost_percent = 150;
                }
                
                // Vô hiệu hóa front-run, nhưng vẫn cho phép phân tích AI
                auto_trade_config.green_token_strategy.front_run_orders = false;
                auto_trade_config.green_token_strategy.use_sandwich_mode = false;
                
                // Vô hiệu hóa MEV hoàn toàn
                auto_trade_config.use_mev_strategies = false;
            },
            SubscriptionLevel::VIP => {
                let vip_config = &self.subscription_config.vip_config;
                
                // Cập nhật cấu hình cho VIP user - mở tất cả các tính năng
                auto_trade_config.enabled = vip_config.enabled;
                auto_trade_config.red_token_strategy = false; // Không bao giờ giao dịch token đỏ
                
                // Mở tất cả các tính năng cho token vàng và xanh
                auto_trade_config.yellow_token_strategy.buy_on_large_orders = vip_config.allow_yellow_tokens;
                auto_trade_config.yellow_token_strategy.use_sandwich_mode = vip_config.use_sandwich_mode;
                auto_trade_config.yellow_token_strategy.use_mempool_data = vip_config.use_mempool_watching;
                auto_trade_config.yellow_token_strategy.take_profit_percent = vip_config.take_profit_percent;
                auto_trade_config.yellow_token_strategy.stop_loss_percent = vip_config.stop_loss_percent;
                
                // Kích hoạt front-run và chiến lược nâng cao
                auto_trade_config.green_token_strategy.front_run_orders = vip_config.use_front_run;
                auto_trade_config.green_token_strategy.use_trailing_stop = vip_config.use_trailing_stop_loss;
                auto_trade_config.green_token_strategy.trailing_stop_percent = vip_config.trailing_stop_percent;
                auto_trade_config.green_token_strategy.use_sandwich_mode = vip_config.use_sandwich_mode;
                auto_trade_config.green_token_strategy.take_profit_percent = vip_config.take_profit_percent;
                
                // Kích hoạt tính năng MEV
                auto_trade_config.use_mev_strategies = true;
                auto_trade_config.max_gas_boost_percent = 200;
            }
        }
        
        self.auto_trade_config = Some(auto_trade_config);
    }
    
    // Thực hiện sandwich với phần trăm số dư cụ thể
    async fn execute_sandwich_strategy_with_percent(&self, token_address: &str, percent: f64) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Lấy cơ hội sandwich
        if let Some(mempool_watcher) = &self.mempool_watcher {
            let watcher = mempool_watcher.lock().await;
            
            if let Some(mempool_tracker) = &watcher.mempool_tracker {
                if let Some(opportunity) = mempool_tracker.get_best_sandwich_opportunity() {
                    if opportunity.token_address == token_address {
                        // Tính toán số lượng ETH dựa trên phần trăm số dư
                        let balance = self.get_wallet_balance().await?;
                        let amount_to_use = balance * percent / 100.0;
                        
                        // Thực hiện sandwich
                        return self.execute_sandwich(&opportunity, &amount_to_use.to_string()).await;
                    }
                }
            }
        }
        
        Err("Không tìm thấy cơ hội sandwich phù hợp".into())
    }
    
    // Tối ưu gas dựa trên điều kiện mạng
    async fn optimize_gas(&self) -> Result<U256, Box<dyn std::error::Error>> {
        let chain_name = self.chain_adapter.get_chain_name();
        let rpc_url = self.chain_adapter.get_config().primary_rpc_urls[0].clone();
        
        match self.gas_optimizer.get_optimal_gas_price().await {
            Ok(price) => {
                info!("Giá gas tối ưu: {} Gwei", price.as_u128() as f64 / 1_000_000_000.0);
                Ok(price)
            },
            Err(e) => {
                warn!("Không thể tối ưu gas price: {}, sử dụng giá mặc định", e);
                Ok(U256::from(5_000_000_000u64)) // 5 Gwei mặc định
            }
        }
    }
    
    // Lấy quyết định giao dịch từ AI
    async fn get_ai_trade_decision(&self, token_address: &str) -> Result<AIDecision, Box<dyn std::error::Error>> {
        if let Some(trade_manager) = &self.trade_manager {
            let manager = trade_manager.lock().await;
            
            // Lấy phân tích AI từ trade manager
            let ai_suggestions = manager.get_ai_suggestions(token_address);
            
            if !ai_suggestions.is_empty() {
                // Lấy suggest gần nhất
                let latest = ai_suggestions.last().unwrap();
                
                // Phân tích loại gợi ý
                let should_buy = matches!(latest.suggestion_type, 
                    AISuggestionType::BuyToken | AISuggestionType::IncreasePosition);
                
                // Trích xuất độ tin cậy từ lý do (nếu có)
                let confidence = if latest.reason.contains("confidence:") {
                    let parts: Vec<&str> = latest.reason.split("confidence:").collect();
                    if parts.len() > 1 {
                        parts[1].trim().parse::<f64>().unwrap_or(0.5)
                    } else {
                        0.5
                    }
                } else {
                    0.5
                };
                
                // Xác định dự đoán
                let prediction = if latest.reason.contains("pump soon") {
                    "pump_soon"
                } else if latest.reason.contains("bearish") {
                    "bearish"
                } else {
                    "neutral"
                };
                
                return Ok(AIDecision {
                    should_buy,
                    confidence,
                    prediction: prediction.to_string(),
                });
            }
            
            // Nếu không có suggest, trả về quyết định trung tính
            return Ok(AIDecision {
                should_buy: false,
                confidence: 0.0,
                prediction: "neutral".to_string(),
            });
        }
        
        Err("Không thể lấy dữ liệu AI, trade manager chưa được khởi tạo".into())
    }
    
    // Phương thức thiết lập cấu hình subscription
    pub fn set_subscription_config(&mut self, config: SubscriptionTradeConfig) {
        self.subscription_config = config;
    }
    
    // Phương thức khởi tạo auto trade theo cấp độ người dùng
    pub async fn start_auto_trade(&mut self, username: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Tìm người dùng trong database
        if let Some(user_manager) = &self.user_manager {
            let manager = user_manager.lock().await;
            
            if let Some(user) = manager.get_user(username) {
                // Kiểm tra subscription có còn hạn không
                if !user.subscription.is_active() {
                    return Err("Subscription đã hết hạn".into());
                }
                
                // Thiết lập cấp độ người dùng
                self.current_user = Some(user.clone());
                self.set_user_level(user.subscription.level.clone());
                
                // Bắt đầu auto trade service nếu chưa chạy
                if let BotMode::Manual = self.bot_mode {
                    self.set_bot_mode(BotMode::Auto);
                }
                
                // Kiểm tra nếu auto trade service chưa chạy
                self.start_auto_trade_service().await?;
                
                info!("Đã bắt đầu auto trade cho người dùng {} với cấp độ {:?}", 
                      username, user.subscription.level);
                
                return Ok(());
            }
            
            return Err(format!("Không tìm thấy người dùng {}", username).into());
        }
        
        Err("User manager chưa được khởi tạo".into())
    }
    
    // Phương thức lấy thông tin cấp độ của người dùng hiện tại
    pub fn get_current_subscription_level(&self) -> SubscriptionLevel {
        self.current_user_level.clone()
    }
    
    // Phương thức lấy cấu hình auto trade cho cấp độ hiện tại
    pub fn get_subscription_config(&self) -> &SubscriptionTradeConfig {
        &self.subscription_config
    }

    // src/snipebot.rs - thêm phương thức này
    async fn start_health_check(&self) {
        let token_tracker_clone = self.token_status_tracker.clone();
        let trade_manager_clone = self.trade_manager.clone();
        
        tokio::spawn(async move {
            let check_interval = tokio::time::Duration::from_secs(60);
            let mut interval = tokio::time::interval(check_interval);
            
            loop {
                interval.tick().await;
                
                // Kiểm tra sức khỏe của token_tracker
                if let Some(tracker) = &token_tracker_clone {
                    match with_timeout(tracker.try_lock(), Duration::from_secs(1)).await {
                        Ok(Ok(_)) => {
                            // OK - có thể lấy lock
                        },
                        _ => {
                            // Không thể lấy lock, có thể bị deadlock
                            warn!("Không thể lấy lock cho token_tracker sau nhiều lần thử, có thể bị deadlock");
                            
                            // Ghi log và gửi thông báo để admin kiểm tra
                            // TODO: Triển khai cơ chế thông báo
                        }
                    }
                }
                
                // Tương tự cho trade_manager
                // ...
            }
        });
    }
    
    // Thêm phương thức để xử lý status updates
    async fn process_status_updates(&mut self) {
        if let Some(mut rx) = self.status_update_receiver.take() {
            let trade_manager_weak = self.trade_manager.as_ref().map(Arc::downgrade);
            let token_tracker_weak = self.token_status_tracker.as_ref().map(Arc::downgrade);
            let risk_analyzer = self.risk_analyzer.clone();
            
            tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    match update {
                        StatusUpdate::TokenUpdate { token_address, new_status } => {
                            // Cập nhật token status
                            if let Some(tracker_weak) = &token_tracker_weak {
                                if let Some(tracker) = tracker_weak.upgrade() {
                                    if let Ok(mut tracker_lock) = tracker.try_lock() {
                                        if let Err(e) = tracker_lock.update_token_status(&token_address, new_status) {
                                            error!("Lỗi khi cập nhật token status: {}", e);
                                        }
                                        drop(tracker_lock);
                                    }
                                }
                            }
                        },
                        StatusUpdate::PriceAlert { token_address, alert } => {
                            // Xử lý cảnh báo giá
                            if let Some(trade_mgr_weak) = &trade_manager_weak {
                                if let Some(trade_mgr) = trade_mgr_weak.upgrade() {
                                    if let Ok(trade_mgr_lock) = trade_mgr.try_lock() {
                                        if let Err(e) = trade_mgr_lock.handle_price_alert(&alert).await {
                                            error!("Lỗi khi xử lý cảnh báo giá: {}", e);
                                        }
                                        drop(trade_mgr_lock);
                                    }
                                }
                            }
                        },
                        // Xử lý các loại update khác
                        // ...
                    }
                }
            });
        }
    }

    // Thêm phương thức xác thực địa chỉ ví
    pub fn validate_wallet_address(&self, address: &str) -> Result<(), Box<dyn std::error::Error>> {
        WalletManager::validate_wallet_address(address)
    }
    
    // Thêm phương thức xác thực private key
    pub fn validate_private_key(&self, private_key: &str) -> Result<(), Box<dyn std::error::Error>> {
        WalletManager::validate_private_key(private_key)
    }
    
    // Thêm phương thức xác thực mnemonic
    pub fn validate_mnemonic(&self, mnemonic: &str) -> Result<(), Box<dyn std::error::Error>> {
        WalletManager::validate_mnemonic(mnemonic)
    }

    // Cập nhật phương thức import_wallet_from_private_key
    pub async fn import_wallet_from_private_key(&self, private_key: &str) -> Result<WalletInfo, Box<dyn std::error::Error>> {
        // Xác thực private key trước khi tiếp tục
        self.validate_private_key(private_key)?;
        
        let mut wallet_manager = self.wallet_manager.lock().await;
        let wallet_info = wallet_manager.import_from_private_key(private_key)?;
        
        // Mã hóa private key
        let mut wallet_info_mut = wallet_info.clone();
        wallet_info_mut.encrypt_private_key(private_key, &wallet_manager.encryption_key)?;
        
        // Cập nhật ví trong danh sách
        for wallet in &mut wallet_manager.wallets {
            if wallet.address == wallet_info.address {
                *wallet = wallet_info_mut.clone();
                break;
            }
        }
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallet_info_mut)
    }
    
    // Cập nhật phương thức import_wallet_from_mnemonic
    pub async fn import_wallet_from_mnemonic(&self, mnemonic: &str, passphrase: Option<&str>) -> Result<WalletInfo, Box<dyn std::error::Error>> {
        // Xác thực mnemonic trước khi tiếp tục
        self.validate_mnemonic(mnemonic)?;
        
        let mut wallet_manager = self.wallet_manager.lock().await;
        let wallet_info = wallet_manager.import_from_mnemonic(mnemonic, passphrase)?;
        
        // Mã hóa mnemonic
        let mut wallet_info_mut = wallet_info.clone();
        wallet_info_mut.encrypt_mnemonic(mnemonic, &wallet_manager.encryption_key)?;
        
        // Cập nhật ví trong danh sách
        for wallet in &mut wallet_manager.wallets {
            if wallet.address == wallet_info.address {
                *wallet = wallet_info_mut.clone();
                break;
            }
        }
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallet_info_mut)
    }
    
    // Cập nhật phương thức create_new_wallet
    pub async fn create_new_wallet(&mut self, passphrase: Option<&str>) -> Result<SafeWalletView, Box<dyn std::error::Error>> {
        let mut wallet_manager = self.wallet_manager.lock().await;
        let wallet_view = wallet_manager.create_new_wallet(passphrase)?;
        
        // Lưu thay đổi
        wallet_manager.save_wallets().await?;
        
        Ok(wallet_view)
    }
    
    // Thêm phương thức để lấy chain adapter theo chain_id
    pub fn get_chain_adapter_by_id(&self, chain_id: u64) -> Option<&Box<dyn ChainAdapter>> {
        // Giả sử hiện tại chỉ có một adapter
        if self.chain_adapter.get_config().chain_id == chain_id {
            Some(&self.chain_adapter)
        } else {
            None
        }
    }

    // Thêm phương thức cấu hình gas optimizer
    pub fn configure_gas_optimizer(&mut self, 
                                max_gas_price: Option<U256>,
                                max_boost_percent: Option<u64>,
                                sample_interval: Option<u64>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(optimizer) = self.chain_adapter.get_gas_optimizer() {
            if let Some(max_price) = max_gas_price {
                optimizer.set_gas_price_limit(max_price);
            }
            
            if let Some(boost) = max_boost_percent {
                optimizer.set_max_boost_percent(boost);
            }
            
            if let Some(interval) = sample_interval {
                optimizer.set_sample_interval(interval);
            }
            
            info!("Đã cấu hình gas optimizer thành công");
            Ok(())
        } else {
            Err("Gas optimizer chưa được khởi tạo".into())
        }
    }

    // Phương thức update_user_subscription đã có trong SnipeBot, cần cập nhật để sử dụng UserManager
    pub async fn update_user_subscription(&mut self, username: &str, new_level: SubscriptionLevel, duration_days: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Lấy User Manager
        let user_manager_mutex = self.wallet_manager.clone();
        let mut user_manager = user_manager_mutex.lock().await;
        
        // Cập nhật subscription trong UserManager
        let user = user_manager.update_user_subscription(username, new_level, duration_days)?;
        
        // Lưu thay đổi vào bộ nhớ vĩnh viễn
        user_manager.save_users().await?;
        
        // Cập nhật mức subscription hiện tại cho SnipeBot nếu user hiện tại
        if let Some(current_user) = &self.current_user {
            if current_user.address == username {
                self.current_user_level = new_level.clone();
                self.update_auto_trade_config_for_subscription();
            }
        }
        
        info!("Cập nhật subscription của người dùng {} thành {:?} trong {} ngày", 
            username, new_level, duration_days);
            
        Ok(())
    }

    /// Lấy ước tính chi phí gas cho một giao dịch swap
    async fn estimate_swap_gas(&self, token_address: &str, amount_in: U256) -> Result<u64, Box<dyn std::error::Error>> {
        debug!("Ước tính gas cho swap token {}", token_address);
        
        // Lấy đường dẫn swap từ native token đến token đích
        let path = self.chain_adapter.get_native_to_token_path(token_address)?;
        
        // Lấy địa chỉ ví hiện tại
        let wallet_address = match &self.current_wallet_address {
            Some(addr) => addr.clone(),
            None => return Err("Không có ví được chọn để ước tính gas".into()),
        };
        
        // Lấy deadline (thời gian hiện tại + 20 phút)
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() + 1200;
        
        // Tạo tham số swap để ước tính gas
        let router_address = self.chain_adapter.get_config().router_address.clone();
        let provider = ethers::providers::Provider::<ethers::providers::Http>::try_from(&self.chain_adapter.get_config().rpc_url)?;
        
        // Lấy ABI của router
        let router_abi = abi_utils::get_router_abi();
        let router_addr = ethers::types::Address::from_str(&router_address)?;
        let router_contract = ethers::contract::Contract::new(
            router_addr, 
            serde_json::from_str(router_abi)?, 
            provider
        );
        
        // Lấy tên hàm swap từ cấu hình
        let swap_function = &self.chain_adapter.get_config().eth_to_token_swap_fn;
        
        // Min amount out (slippage 50% cho ước tính)
        let min_amount_out = U256::from(1);
        
        // Địa chỉ ví của người dùng
        let recipient = ethers::types::Address::from_str(&wallet_address)?;
        
        // Ước tính gas limit
        let gas_estimate = match swap_function.as_str() {
            "swapExactETHForTokens" => {
                let call = router_contract.method::<_, ()>(
                    "swapExactETHForTokens",
                    (min_amount_out, path, recipient, U256::from(deadline))
                )?;
                
                // Thêm value cho transaction
                let call = call.value(amount_in);
                
                // Ước tính gas
                call.estimate_gas().await?
            },
            "swapExactAVAXForTokens" => {
                let call = router_contract.method::<_, ()>(
                    "swapExactAVAXForTokens",
                    (min_amount_out, path, recipient, U256::from(deadline))
                )?;
                
                // Thêm value cho transaction
                let call = call.value(amount_in);
                
                // Ước tính gas
                call.estimate_gas().await?
            },
            _ => {
                // Mặc định cho các chains khác
                let call = router_contract.method::<_, ()>(
                    "swapExactETHForTokens",
                    (min_amount_out, path, recipient, U256::from(deadline))
                )?;
                
                // Thêm value cho transaction
                let call = call.value(amount_in);
                
                // Ước tính gas
                call.estimate_gas().await?
            }
        };
        
        // Thêm 30% buffer cho gas limit để tránh gas errors
        let gas_limit = gas_estimate.as_u64() * 130 / 100;
        
        debug!("Ước tính gas limit cho swap {}: {}", token_address, gas_limit);
        
        Ok(gas_limit)
    }
    
    /// Tính toán gas fees dựa trên các tham số khác nhau
    async fn calculate_gas_fees(&self, gas_limit: u64, boost_percent: u64) -> Result<(U256, U256, U256), Box<dyn std::error::Error>> {
        // Lấy gas price cơ bản
        let base_gas_price = self.optimize_gas().await?;
        let base_gas_price = U256::from(base_gas_price);
        
        // Tính gas price với boost
        let boosted_gas_price = base_gas_price + (base_gas_price * U256::from(boost_percent) / U256::from(100));
        
        // Tính max fee per gas và priority fee nếu chain hỗ trợ EIP-1559
        let (max_fee_per_gas, priority_fee_per_gas) = if self.chain_adapter.get_config().eip1559_supported {
            // Lấy priority fee từ config
            let priority_fee = match self.chain_adapter.get_config().max_priority_fee {
                Some(fee) => {
                    let priority_fee_gwei = ethers::utils::parse_units(fee.to_string(), "gwei")?;
                    priority_fee_gwei
                },
                None => U256::from(1500000000), // 1.5 gwei mặc định
            };
            
            // Max fee = base fee + priority fee
            let max_fee = boosted_gas_price + priority_fee;
            
            (max_fee, priority_fee)
        } else {
            (boosted_gas_price, U256::zero())
        };
        
        // Trả về gas price, max fee, priority fee
        Ok((boosted_gas_price, max_fee_per_gas, priority_fee_per_gas))
    }
    
    /// Tính ước lượng chi phí giao dịch bằng USD
    async fn estimate_transaction_cost_usd(&self, gas_limit: u64, gas_price: U256) -> Result<f64, Box<dyn std::error::Error>> {
        // Tính tổng chi phí gas
        let total_gas_cost = gas_price * U256::from(gas_limit);
        
        // Đổi từ wei sang ether
        let gas_cost_eth = ethers::utils::format_units(total_gas_cost, "ether")?;
        let gas_cost_eth: f64 = gas_cost_eth.parse()?;
        
        // Lấy giá native token
        let eth_price = match self.chain_adapter.get_native_token_price().await {
            Ok(price) => price,
            Err(_) => 2000.0, // Giá mặc định nếu không lấy được
        };
        
        // Tính chi phí USD
        let gas_cost_usd = gas_cost_eth * eth_price;
        
        Ok(gas_cost_usd)
    }

    pub async fn initialize_mempool_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mempool_tracker = Arc::new(Mutex::new(MempoolTracker::new(
            self.config.min_large_tx_amount
        )));
        
        // Đăng ký callback từ mempool_tracker đến token_status_tracker
        {
            let token_tracker = self.token_status_tracker.clone();
            let chain_adapter = self.chain_adapter.clone();
            
            let mempool = mempool_tracker.clone();
            let callback = Box::new(move |pending_swap: PendingSwap| {
                let token_addr = pending_swap.token_address.clone();
                let tt = token_tracker.clone();
                let ca = chain_adapter.clone();
                
                tokio::spawn(async move {
                    // Khi phát hiện token mới từ mempool, cập nhật vào token_status_tracker
                    if let Ok(mut tracker) = tt.lock() {
                        // Thêm token vào danh sách theo dõi
                        match tracker.add_token_to_track(&token_addr).await {
                            Ok(_) => {
                                debug!("Đã thêm token {} từ mempool vào theo dõi", token_addr);
                                
                                // Lấy thông tin token
                                if let Ok(token_info) = ca.get_token_info(&token_addr).await {
                                    // Lập tức kiểm tra và cập nhật trạng thái
                                    if let Err(e) = tracker.update_token_status(&token_addr).await {
                                        warn!("Không thể cập nhật trạng thái token: {}", e);
                                    }
                                }
                            },
                            Err(e) => warn!("Không thể thêm token vào theo dõi: {}", e),
                        }
                    }
                });
            });
            
            // Đăng ký callback vào mempool_tracker
            if let Ok(mut tracker) = mempool_tracker.lock() {
                tracker.add_new_token_callback(callback);
            }
        }
        
        // Lưu trữ trong SnipeBot
        self.mempool_tracker = Some(mempool_tracker);
        
        Ok(())
    }

    // Trong SnipeBot
    pub async fn start_auto_trading_system(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Khởi động hệ thống giao dịch tự động...");
        
        // 1. Khởi tạo các thành phần
        self.initialize_mempool_monitoring().await?;
        self.initialize_token_status_tracker().await?;
        self.initialize_risk_analyzer().await?;
        self.initialize_ai_module().await?;
        self.initialize_monte_equilibrium().await?;
        self.initialize_trade_manager().await?;
        self.initialize_auto_tuner().await?;
        
        // 2. Thiết lập các callbacks giữa các thành phần
        self.connect_component_callbacks().await?;
        
        // 3. Bắt đầu các dịch vụ chạy nền
        self.start_background_services().await?;
        
        // 4. Bắt đầu chu trình auto trade
        if self.config.auto_trade_enabled && self.bot_mode == BotMode::Auto {
            self.start_auto_trade_cycle().await?;
        }
        
        info!("Hệ thống giao dịch tự động đã khởi động thành công");
        
        Ok(())
    }

    async fn start_auto_trade_cycle(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Chạy một chu kỳ auto-trade
        loop {
            // 1. Tìm token mới từ mempool
            self.discover_new_tokens_from_mempool().await?;
            
            // 2. Cập nhật trạng thái tất cả token đang theo dõi
            self.update_tracked_tokens_status().await?;
            
            // 3. Thực hiện phân tích AI và đề xuất giao dịch
            let ai_recommendations = self.get_ai_trading_recommendations().await?;
            
            // 4. Tối ưu hóa và thực hiện giao dịch
            for rec in &ai_recommendations {
                if rec.confidence >= self.auto_trade_config.ai_confidence_threshold {
                    // Tối ưu hóa tham số giao dịch
                    let optimized_params = self.monte_equilibrium.optimize_trade_parameters(
                        &rec.token_address,
                        &rec.action_type,
                        rec.confidence
                    ).await?;
                    
                    // Thực hiện giao dịch
                    self.trade_manager.execute_optimized_trade(
                        &rec.token_address,
                        &rec.action_type,
                        &optimized_params
                    ).await?;
                }
            }
            
            // 5. Tự động điều chỉnh các tham số hệ thống
            if self.auto_tuning_enabled {
                self.auto_tuner.optimize_system_parameters().await?;
            }
            
            // 6. Đợi khoảng thời gian trước khi bắt đầu chu kỳ mới
            tokio::time::sleep(tokio::time::Duration::from_secs(self.auto_trade_config.cycle_interval_seconds)).await;
        }
    }

    // Thêm phương thức kiểm tra tích hợp
    pub async fn check_module_integration(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut status = vec![];
        
        // Kiểm tra Chain Adapter
        if self.chain_adapter.is_connected().await {
            status.push("ChainAdapter: Đã kết nối".to_string());
        } else {
            status.push("ChainAdapter: Không kết nối".to_string());
        }
        
        // Kiểm tra TokenStatusTracker
        if let Some(tracker) = &self.token_status_tracker {
            if let Ok(_) = tracker.lock() {
                status.push("TokenStatusTracker: Khả dụng".to_string());
            } else {
                status.push("TokenStatusTracker: Lỗi mutex".to_string());
            }
        } else {
            status.push("TokenStatusTracker: Chưa khởi tạo".to_string());
        }
        
        // Kiểm tra AIModule
        if let Some(ai_module) = &self.ai_module {
            if let Ok(_) = ai_module.lock() {
                status.push("AIModule: Khả dụng".to_string());
            } else {
                status.push("AIModule: Lỗi mutex".to_string());
            }
        } else {
            status.push("AIModule: Chưa khởi tạo".to_string());
        }
        
        // Kiểm tra TradeManager
        if let Some(trade_manager) = &self.trade_manager {
            // Kiểm tra trade manager
            status.push("TradeManager: Khả dụng".to_string());
        } else {
            status.push("TradeManager: Chưa khởi tạo".to_string());
        }
        
        // Kiểm tra các module khác tương tự
        
        Ok(status.join("\n"))
    }

    // Dòng 2671, thêm phương thức sau check_module_integration

    pub async fn check_system_performance(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        // Khởi tạo kết quả
        let mut performance_metrics = HashMap::new();
        
        // Kiểm tra thời gian phản hồi của các RPC endpoint
        let chain_id = self.chain_adapter.get_chain_id().await?;
        let rpc_response_time = self.measure_rpc_response_time().await?;
        performance_metrics.insert("rpc_response_time_ms".to_string(), rpc_response_time);
        
        // Kiểm tra mempool performance
        if let Some(mempool_watcher) = &self.mempool_watcher {
            if let Ok(watcher) = mempool_watcher.try_lock() {
                performance_metrics.insert("mempool_tx_per_second".to_string(), watcher.get_transaction_rate());
                performance_metrics.insert("mempool_pending_count".to_string(), watcher.get_pending_transaction_count() as f64);
            }
        }
        
        // Kiểm tra trạng thái mạng
        let gas_price = self.chain_adapter.get_gas_price().await?;
        performance_metrics.insert("current_gas_price_gwei".to_string(), gas_price.as_u128() as f64 / 1_000_000_000.0);
        
        // Kiểm tra mutex contention
        if let Some(token_status_tracker) = &self.token_status_tracker {
            let start = std::time::Instant::now();
            let _guard = match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                token_status_tracker.lock()
            ).await {
                Ok(_) => {
                    let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
                    performance_metrics.insert("token_tracker_lock_time_ms".to_string(), elapsed);
                    performance_metrics.insert("token_tracker_lock_contention".to_string(), 0.0);
                    None // Không cần giữ khóa
                },
                Err(_) => {
                    performance_metrics.insert("token_tracker_lock_contention".to_string(), 1.0);
                    None
                }
            };
        }
        
        // Kiểm tra AI module performance
        if let Some(ai_module) = &self.ai_module {
            let start = std::time::Instant::now();
            if let Ok(ai) = ai_module.try_lock() {
                let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
                performance_metrics.insert("ai_module_lock_time_ms".to_string(), elapsed);
                
                // Kiểm tra thời gian dự đoán AI
                let test_features = HashMap::from([
                    ("price".to_string(), 1.0),
                    ("volume_24h".to_string(), 1000000.0),
                    ("liquidity".to_string(), 500000.0),
                    ("holders_count".to_string(), 100.0),
                ]);
                
                let predict_start = std::time::Instant::now();
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    ai.model.predict(test_features.clone())
                ).await {
                    Ok(Ok(_)) => {
                        let predict_time = predict_start.elapsed().as_millis() as f64;
                        performance_metrics.insert("ai_prediction_time_ms".to_string(), predict_time);
                    },
                    _ => {
                        performance_metrics.insert("ai_prediction_time_ms".to_string(), -1.0);
                    }
                }
            } else {
                performance_metrics.insert("ai_module_lock_contention".to_string(), 1.0);
            }
        }
        
        // Kiểm tra hiệu suất giao dịch
        if let Some(trade_manager) = &self.trade_manager {
            if let Ok(manager) = trade_manager.try_lock() {
                if let Ok(stats) = manager.get_trade_performance_stats().await {
                    performance_metrics.insert("trade_success_rate".to_string(), stats.success_rate);
                    performance_metrics.insert("average_slippage".to_string(), stats.average_slippage);
                    performance_metrics.insert("average_execution_time_ms".to_string(), stats.average_execution_time_ms);
                }
            }
        }
        
        // Kiểm tra memory usage của các module chính
        performance_metrics.insert("mempool_memory_usage_mb".to_string(), self.estimate_module_memory_usage("mempool"));
        performance_metrics.insert("ai_module_memory_usage_mb".to_string(), self.estimate_module_memory_usage("ai"));
        performance_metrics.insert("monte_equilibrium_memory_usage_mb".to_string(), self.estimate_module_memory_usage("monte"));
        
        Ok(performance_metrics)
    }

    // Đo thời gian phản hồi của RPC endpoint hiện tại
    async fn measure_rpc_response_time(&self) -> Result<f64, Box<dyn std::error::Error>> {
        let start = std::time::Instant::now();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.chain_adapter.get_block_number()
        ).await {
            Ok(Ok(_)) => Ok(start.elapsed().as_micros() as f64 / 1000.0),
            Ok(Err(e)) => Err(format!("Lỗi khi đo RPC response time: {}", e).into()),
            Err(_) => Err("Timeout khi đo RPC response time".into()),
        }
    }

    // Ước tính memory usage của các module chính
    fn estimate_module_memory_usage(&self, module_name: &str) -> f64 {
        // Trong thực tế, cần triển khai đo memory usage thực tế
        // Đây chỉ là placeholder
        match module_name {
            "mempool" => {
                if let Some(mempool) = &self.mempool_watcher {
                    if let Ok(watcher) = mempool.try_lock() {
                        return (watcher.get_pending_transaction_count() * 500) as f64 / (1024.0 * 1024.0);
                    }
                }
            },
            "ai" => {
                // Ước tính 50MB cho AI model
                return 50.0;
            },
            "monte" => {
                // Ước tính 100MB cho Monte Equilibrium
                return 100.0;
            },
            _ => {}
        }
        
        0.0
    }

    // Phương thức hỗ trợ để kiểm tra tích hợp giữa các module
    async fn verify_module_integration(&self) -> String {
        let mut issues = Vec::new();
        
        // Kiểm tra TradeManager <-> AIModule
        if let (Some(trade_manager), Some(ai_module)) = (&self.trade_manager, &self.ai_module) {
            match tokio::time::timeout(
                std::time::Duration::from_secs(1),
                async {
                    let _trade = trade_manager.try_lock();
                    let _ai = ai_module.try_lock();
                }
            ).await {
                Ok(_) => {},
                Err(_) => {
                    issues.push("Tích hợp TradeManager-AIModule: Có thể deadlock");
                }
            }
        }
        
        // Kiểm tra TradeManager <-> MempoolWatcher
        if let (Some(trade_manager), Some(mempool)) = (&self.trade_manager, &self.mempool_watcher) {
            if let (Ok(tm), Ok(mp)) = (trade_manager.try_lock(), mempool.try_lock()) {
                if !tm.is_mempool_integration_valid() || !mp.is_trade_manager_integration_valid() {
                    issues.push("Tích hợp TradeManager-MempoolWatcher: Không hợp lệ");
                }
            } else {
                issues.push("Tích hợp TradeManager-MempoolWatcher: Không thể kiểm tra (lock)");
            }
        }
        
        // Kiểm tra TokenStatusTracker <-> TradeManager
        if let (Some(token_tracker), Some(trade_manager)) = (&self.token_status_tracker, &self.trade_manager) {
            if let (Ok(tt), Ok(tm)) = (token_tracker.try_lock(), trade_manager.try_lock()) {
                // Kiểm tra xem tất cả các token trong vị thế có được theo dõi không
                let all_positions = tm.get_all_positions();
                let tracked_tokens = tt.get_tracked_tokens();
                
                for position in &all_positions {
                    if !tracked_tokens.contains(&position.token_address) {
                        issues.push(format!("Tích hợp TokenTracker-TradeManager: Token {} không được theo dõi", 
                                           position.token_address));
                    }
                }
            } else {
                issues.push("Tích hợp TokenTracker-TradeManager: Không thể kiểm tra (lock)");
            }
        }
        
        if issues.is_empty() {
            "OK - Tất cả các module tích hợp đúng".to_string()
        } else {
            format!("CẢNH BÁO - Phát hiện {} vấn đề: {}", issues.len(), issues.join("; "))
        }
    }

    // Phương thức hỗ trợ để thử dự đoán AI
    async fn test_ai_prediction(&self, features: HashMap<String, f64>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ai_module) = &self.ai_module {
            if let Ok(ai) = ai_module.try_lock() {
                ai.model.predict(features).await?;
                return Ok(());
            }
        }
        
        Err("Không thể dự đoán AI".into())
    }

    async fn get_token_price_usd(&self, token_address: &str) -> Result<f64, Box<dyn std::error::Error>> {
        // Lấy giá token/ETH (hoặc token/BNB...)
        let token_eth_price = self.chain_adapter.get_token_price(token_address, None).await?;
        
        // Lấy giá ETH/USD (hoặc BNB/USD...)
        let eth_price = match self.chain_adapter.get_native_token_price().await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá native token: {}, dùng giá mặc định", e);
                1500.0 // Giá mặc định
            }
        };
        
        // Tính giá token/USD
        Ok(token_eth_price * eth_price)
    }

    async fn get_estimated_cost(&self) -> Result<EstimatedCost, Box<dyn std::error::Error>> {
        let gas_price = match self.chain_adapter.get_gas_price().await {
            Ok(price) => price,
            Err(e) => {
                return Err(format!("Không thể lấy gas price: {}", e).into());
            }
        };
        
        let gas_price_gwei = gas_price.as_u128() as f64 / 1_000_000_000.0;
        let eth_price = match self.chain_adapter.get_native_token_price().await {
            Ok(price) => price,
            Err(e) => {
                warn!("Không thể lấy giá ETH: {}, sử dụng giá mặc định", e);
                1500.0 // Giá mặc định
            }
        };
        
        // Ước tính chi phí cho các loại giao dịch phổ biến
        let swap_gas = 250_000;
        let approve_gas = 65_000;
        
        let swap_cost_eth = (swap_gas as f64) * gas_price_gwei / 1_000_000_000.0;
        let approve_cost_eth = (approve_gas as f64) * gas_price_gwei / 1_000_000_000.0;
        
        Ok(EstimatedCost {
            gas_price_gwei,
            swap_cost_eth,
            approve_cost_eth,
            swap_cost_usd: swap_cost_eth * eth_price,
            approve_cost_usd: approve_cost_eth * eth_price,
        })
    }

    // Kiểm tra chi phí giao dịch so với ETH USD
    async fn validate_transaction_cost_vs_balance(&self, estimated_gas: u64, gas_price_gwei: f64) -> Result<bool, Box<dyn std::error::Error>> {
        // Lấy giá token gốc (ETH, BNB, v.v) bằng cách sử dụng chain_adapter
        let eth_price = match self.chain_adapter.get_native_token_price().await {
            Ok(price) => price,
            Err(e) => {
                error!("Không thể lấy giá token gốc: {}", e);
                return Ok(false);
            }
        };

        // Lấy gas price hiện tại
        let current_gas_price = self.chain_adapter.get_gas_price().await?;
        
        // Lấy số dư token gốc
        let native_balance = self.chain_adapter.get_native_balance().await?;
        
        // Tính toán chi phí giao dịch ước tính bằng USD
        let tx_cost_in_eth = (estimated_gas as f64) * gas_price_gwei * 1e-9;
        let tx_cost_in_usd = tx_cost_in_eth * eth_price;
        
        // Lấy số dư token gốc dưới dạng ETH
        let balance_in_eth = native_balance.as_u128() as f64 / 1e18;
        let balance_in_usd = balance_in_eth * eth_price;
        
        // Kiểm tra xem chi phí giao dịch có vượt quá 20% số dư hiện tại không
        let tx_cost_percent = tx_cost_in_usd / balance_in_usd * 100.0;
        
        if tx_cost_percent > 20.0 {
            warn!(
                "Chi phí giao dịch ({:.2} USD, {:.2}% số dư) quá cao so với số dư ({:.2} USD)",
                tx_cost_in_usd, tx_cost_percent, balance_in_usd
            );
            return Ok(false);
        }
        
        Ok(true)
    }

    // Dữ liệu thống kê của một cặp token
    async fn update_token_pair_stats(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Thực hiện cập nhật thống kê cặp token
        
        Ok(())
    }

    // Enum để xác định nguồn gây ra deadlock
    #[derive(Debug, Clone, Copy)]
    pub enum DeadlockSource {
        TokenTracker,
        TradeManager,
        MempoolWatcher,
        AIModule,
        MonteEquilibrium,
    }

    // Phương thức kiểm tra xem có cần khôi phục deadlock không
    pub fn check_deadlock_recovery_needed(&self, source: DeadlockSource) -> bool {
        match source {
            DeadlockSource::TokenTracker => {
                // Kiểm tra xem TokenTracker có bị treo không
                if let Ok(last_lock_time) = self.last_token_tracker_lock_time.load(std::sync::atomic::Ordering::Relaxed) {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    
                    // Nếu thời gian cuối cùng lock là quá 60 giây trước, coi như bị treo
                    if now - last_lock_time > 60 {
                        warn!("Phát hiện TokenTracker có thể đã bị deadlock (không được cập nhật trong {} giây)", now - last_lock_time);
                        return true;
                    }
                }
                false
            },
            DeadlockSource::TradeManager => {
                // Kiểm tra xem TradeManager có bị treo không
                if let Ok(last_lock_time) = self.last_trade_manager_lock_time.load(std::sync::atomic::Ordering::Relaxed) {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    
                    // Nếu thời gian cuối cùng lock là quá 60 giây trước, coi như bị treo
                    if now - last_lock_time > 60 {
                        warn!("Phát hiện TradeManager có thể đã bị deadlock (không được cập nhật trong {} giây)", now - last_lock_time);
                        return true;
                    }
                }
                false
            },
            _ => false, // Các nguồn khác chưa được xử lý
        }
    }

    // Phương thức khôi phục từ deadlock
    pub async fn recover_from_deadlock(&self, source: DeadlockSource) -> Result<(), Box<dyn std::error::Error>> {
        match source {
            DeadlockSource::TokenTracker => {
                warn!("Đang khôi phục TokenTracker từ deadlock");
                
                // Tạo TokenTracker mới
                if let Some(chain_adapter) = &self.chain_adapter {
                    let config = chain_adapter.get_config();
                    let new_tracker = Arc::new(Mutex::new(TokenStatusTracker::new(
                        chain_adapter.clone(),
                        vec![config.factory_address.clone()],
                        vec![config.router_address.clone()],
                        config.wrapped_native_token.clone(),
                    )?));
                    
                    // Thay thế tracker cũ
                    self.replace_token_tracker(new_tracker).await;
                    
                    // Ghi nhận thời gian khôi phục
                    self.update_token_tracker_lock_time();
                    
                    info!("Đã khôi phục TokenTracker thành công");
                } else {
                    return Err("Không thể khôi phục TokenTracker: ChainAdapter không có sẵn".into());
                }
            },
            DeadlockSource::TradeManager => {
                warn!("Đang khôi phục TradeManager từ deadlock");
                
                // Sử dụng try_write với timeout để tránh deadlock mới
                let trade_manager_write_result = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    async {
                        self.trade_manager.try_write()
                    }
                ).await;
                
                match trade_manager_write_result {
                    Ok(Ok(mut trade_manager_guard)) => {
                        // Tạo TradeManager mới
                        if let Some(chain_adapter) = &self.chain_adapter {
                            let trade_config = trade::trade_logic::TradeConfig {
                                gas_limit: U256::from(400000),
                                gas_price: U256::from(5000000000u64),
                                slippage: 0.5,
                                timeout: 30,
                                auto_approve: true,
                                use_flashbots: self.config.advanced.use_flashbots,
                                emergency_sell_gas_multiplier: 1.5,
                                router_address: self.config.router_address.clone(),
                                wrapped_native_token: self.config.wrapped_native_token.clone(),
                                max_slippage: 2.0,
                                twap_window_size: 10,
                                twap_min_samples: 5,
                                twap_update_interval: 60,
                            };
                            
                            let new_manager = Arc::new(Mutex::new(TradeManager::new(
                                Arc::new(chain_adapter.clone()),
                                trade_config,
                            )));
                            
                            // Cập nhật TradeManager
                            *trade_manager_guard = Some(new_manager.clone());
                            
                            // Cập nhật timestamp để biết đã khôi phục thành công
                            self.update_trade_manager_lock_time();
                            
                            info!("Đã khôi phục TradeManager thành công");
                            return Ok(());
                        }
                        return Err("Không thể khôi phục TradeManager: Thiếu các component".into());
                    },
                    _ => {
                        error!("Không thể khôi phục TradeManager: không thể lấy write lock");
                        return Err("Không thể khôi phục TradeManager: không thể lấy write lock".into());
                    }
                }
            },
            // Tương tự cho các nguồn khác
            _ => return Err(format!("Chưa hỗ trợ khôi phục từ nguồn: {:?}", source).into()),
        }
        
        Ok(())
    }
    
    // Phương thức thay thế token tracker
    async fn replace_token_tracker(&self, new_tracker: Arc<Mutex<TokenStatusTracker>>) {
        // Sử dụng RwLock thay vì unsafe
        if let Ok(mut tracker_guard) = self.token_status_tracker.write() {
            // Cố gắng copy dữ liệu từ tracker cũ nếu có thể
            if let Some(old_tracker) = tracker_guard.as_ref() {
                match old_tracker.try_lock() {
                    Ok(tracker) => {
                        // Copy dữ liệu
                        if let Ok(mut new_tracker_guard) = new_tracker.try_lock() {
                            // Copy dữ liệu từ tracker cũ sang mới
                            for token_address in tracker.get_tracked_tokens() {
                                if let Ok(token_status) = tracker.get_token(&token_address).await {
                                    if let Some(status) = token_status {
                                        if let Err(e) = new_tracker_guard.add_token(&token_address).await {
                                            warn!("Không thể sao chép token {}: {}", token_address, e);
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(_) => {
                        warn!("Không thể lấy dữ liệu từ tracker cũ do deadlock");
                    }
                }
            }
            
            // Gán tracker mới an toàn
            *tracker_guard = Some(new_tracker);
        }
    }
    
    // Phương thức thay thế trade manager
    async fn replace_trade_manager(&self, new_manager: Arc<Mutex<TradeManager<ChainAdapterEnum>>>) {
        // Sử dụng try_write với timeout để tránh bị treo
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async {
                if let Ok(mut manager_guard) = self.trade_manager.try_write() {
                    *manager_guard = Some(new_manager);
                    self.update_trade_manager_lock_time();
                    return true;
                }
                false
            }
        ).await {
            Ok(true) => {
                info!("Đã thay thế TradeManager thành công");
            },
            _ => {
                error!("Không thể thay thế TradeManager: không thể lấy write lock");
            }
        }
    }
    
    // Ghi nhận thời gian khóa thành công
    fn update_token_tracker_lock_time(&self) {
        // Sử dụng Atomic thay vì unsafe
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.last_token_tracker_lock_time.store(now, Ordering::SeqCst);
    }
    
    fn update_trade_manager_lock_time(&self) {
        // Sử dụng Atomic thay vì unsafe
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.last_trade_manager_lock_time.store(now, Ordering::SeqCst);
    }
    
    // Phát hiện áp lực bộ nhớ
    pub fn memory_pressure_detected(&self) -> bool {
        // Simple heuristic: Kiểm tra kích thước cache của TokenTracker
        if let Ok(tracker_guard) = self.token_status_tracker.read() {
            if let Some(tracker) = tracker_guard.as_ref() {
                if let Ok(token_tracker) = tracker.try_lock() {
                    token_tracker.get_cache_size() > 10000
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    }
}

// Thêm các struct phụ trợ

#[derive(Debug, Clone)]
pub struct SandwichOpportunity {
    pub token_address: String,
    pub victim_swap: PendingSwap,
    pub potential_profit: f64,
    pub estimated_price_impact: f64,
}

// Hàm tạo một instance mới của SnipeBot
pub async fn create_new_snipebot(config: Config) -> Result<Arc<SnipeBot>, Box<dyn std::error::Error>> {
    // Tạo storage manager
    let storage = Arc::new(Storage::new(config.db_path.clone()));
    
    // Tạo instance của SnipeBot
    let snipebot = SnipeBot::new(config, storage).await?;
    
    Ok(Arc::new(snipebot))
}

#[derive(Debug, Clone)]
struct AIDecision {
    should_buy: bool,
    confidence: f64,
    prediction: String,
}

// Thiết lập cấu hình subscription
pub fn set_subscription_config(&mut self, config: SubscriptionTradeConfig) {
    self.subscription_config = config;
}

// Tạo một API endpoint để cập nhật cấp độ người dùng
pub async fn update_user_subscription(&mut self, username: &str, new_level: SubscriptionLevel, duration_days: u64) -> Result<(), Box<dyn std::error::Error>> {
    // Tìm người dùng trong database
    if let Some(user_manager) = &self.user_manager {
        let mut manager = user_manager.lock().await;
        
        if let Some(user) = manager.get_user_mut(username) {
            // Cập nhật subscription
            user.subscription = Subscription::new(new_level, duration_days);
            
            // Lưu thay đổi
            manager.save_users().await?;
            
            // Nếu người dùng hiện tại, cập nhật cấp độ trong SnipeBot
            if let Some(current_user) = &self.current_user {
                if current_user.username == username {
                    self.set_user_level(new_level);
                }
            }
            
            return Ok(());
        }
        
        return Err(format!("Không tìm thấy người dùng {}", username).into());
    }
    
    Err("User manager chưa được khởi tạo".into())
}

// Triển khai API endpoint để bắt đầu auto trade
pub async fn start_auto_trade(&mut self, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Kiểm tra cấp độ người dùng
    if let Some(user_manager) = &self.user_manager {
        let manager = user_manager.lock().await;
        
        if let Some(user) = manager.get_user(username) {
            // Thiết lập user level
            self.set_user_level(user.subscription.level.clone());
            
            // Chuyển sang chế độ Auto
            self.set_bot_mode(BotMode::Auto);
            
            // Bắt đầu dịch vụ auto trade
            self.start_auto_trade_service().await?;
            
            return Ok(());
        }
        
        return Err(format!("Không tìm thấy người dùng {}", username).into());
    }
    
    Err("User manager chưa được khởi tạo".into())
}

async fn with_timeout<T>(future: impl Future<Output = T>, duration: Duration) -> Result<T, tokio::time::error::Elapsed> {
    tokio::time::timeout(duration, future).await
}

// Hàm ví dụ để minh họa cách sử dụng with_timeout
async fn example_with_timeout<T>(mutex: Arc<Mutex<T>>) -> Option<MutexGuard<T>> {
    match with_timeout(mutex.lock(), Duration::from_secs(5)).await {
        Ok(Ok(guard)) => Some(guard),
        Ok(Err(e)) => {
            error!("Lỗi khi lấy lock: {}", e);
            None
        },
        Err(_) => {
            error!("Timeout khi lấy lock");
            None
        }
    }
}

/// Hàm retry giao dịch blockchain
async fn retry_blockchain_tx<F, T, E>(
    operation: F,
    max_retries: usize,
    initial_backoff_ms: u64,
) -> Result<T, Box<dyn std::error::Error>>
where
    F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>> + Send + Sync,
    E: std::fmt::Display + 'static,
{
    let mut retries = 0;
    let mut backoff_ms = initial_backoff_ms;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if retries >= max_retries {
                    return Err(format!("Đã hết số lần thử lại: {}", e).into());
                }

                warn!("Lỗi giao dịch (lần thử {}): {}. Thử lại sau {} ms", retries + 1, e, backoff_ms);
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;

                retries += 1;
                backoff_ms *= 2; // Tăng thời gian chờ theo cấp số nhân
            }
        }
    }
}
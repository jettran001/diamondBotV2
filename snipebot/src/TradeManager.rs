use tokio::sync::Mutex;
use crate::types::{OrderStatus, OrderType};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ethers::types::{Address, H256, U256, TransactionRequest, TransactionReceipt, Log};
use ethers::utils;
use ethers::providers::{Http, Provider, Middleware};
use serde::{Serialize, Deserialize};
use log::{info, warn, debug, error};
use uuid::Uuid;
use anyhow::{Result, anyhow};
use crate::mempool::SandwichResult;
use crate::trade_logic::TradeResult;
use crate::auto_tuning::AutoTuner;
use crate::types::{SandwichParams, TradeConfig, OptimizedStrategy, FrontrunParams, StrategyType};
use async_trait::async_trait;
use crate::trade_logic::TradeType;
use std::future::Future;
use std::pin::Pin;
use std::marker::Unpin;
use std::str::FromStr;
use ethers::signers::Signer as EthersSigner;
use ethers::providers::JsonRpcClient;
use serde_json;
use std::sync::RwLock;
use crate::token_status::TokenStatus;
use crate::types;
use crate::ChainAdapter;
use crate::types::{self, TradeStats};

// Định nghĩa struct ArbitrageParams
#[derive(Clone, Debug)]
pub struct ArbitrageParams {
    pub token_address: String,
    pub route: Vec<String>,
    pub amount: String,
    pub min_profit: f64,
    pub max_slippage: f64,
}

// Cấu trúc TradeManager cho quản lý giao dịch
pub struct TradeManager<C: ChainAdapter + ?Sized> {
    chain_adapter: Box<C>,
    wallet_address: Option<Address>,
    slippage_percent: f64,
    gas_price_multiplier: f64,
    router_address: Option<Address>,
    token_status_tracker: Arc<Mutex<Box<dyn AsyncTokenStatusTracker>>>,
    stats: RwLock<TradeStats>,
    active_trades: RwLock<HashMap<String, TradeTaskInfo>>,
    positions: HashMap<String, TokenPosition>,
    limit_orders: HashMap<String, LimitOrder>,
    auto_sandwich_configs: HashMap<String, AutoSandwichConfig>,
    price_monitor: Option<Arc<dyn PriceMonitor>>,
    auto_tuner: Option<Arc<Mutex<AutoTuner>>>,
    config: TradeManagerConfig,
}

// Cấu trúc cho vị thế token
#[derive(Clone, Debug)]
pub struct TokenPosition {
    pub token_address: String,
    pub token_amount: f64,
    pub entry_price_usd: f64,
    pub current_price_usd: f64,
    pub last_updated: u64,
}

// Cấu trúc cho đơn hàng giới hạn
#[derive(Clone, Debug)]
pub struct LimitOrder {
    pub id: String,
    pub token_address: String,
    pub order_type: OrderType,
    pub price_target: f64,
    pub percent: u8, // Phần trăm số lượng token cần giao dịch
    pub created_at: u64,
    pub expires_at: u64,
    pub status: OrderStatus,
}

// Cấu trúc cho cấu hình sandwich tự động
#[derive(Clone, Debug)]
pub struct AutoSandwichConfig {
    pub token_address: String,
    pub max_additional_buys: u32,
    pub buys_completed: u32,
    pub start_time: u64,
    pub end_time: u64,
    pub is_active: bool,
    pub min_victim_size_usd: f64,
    pub use_flashbots: bool,
}

// Cấu trúc cho cấu hình quản lý giao dịch
#[derive(Clone, Debug)]
pub struct TradeManagerConfig {
    pub default_slippage: f64,
    pub min_sandwich_victim_usd: f64,
    pub max_pending_orders: usize,
    pub enable_auto_tuning: bool,
}

// Định nghĩa TradePerformance struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePerformance {
    pub token_address: String,
    pub strategy_type: types::StrategyType,
    pub entry_price: f64,
    pub exit_price: f64,
    pub entry_time: u64,
    pub exit_time: u64,
    pub roi: f64,
    pub profit: f64,
    pub position_size: f64,
    pub gas_cost: f64,
    pub tx_hash: String,
    pub success: bool,
    pub tags: Vec<String>,
    pub meta: serde_json::Value,
}

// TradeResult struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub amount_in: String,
    pub amount_out: Option<String>,
    pub price_per_token: Option<f64>,
    pub gas_used: Option<u64>,
    pub gas_price: u64,
    pub timestamp: u64,
    pub error: Option<String>,
    pub trade_type: TradeType,
    pub token_address: String,
    pub victim_tx_hash: Option<String>,
    pub profit_usd: Option<f64>,
    pub gas_cost_usd: Option<f64>,
}

// SandwichResult struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichResult {
    pub token_address: String,
    pub victim_tx_hash: String,
    pub front_tx_hash: Option<String>,
    pub back_tx_hash: Option<String>,
    pub success: bool,
    pub profit_usd: f64,
    pub front_run_confirmation_time: u64,
    pub total_execution_time: u64,
    pub timestamp: u64,
    pub profit: f64,
    pub front_run_gas_cost: f64,
    pub back_run_gas_cost: f64,
    pub execution_time: u64,
}

// Cập nhật trait TokenStatusTracker
#[async_trait]
pub trait TokenStatusTracker: Send + Sync + 'static {
    async fn add_token_to_track(&mut self, token_address: &str) -> Result<(), Box<dyn std::error::Error>>;
    async fn get_token_status(&self, token_address: &str) -> Result<Option<TokenStatus>, Box<dyn std::error::Error>>;
    async fn update_all_tokens(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Clone)]
pub enum TokenStatusTrackerEnum {
    Default(Arc<Mutex<DefaultTokenStatusTracker>>),
    // Thêm các loại khác nếu cần
}

pub struct DefaultTokenStatusTracker;

#[async_trait]
impl TokenStatusTracker for DefaultTokenStatusTracker {
    async fn add_token_to_track(&mut self, token_address: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Triển khai cụ thể
        Ok(())
    }
    
    async fn get_token_status(&self, token_address: &str) -> Result<Option<TokenStatus>, Box<dyn std::error::Error>> {
        // Triển khai cụ thể
        Ok(None)
    }
    
    async fn update_all_tokens(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Triển khai cụ thể
        Ok(())
    }
}

// Trait cho theo dõi giá
pub trait PriceMonitor: Send + Sync {
    fn add_price_alert(&self, token_address: &str, price: f64, alert_id: String) -> Box<dyn Future<Output = Result<(), Box<dyn std::error::Error>>> + Send + Unpin>;
}

// Cập nhật trait với async_trait
pub trait TradePerformanceStorage: Send + Sync {
    async fn add_performance_record(&mut self, performance: TradePerformance) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Clone)]
pub enum TradePerformanceStorageEnum {
    Default(Arc<Mutex<DefaultTradePerformanceStorage>>),
    // Thêm các loại khác nếu cần
}

pub struct DefaultTradePerformanceStorage;

#[async_trait]
impl TradePerformanceStorage for DefaultTradePerformanceStorage {
    async fn add_performance_record(&mut self, performance: TradePerformance) -> Result<(), Box<dyn std::error::Error>> {
        // Triển khai cụ thể
        Ok(())
    }
}

// Cập nhật trait cho AutoTuner
#[async_trait]
pub trait AutoTuner: Send + Sync {
    async fn report_trade_result(&mut self, result: TradeResult) -> Result<(), Box<dyn std::error::Error>>;
    async fn update_trading_parameters(&mut self, gas_multiplier: f64, slippage: f64, retry_count: u32) -> Result<(), Box<dyn std::error::Error>>;
}

impl<C: ChainAdapter> TradeManager<C> {
    // Di chuyển các phương thức vào đây
    pub async fn buy_token_with_optimized_params(&self, token_address: &str, amount: &str, gas_price: Option<u64>) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Kiểm tra đầu vào
        if token_address.trim().is_empty() {
            return Err("Địa chỉ token không được để trống".into());
        }
        
        // Kiểm tra xem token_address có phải là địa chỉ hợp lệ
        if let Err(e) = Address::from_str(token_address) {
            return Err(format!("Địa chỉ token không hợp lệ: {}", e).into());
        }
        
        if amount.trim().is_empty() {
            return Err("Số lượng token không được để trống".into());
        }
        
        // Xử lý parse amount an toàn hơn
        let amount_in = match ethers::utils::parse_ether(amount) {
            Ok(amount) => amount,
            Err(e) => return Err(format!("Số lượng không hợp lệ ({}): {}", amount, e).into())
        };
        
        // Kiểm tra số lượng token đầu vào
        if amount_in.is_zero() {
            return Err("Số lượng token đầu vào phải lớn hơn 0".into());
        }
        
        // Kiểm tra gas_price nếu được cung cấp
        if let Some(price) = gas_price {
            if price == 0 {
                return Err("Gas price phải lớn hơn 0".into());
            }
        }
        
        // Lấy gas limit phù hợp
        let gas_limit = match self.estimate_gas_for_swap(token_address, amount_in).await {
            Ok(limit) => limit,
            Err(e) => {
                warn!("Không thể ước tính gas limit cho token {}: {}", token_address, e);
                return Err(format!("Không thể ước tính gas limit: {}", e).into());
            }
        };
        
        // Kiểm tra gas_limit
        if gas_limit == 0 {
            return Err("Gas limit không hợp lệ".into());
        }
        
        // Thực hiện mua với retry tự động
        let mut attempts = 0;
        let max_attempts = 3;
        
        while attempts < max_attempts {
            match self.buy_token(token_address, amount_in, gas_price, Some(gas_limit)).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(format!("Không thể mua token sau {} lần thử: {}", max_attempts, e).into());
                    }
                    warn!("Lỗi khi mua token (lần thử {}): {}", attempts, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }
        
        // Không bao giờ đạt đến đây nhưng để đảm bảo tính đầy đủ
        Err("Lỗi không xác định khi mua token".into())
    }
    
    // Thêm các phương thức execute_optimized_strategy
    pub async fn execute_optimized_strategy(&self, strategy: &OptimizedStrategy) -> Result<TradeResult, Box<dyn std::error::Error>> {
        match &strategy.params {
            crate::types::StrategyParams::DirectBuy { amount, gas_price } => {
                self.buy_token_with_optimized_params(
                    &strategy.token_address,
                    amount,
                    *gas_price
                ).await
            },
            crate::types::StrategyParams::Sandwich(params) => {
                // Chuyển đổi SandwichResult sang TradeResult
                let sandwich_result = self.execute_sandwich_attack(params).await?;
                
                // Tính toán tổng gas cost
                let total_gas_cost = sandwich_result.front_run_gas_cost + sandwich_result.back_run_gas_cost;
                
                Ok(TradeResult {
                    success: sandwich_result.success,
                    tx_hash: sandwich_result.front_tx_hash.clone(),
                    amount_in: "0".to_string(), // Không có thông tin chính xác
                    amount_out: None,
                    price_per_token: None,
                    gas_used: None,
                    gas_price: 0, // Không có thông tin chính xác về gas price đã sử dụng
                    timestamp: sandwich_result.timestamp,
                    error: None,
                    trade_type: TradeType::Sandwich,
                    token_address: sandwich_result.token_address.clone(),
                    victim_tx_hash: Some(sandwich_result.victim_tx_hash.clone()),
                    profit_usd: Some(sandwich_result.profit_usd),
                    gas_cost_usd: Some(total_gas_cost), // Chuyển đổi gas cost từ ETH sang USD
                })
            },
            crate::types::StrategyParams::Frontrun(params) => {
                self.execute_frontrun_strategy(params).await
            },
            crate::types::StrategyParams::Arbitrage(_) => {
                Err("Chiến lược Arbitrage chưa được hỗ trợ".into())
            },
        }
    }
    
    // Tự động theo dõi token
    pub async fn auto_track_position_tokens(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tracker) = &self.token_status_tracker {
            for (token_address, _position) in &self.positions {
                // Clone dữ liệu cần thiết trước khi lấy lock
                let token_to_track = token_address.clone();
                
                // Lấy lock trong phạm vi giới hạn
                let mut tracker_guard = match tracker.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Không thể lấy lock cho token tracker: {}", e);
                        continue; // Bỏ qua token này và tiếp tục với token tiếp theo
                    }
                };
                
                // Xử lý async trong phạm vi nhỏ nhất có thể
                if let Err(e) = tracker_guard.add_token_to_track(&token_to_track).await {
                    warn!("Không thể theo dõi token {}: {}", token_to_track, e);
                }
                
                // Lock tự động được giải phóng khi ra khỏi phạm vi
            }
        }
        
        Ok(())
    }
    
    pub async fn execute_sandwich_attack(&self, params: &SandwichParams) -> Result<SandwichResult, Box<dyn std::error::Error>> {
        // Kiểm tra tham số đầu vào
        if params.victim_tx_hash.trim().is_empty() {
            return Err("Victim transaction hash không được để trống".into());
        }
        
        if params.token_address.trim().is_empty() {
            return Err("Token address không được để trống".into());
        }
        
        if params.front_run_gas_multiplier <= 0.0 {
            return Err("Front run gas multiplier phải lớn hơn 0".into());
        }
        
        if params.back_run_gas_multiplier <= 0.0 {
            return Err("Back run gas multiplier phải lớn hơn 0".into());
        }
        
        // 1. Lấy thông tin về giao dịch victim
        let victim_tx_hash = H256::from_str(&params.victim_tx_hash)?;
        let provider = self.chain_adapter.get_provider();
        let victim_tx = provider
            .get_transaction(victim_tx_hash).await?
            .ok_or("Không tìm thấy victim transaction")?;
        
        let token_address = params.token_address.clone();
        
        // 2. Tính toán gas price cho frontrun - sử dụng cách an toàn hơn
        let victim_gas_price = victim_tx.gas_price.unwrap_or_default();
        let front_run_gas = if let Some(gas_u128) = victim_gas_price.as_u128().checked_mul(
            (params.front_run_gas_multiplier * 1_000_000.0) as u128
        ) {
            U256::from(gas_u128 / 1_000_000)
        } else {
            return Err("Overflow khi tính front run gas price".into());
        };
        
        // 3. Tính toán lượng token cần mua
        let optimal_amount = self.calculate_frontrun_amount(
            &token_address,
            params.front_run_amount_percent,
            victim_tx.value.as_u128()
        ).await?;
        
        // 4. Thực hiện frontrun
        let front_run_result = if params.use_flashbots {
            self.execute_flashbots_buy(
                &token_address, 
                optimal_amount, 
                Some(front_run_gas.as_u64())
            ).await?
        } else {
            self.buy_token(
                &token_address, 
                optimal_amount, 
                Some(front_run_gas.as_u64()), 
                None
            ).await?
        };
        
        // 5. Theo dõi trạng thái frontrun
        if let Some(tx_hash_str) = &front_run_result.tx_hash {
            self.monitor_transaction(tx_hash_str).await?;
        }
        
        // 6. Theo dõi victim transaction (sử dụng mempool watcher)
        let victim_confirmed = self.wait_for_transaction(victim_tx_hash).await?;
        
        // 7. Nếu victim không xác nhận, hủy kế hoạch và bán
        if !victim_confirmed {
            // Giảm slippage để đảm bảo bán thành công
            let emergency_config = TradeConfig {
                slippage: 5.0, // Tăng slippage trong trường hợp khẩn cấp
                gas_limit: None,
                gas_price: None,
            };
            
            // Xử lý an toàn hơn khi parse chuỗi
            let amount_to_sell = match &front_run_result.amount_out {
                Some(amount_str) => match amount_str.parse::<f64>() {
                    Ok(amount) => amount,
                    Err(_) => {
                        warn!("Không thể parse số lượng token để bán: {}", amount_str);
                        0.0
                    }
                },
                None => 0.0
            };
            
            let emergency_sell = self.sell_token(
                &token_address,
                amount_to_sell,
                &emergency_config
            ).await?;
            
            // Lấy thời gian hiện tại an toàn hơn
            let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_secs(),
                Err(_) => 0,
            };
            
            // Tính toán gas cost
            let front_run_gas_cost = match (front_run_result.gas_used, front_run_result.gas_price) {
                (Some(gas), price) if gas > 0 && price > 0 => {
                    (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
                },
                _ => front_run_result.gas_cost_usd.unwrap_or(0.0),
            };
            
            let back_run_gas_cost = match (emergency_sell.gas_used, emergency_sell.gas_price) {
                (Some(gas), price) if gas > 0 && price > 0 => {
                    (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
                },
                _ => emergency_sell.gas_cost_usd.unwrap_or(0.0),
            };
            
            let execution_time = match front_run_result.timestamp {
                0 => 0,
                timestamp if timestamp <= current_time => current_time - timestamp,
                _ => 0,
            };
            
            // Tính profit (luôn âm trong trường hợp lỗi)
            let profit_eth = -1.0 * (front_run_gas_cost + back_run_gas_cost);
            let eth_price = match self.get_eth_price().await {
                Ok(price) => price,
                Err(_) => 2000.0, // Giá ETH mặc định nếu không lấy được
            };
            let profit_usd = profit_eth * eth_price;
            
            let result = SandwichResult {
                token_address: token_address.clone(),
                victim_tx_hash: params.victim_tx_hash.clone(),
                front_tx_hash: front_run_result.tx_hash.clone(),
                back_tx_hash: emergency_sell.tx_hash.clone(),
                success: false,
                profit_usd,
                front_run_confirmation_time: 0,
                total_execution_time: execution_time,
                timestamp: current_time,
                profit: profit_eth,
                front_run_gas_cost,
                back_run_gas_cost,
                execution_time,
            };
            
            // Báo cáo kết quả không thành công
            // Thực hiện hai tác vụ này bên ngoài phần xử lý chính để tránh lỗi ảnh hưởng đến kết quả
            if let Err(e) = self.store_trade_performance(&result).await {
                warn!("Không thể lưu kết quả giao dịch: {}", e);
            }
            
            if let Err(e) = self.report_to_auto_tuner(&result).await {
                warn!("Không thể báo cáo đến AutoTuner: {}", e);
            }
            
            return Ok(result);
        }
        
        // 8. Thực hiện backrun - sử dụng cách an toàn hơn
        let back_run_gas = if let Some(gas_u128) = victim_gas_price.as_u128().checked_mul(
            (params.back_run_gas_multiplier * 1_000_000.0) as u128
        ) {
            U256::from(gas_u128 / 1_000_000)
        } else {
            return Err("Overflow khi tính back run gas price".into());
        };
        
        // 9. Bán token với backrun
        let back_run_result = if params.use_flashbots {
            self.execute_flashbots_sell(
                &token_address,
                100.0,  // Bán 100% số lượng
                Some(back_run_gas.as_u64())
            ).await?
        } else {
            // Tạo TradeConfig đơn giản hơn - chỉ sử dụng các trường cần thiết
            self.sell_token(
                &token_address,
                100.0,  // Bán 100% số lượng
                &TradeConfig {
                    slippage: params.max_slippage,
                    gas_limit: Some(300000),  // Giá trị mặc định hợp lý
                    gas_price: Some(back_run_gas.as_u64())
                }
            ).await?
        };
        
        // 10. Tính toán lợi nhuận
        let eth_price = self.get_eth_price().await?;
        
        // Tính gas cost từ front_run và back_run
        let front_run_gas_cost = match (front_run_result.gas_used, front_run_result.gas_price) {
            (Some(gas), price) if gas > 0 && price > 0 => {
                (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
            },
            _ => 0.0, // Giá trị mặc định nếu không có thông tin
        };
        
        let back_run_gas_cost = match (back_run_result.gas_used, back_run_result.gas_price) {
            (Some(gas), price) if gas > 0 && price > 0 => {
                (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
            },
            _ => 0.0, // Giá trị mặc định nếu không có thông tin
        };
        
        // Tính profit từ back_run_result
        let profit_eth = match (back_run_result.amount_out, front_run_result.amount_in.parse::<f64>()) {
            (Some(amount_out_str), Ok(amount_in)) => {
                match amount_out_str.parse::<f64>() {
                    Ok(amount_out) => amount_out - amount_in,
                    Err(_) => 0.0,
                }
            },
            _ => 0.0,
        };
        
        let profit_usd = profit_eth * eth_price;
        
        // Lấy thời gian hiện tại an toàn hơn
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => 0,
        };
        
        let execution_time = match front_run_result.timestamp {
            0 => 0,
            timestamp if timestamp <= current_time => current_time - timestamp,
            _ => 0,
        };
        
        // 11. Xây dựng kết quả
        let result = SandwichResult {
            token_address: token_address.clone(),
            victim_tx_hash: params.victim_tx_hash.clone(),
            front_tx_hash: front_run_result.tx_hash.clone(),
            back_tx_hash: back_run_result.tx_hash.clone(),
            success: true,
            profit_usd,
            front_run_confirmation_time: 0, // Cần tính toán thực tế
            total_execution_time: execution_time,
            timestamp: current_time,
            profit: profit_eth,
            front_run_gas_cost,
            back_run_gas_cost,
            execution_time,
        };
        
        // Thực hiện báo cáo riêng biệt để không ảnh hưởng đến kết quả chính
        // Tránh lỗi ảnh hưởng đến kết quả
        if let Err(e) = self.store_trade_performance(&result).await {
            warn!("Không thể lưu kết quả giao dịch: {}", e);
        }
        
        if let Err(e) = self.report_to_auto_tuner(&result).await {
            warn!("Không thể báo cáo đến AutoTuner: {}", e);
        }
        
        Ok(result)
    }
    
    async fn store_trade_performance(&self, sandwich_result: &SandwichResult) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(storage) = &self.trade_performance_storage {
            // Tạo bản sao của dữ liệu cần thiết trước khi lấy lock
            let performance = TradePerformance {
                token_address: sandwich_result.token_address.clone(),
                strategy_type: types::StrategyType::Sandwich,
                entry_price: 0.0, // Cần cập nhật
                exit_price: 0.0,  // Cần cập nhật
                entry_time: sandwich_result.timestamp,
                exit_time: sandwich_result.timestamp + sandwich_result.total_execution_time,
                roi: if sandwich_result.success { 
                    (sandwich_result.profit * 100.0) / (sandwich_result.front_run_gas_cost + sandwich_result.back_run_gas_cost) 
                } else { 
                    -100.0 
                },
                profit: sandwich_result.profit,
                position_size: 0.0, // Cần cập nhật
                gas_cost: sandwich_result.front_run_gas_cost + sandwich_result.back_run_gas_cost,
                tx_hash: sandwich_result.front_tx_hash.clone().unwrap_or_default(),
                success: sandwich_result.success,
                tags: vec!["sandwich".to_string()],
                meta: serde_json::json!({
                    "victim_tx": sandwich_result.victim_tx_hash,
                    "back_run_tx": sandwich_result.back_tx_hash,
                }),
            };
            
            // Lấy lock nhưng giảm thiểu phạm vi của lock
            let mut storage_guard = match storage.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Không thể lấy lock để lưu kết quả: {}", e);
                    return Err("Lock error in storage".into());
                }
            };
            
            // Gọi phương thức async trực tiếp
            storage_guard.add_performance_record(performance).await?;
            
            // Lock tự động được drop khi ra khỏi phạm vi
        }
        
        Ok(())
    }
    
    async fn report_to_auto_tuner(&self, sandwich_result: &SandwichResult) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(auto_tuner) = &self.auto_tuner {
            // Kiểm tra token_address trước khi xử lý
            if sandwich_result.token_address.trim().is_empty() {
                warn!("Token address trống khi báo cáo cho AutoTuner");
                return Err("Token address không được để trống".into());
            }
            
            // Tính toán tổng gas cost
            let total_gas_cost = sandwich_result.front_run_gas_cost + sandwich_result.back_run_gas_cost;
            
            // Kiểm tra và chuẩn bị dữ liệu trước khi lấy lock
            let trade_result = TradeResult {
                token_address: sandwich_result.token_address.clone(),
                success: sandwich_result.success,
                tx_hash: match &sandwich_result.front_tx_hash {
                    Some(hash) if !hash.trim().is_empty() => Some(hash.clone()),
                    _ => {
                        warn!("Front tx hash trống khi báo cáo cho AutoTuner");
                        return Err("Front transaction hash không được để trống".into());
                    }
                },
                victim_tx_hash: if sandwich_result.victim_tx_hash.trim().is_empty() {
                    warn!("Victim tx hash trống khi báo cáo cho AutoTuner");
                    return Err("Victim transaction hash không được để trống".into());
                } else {
                    Some(sandwich_result.victim_tx_hash.clone())
                },
                amount_in: "0".to_string(), // Không có thông tin chính xác
                amount_out: None,
                price_per_token: None,
                gas_used: None,
                gas_price: 0,
                gas_cost_usd: Some(total_gas_cost),
                error: None,
                trade_type: TradeType::Sandwich,
                timestamp: sandwich_result.timestamp,
                profit_usd: Some(sandwich_result.profit_usd),
            };
            
            // Lấy lock để thực hiện tác vụ nhanh nhất có thể
            let mut auto_tuner_guard = match auto_tuner.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Không thể lấy lock để báo cáo kết quả: {}", e);
                    return Err("Lock error in auto_tuner".into());
                }
            };
            
            // Gọi phương thức async trực tiếp
            auto_tuner_guard.report_trade_result(trade_result).await?;
            
            // Lock tự động được drop khi ra khỏi phạm vi
        }
        
        Ok(())
    }
    
    // Tạo đơn hàng giới hạn
    pub async fn create_limit_order(&mut self, token_address: &str, order_type: OrderType, price_target: f64, percent: u8, time_limit_seconds: u64) -> Result<String, Box<dyn std::error::Error>> {
        let order_id = Uuid::new_v4().to_string();
        
        // Lấy thời gian hiện tại an toàn
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!("Lỗi khi lấy thời gian hiện tại: {}", err);
                // Fallback: Sử dụng timestamp cố định nếu không thể lấy thời gian hiện tại
                // Giá trị này chỉ là giá trị an toàn, có thể không chính xác về mặt thời gian
                1609459200 // 2021-01-01 00:00:00 UTC
            }
        };
        
        // Thêm vào danh sách đơn hàng
        let order = LimitOrder {
            id: order_id.clone(),
            token_address: token_address.to_string(),
            order_type,
            price_target,
            percent,
            created_at: current_time,
            expires_at: current_time + time_limit_seconds,
            status: OrderStatus::Active,
        };
        
        // Cập nhật danh sách đơn hàng
        self.limit_orders.insert(order_id.clone(), order);
        
        // Thiết lập cảnh báo giá
        if let Some(price_monitor) = &self.price_monitor {
            let future = price_monitor.add_price_alert(token_address, price_target, order_id.clone());
            future.await?;
        }
        
        Ok(order_id)
    }
    
    // Kích hoạt chế độ sandwich tự động cho token
    pub async fn enable_auto_sandwich(&mut self, token_address: &str, max_buys: u32, time_limit_seconds: u64) 
                                      -> Result<(), Box<dyn std::error::Error>> {
        // Lấy thời gian hiện tại an toàn
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!("Lỗi khi lấy thời gian hiện tại: {}", err);
                // Fallback: Sử dụng timestamp cố định
                1609459200 // 2021-01-01 00:00:00 UTC
            }
        };
        
        // Thiết lập cấu hình sandwich tự động
        let auto_sandwich_config = AutoSandwichConfig {
            token_address: token_address.to_string(),
            max_additional_buys: max_buys,
            buys_completed: 0,
            start_time: current_time,
            end_time: current_time + time_limit_seconds,
            is_active: true,
            min_victim_size_usd: self.config.min_sandwich_victim_usd,
            use_flashbots: self.network_congestion_level() > 7, // Sử dụng flashbots khi mạng tắc nghẽn
        };
        
        // Lưu cấu hình
        self.auto_sandwich_configs.insert(token_address.to_string(), auto_sandwich_config);
        
        // Kích hoạt dịch vụ theo dõi mempool nếu chưa chạy
        Ok(())
    }
    
    // Định nghĩa các phương thức phụ trợ thiếu
    async fn estimate_gas_for_swap(&self, _token_address: &str, _amount: U256) -> Result<u64, Box<dyn std::error::Error>> {
        // Phương thức giả định
        Ok(300000) // Gas limit mặc định
    }
    
    async fn buy_token(&self, _token_address: &str, _amount: U256, _gas_price: Option<u64>, _gas_limit: Option<u64>) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức buy_token")
    }
    
    async fn calculate_frontrun_amount(&self, _token_address: &str, _front_run_amount_percent: f64, _victim_value: u128) -> Result<U256, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức calculate_frontrun_amount")
    }
    
    async fn execute_flashbots_buy(&self, _token_address: &str, _amount: U256, _gas_price: Option<u64>) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức execute_flashbots_buy")
    }
    
    async fn monitor_transaction(&self, _tx_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Phương thức giả định
        Ok(())
    }
    
    async fn wait_for_transaction(&self, _tx_hash: H256) -> Result<bool, Box<dyn std::error::Error>> {
        // Phương thức giả định
        Ok(true)
    }
    
    async fn sell_token(&self, token_address: &str, amount: f64, config: &TradeConfig) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức sell_token")
    }
    
    async fn execute_flashbots_sell(&self, _token_address: &str, _amount: f64, _gas_price: Option<u64>) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức execute_flashbots_sell")
    }
    
    async fn get_eth_price(&self) -> Result<f64, Box<dyn std::error::Error>> {
        // Phương thức giả định
        Ok(2000.0) // Giá ETH mặc định
    }
    
    fn get_token_position(&self, token_address: &str) -> Result<TokenPosition, Box<dyn std::error::Error>> {
        // Phương thức giả định
        self.positions.get(token_address)
            .cloned()
            .ok_or_else(|| format!("Không tìm thấy vị thế cho token {}", token_address).into())
    }
    
    fn network_congestion_level(&self) -> u64 {
        // Phương thức giả định
        5 // Mức độ tắc nghẽn mạng mặc định (1-10)
    }
    
    pub async fn execute_frontrun_strategy(&self, _params: &crate::types::FrontrunParams) -> Result<TradeResult, Box<dyn std::error::Error>> {
        // Phương thức giả định
        unimplemented!("Chưa triển khai phương thức execute_frontrun_strategy")
    }

    /// Chuyển đổi từ TradeResult sang SandwichResult
    pub fn convert_trade_result_to_sandwich(&self, trade_result: &TradeResult, back_tx_hash: Option<String>) -> Result<SandwichResult, Box<dyn std::error::Error>> {
        // Kiểm tra nếu là loại giao dịch sandwich
        if trade_result.trade_type != TradeType::Sandwich {
            return Err("Không thể chuyển đổi: TradeResult không phải là loại Sandwich".into());
        }
        
        // Kiểm tra thông tin bắt buộc
        let victim_tx_hash = match &trade_result.victim_tx_hash {
            Some(hash) if !hash.trim().is_empty() => hash.clone(),
            _ => return Err("Thiếu victim_tx_hash trong TradeResult".into())
        };
        
        // Tính toán thời gian thực hiện
        let execution_time = match trade_result.gas_used {
            Some(gas) => {
                // Ước tính thời gian dựa trên gas (giả sử 1 block = 15s, thời gian xử lý = gas/21000 * 15s)
                // Đây chỉ là ước tính đơn giản, trong thực tế cần thuật toán phức tạp hơn
                (gas as f64 / 21000.0 * 15.0).round() as u64
            },
            None => 30 // Giá trị mặc định nếu không có thông tin gas
        };
        
        // Tạo SandwichResult từ TradeResult
        let sandwich_result = SandwichResult {
            token_address: trade_result.token_address.clone(),
            victim_tx_hash,
            front_tx_hash: trade_result.tx_hash.clone(),
            back_tx_hash,
            success: trade_result.success,
            profit_usd: trade_result.profit_usd.unwrap_or(0.0),
            front_run_confirmation_time: 0, // Không có thông tin chính xác
            total_execution_time: execution_time,
            timestamp: trade_result.timestamp,
            profit: 0.0, // Cần tính toán dựa trên giá ETH
            front_run_gas_cost: trade_result.gas_cost_usd.unwrap_or(0.0) / 2.0, // Chia đều cho front và back run
            back_run_gas_cost: trade_result.gas_cost_usd.unwrap_or(0.0) / 2.0,
            execution_time,
        };
        
        Ok(sandwich_result)
    }

    /// Tạo SandwichResult từ kết quả front_run và back_run
    pub async fn create_sandwich_result_from_trades(
        &self,
        victim_tx_hash: &str,
        front_run_result: &TradeResult,
        back_run_result: &TradeResult,
        success: bool
    ) -> Result<SandwichResult, Box<dyn std::error::Error>> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let execution_time = back_run_result.timestamp - front_run_result.timestamp;
        
        // Lấy front_run gas cost
        let front_run_gas_cost = match (front_run_result.gas_used, front_run_result.gas_price) {
            (Some(gas), price) if gas > 0 && price > 0 => {
                (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
            },
            _ => front_run_result.gas_cost_usd.unwrap_or(0.0),
        };
        
        // Lấy back_run gas cost
        let back_run_gas_cost = match (back_run_result.gas_used, back_run_result.gas_price) {
            (Some(gas), price) if gas > 0 && price > 0 => {
                (gas as f64 * price as f64) / 1e9 // Chuyển đổi từ gwei sang ETH
            },
            _ => back_run_result.gas_cost_usd.unwrap_or(0.0),
        };
        
        // Tính profit (ước tính dựa trên profit_usd)
        let eth_price = match self.get_eth_price().await {
            Ok(price) => price,
            Err(_) => 2000.0, // Giá ETH mặc định nếu không lấy được
        };
        
        let profit_usd = back_run_result.profit_usd.unwrap_or(0.0);
        let profit = if eth_price > 0.0 { profit_usd / eth_price } else { 0.0 };
        
        // Tạo SandwichResult
        let result = SandwichResult {
            token_address: front_run_result.token_address.clone(),
            victim_tx_hash: victim_tx_hash.to_string(),
            front_tx_hash: front_run_result.tx_hash.clone(),
            back_tx_hash: back_run_result.tx_hash.clone(),
            success,
            profit_usd,
            front_run_confirmation_time: 0, // Không có thông tin chính xác
            total_execution_time: execution_time,
            timestamp,
            profit,
            front_run_gas_cost,
            back_run_gas_cost,
            execution_time,
        };
        
        Ok(result)
    }
}

// Thêm từ std::str::FromStr, vì có chỗ sử dụng H256::from_str
use std::str::FromStr;

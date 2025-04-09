// Standard library imports
use std::sync::Arc;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::sync::{Mutex, RwLock};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::collections::VecDeque;

// Third party imports
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use ethers::providers::{Provider, Http, Middleware};
use ethers::types::U64;
use lazy_static::lazy_static;
use log::{info, warn};
use serde::{Serialize, Deserialize};
use tokio::time::sleep;
use tracing::{error};

// Internal imports
use crate::chain_adapters::ChainAdapter;
use crate::gas_optimizer::GasOptimizer;
use crate::mempool::{MempoolWatcher, MempoolTracker, PendingSwap, SandwichResult};
use crate::token_status::TokenStatusTracker;
use crate::utils::RetryConfig;
use crate::metrics::RETRY_METRICS;
use crate::MonteEquilibrium::MonteCarloSimulator;
use crate::types::SandwichPreferredParams;
use crate::trade_logic::{TradeResult, TradeType};

// Theo dõi thời gian trung bình để hoàn thành block
static AVG_BLOCK_TIME_MS: AtomicU64 = AtomicU64::new(12000); // Mặc định 12s cho Ethereum
static NETWORK_CONGESTION_LEVEL: AtomicU64 = AtomicU64::new(1); // 1-10, 1=thấp, 10=cao

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTuningConfig {
    pub enabled: bool,
    pub update_interval: Duration,
    pub max_adjustment: f64,
    pub min_adjustment: f64,
    pub learning_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTuningStats {
    pub total_adjustments: u64,
    pub successful_adjustments: u64,
    pub failed_adjustments: u64,
    pub average_performance: f64,
    pub timestamp: u64,
}

pub struct AutoTuner {
    provider: Provider<Http>,
    check_interval: Duration,
    last_check: Instant,
    last_block_number: U64,
    last_block_time: Instant,
    trade_results: VecDeque<TradeResult>,
    config: AutoTunerConfig,
    network_congestion_level: Arc<AtomicU64>,
    ai_module: Option<Arc<Mutex<dyn AIAnalyzer>>>,
    mempool_tracker: Option<Arc<Mutex<MempoolTracker>>>,
    monte_equilibrium: Option<Arc<Mutex<MonteCarloSimulator>>>,
    runtime_retry_config: Arc<RwLock<RetryConfig>>,
    tuning_config: AutoTuningConfig,
    tuning_stats: AutoTuningStats,
}

// Định nghĩa trait AIAnalyzer
pub trait AIAnalyzer: Send + Sync {
    fn get_confidence_threshold(&self) -> Box<dyn std::future::Future<Output = f64> + Send + Unpin>;
    fn set_confidence_threshold(&mut self, threshold: f64) -> Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error>>> + Send + Unpin>;
}

// Thêm struct cho cấu hình AutoTuner
#[derive(Clone, Debug)]
pub struct AutoTunerConfig {
    pub min_results_for_adjustment: usize,
    pub max_trade_history: usize,
    pub min_ai_accuracy: f64,
    pub min_sandwich_profit_usd: f64,
    pub min_sandwich_victim_usd: f64,
}

impl Default for AutoTunerConfig {
    fn default() -> Self {
        Self {
            min_results_for_adjustment: 10,
            max_trade_history: 100,
            min_ai_accuracy: 0.7,
            min_sandwich_profit_usd: 0.5,
            min_sandwich_victim_usd: 10.0,
        }
    }
}

impl Default for AutoTuningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            update_interval: Duration::from_secs(3600), // 1 giờ
            max_adjustment: 0.2, // Điều chỉnh tối đa 20%
            min_adjustment: 0.01, // Điều chỉnh tối thiểu 1%
            learning_rate: 0.05, // Tốc độ học 5%
        }
    }
}

impl Default for AutoTuningStats {
    fn default() -> Self {
        Self {
            total_adjustments: 0,
            successful_adjustments: 0,
            failed_adjustments: 0,
            average_performance: 0.0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

impl AutoTuner {
    pub fn new(provider: Provider<Http>) -> Self {
        // Tạo cấu hình retry mặc định
        let default_retry_config = RetryConfig::default();
        
        Self {
            provider,
            check_interval: Duration::from_secs(30),
            last_check: Instant::now(),
            last_block_number: U64::zero(),
            last_block_time: Instant::now(),
            trade_results: VecDeque::new(),
            config: AutoTunerConfig::default(),
            network_congestion_level: Arc::new(NETWORK_CONGESTION_LEVEL),
            ai_module: None,
            mempool_tracker: None,
            monte_equilibrium: None,
            runtime_retry_config: Arc::new(RwLock::new(default_retry_config)),
            tuning_config: AutoTuningConfig::default(),
            tuning_stats: AutoTuningStats::default(),
        }
    }
    
    // Khởi động tuning service
    pub async fn start(mut self) {
        info!("Khởi động Auto-Tuner service");
        
        loop {
            sleep(Duration::from_secs(5)).await;
            
            if self.last_check.elapsed() >= self.check_interval {
                let network_conditions = self.check_network_conditions().await;
                // Không cần truyền network_conditions vào adjust_retry_config vì phương thức này không cần tham số
                self.adjust_retry_config().await;
                self.last_check = Instant::now();
                
                // Thực hiện auto-tuning nếu được bật
                if self.tuning_config.enabled {
                    if let Err(e) = self.analyze_and_adjust().await {
                        error!("Lỗi khi thực hiện auto-tuning: {}", e);
                    }
                }
            }
        }
    }
    
    // Kiểm tra điều kiện mạng
    async fn check_network_conditions(&mut self) -> NetworkConditions {
        let mut conditions = NetworkConditions::default();
        
        match self.provider.get_block_number().await {
            Ok(block_number) => {
                if self.last_block_number != U64::zero() {
                    let blocks_passed = block_number.as_u64() - self.last_block_number.as_u64();
                    
                    if blocks_passed > 0 {
                        let time_passed = self.last_block_time.elapsed().as_millis() as u64;
                        let avg_time = time_passed / blocks_passed;
                        
                        // Cập nhật thời gian block trung bình
                        let current_avg = AVG_BLOCK_TIME_MS.load(Ordering::SeqCst);
                        let new_avg = (current_avg * 9 + avg_time) / 10; // EMA-10
                        AVG_BLOCK_TIME_MS.store(new_avg, Ordering::SeqCst);
                        
                        // Ước tính độ tắc nghẽn mạng
                        let congestion_level = self.estimate_congestion_level(new_avg).await;
                        self.network_congestion_level.store(congestion_level, Ordering::SeqCst);
                        
                        info!("Network stats: avg_block_time={}ms, congestion_level={}/10", 
                             new_avg, congestion_level);
                        
                        // Cập nhật thông tin cho NetworkConditions
                        conditions.avg_block_time = new_avg;
                        conditions.congestion_level = congestion_level;
                    }
                }
                
                self.last_block_number = block_number;
                self.last_block_time = Instant::now();
            },
            Err(e) => {
                warn!("Không thể lấy block number: {}", e);
            }
        }
        
        // Lấy gas price hiện tại
        if let Ok(gas_price) = self.provider.get_gas_price().await {
            conditions.gas_price = gas_price.as_u64();
        }
        
        // Lấy tỷ lệ thành công của retry
        conditions.retry_success_rate = RETRY_METRICS.get_success_rate();
        
        conditions
    }
    
    // Ước tính độ tắc nghẽn mạng dựa vào các yếu tố
    async fn estimate_congestion_level(&self, avg_block_time_ms: u64) -> u64 {
        // Kiểm tra gas price trung bình
        let gas_price = match self.provider.get_gas_price().await {
            Ok(price) => price.as_u64(),
            Err(_) => 0,
        };
        
        // Lấy tỷ lệ thành công của retry
        let success_rate = RETRY_METRICS.get_success_rate();
        
        // Kết hợp các yếu tố để ước tính độ tắc nghẽn
        let base_block_time = 12000; // 12s cho Ethereum
        let block_time_factor = if avg_block_time_ms > base_block_time {
            let ratio = avg_block_time_ms as f64 / base_block_time as f64;
            (ratio.min(3.0) - 1.0) * 5.0 // 0-10 scale
        } else {
            0.0
        };
        
        // Gas price factor
        let gas_price_factor = if gas_price > 0 {
            let base_gas = 50_000_000_000; // 50 Gwei
            let ratio = gas_price as f64 / base_gas as f64;
            (ratio.min(5.0) - 1.0).max(0.0) * 2.0 // 0-8 scale
        } else {
            0.0
        };
        
        // Success rate factor
        let success_factor = if success_rate < 80.0 {
            (80.0 - success_rate) / 8.0 // 0-10 scale
        } else {
            0.0
        };
        
        // Kết hợp các yếu tố và làm tròn
        let congestion = (block_time_factor + gas_price_factor + success_factor).min(10.0);
        congestion.round() as u64
    }
    
    // Điều chỉnh cấu hình retry dựa vào điều kiện mạng
    async fn adjust_retry_config(&self) {
        let congestion_level = self.network_congestion_level.load(Ordering::SeqCst);
        
        // Thay đổi cách lấy cấu hình hiện tại, sử dụng biến cục bộ
        let mut runtime_config = RetryConfig::default();
        
        // Sử dụng self.runtime_retry_config thay vì RUNTIME_RETRY_CONFIG
        if let Ok(config_read) = self.runtime_retry_config.read() {
            runtime_config = config_read.clone();
        }
        
        // Cấu hình mới dựa vào độ tắc nghẽn
        let new_config = match congestion_level {
            1..=3 => { // Mạng nhẹ
                RetryConfig {
                    max_attempts: 3,
                    initial_backoff_ms: 1000,
                    max_backoff_ms: 30000,
                    backoff_multiplier: 2.0,
                    jitter_factor: 0.1,
                }
            },
            4..=6 => { // Mạng trung bình
                RetryConfig {
                    max_attempts: 5,
                    initial_backoff_ms: 2000,
                    max_backoff_ms: 60000,
                    backoff_multiplier: 2.0,
                    jitter_factor: 0.2,
                }
            },
            7..=8 => { // Mạng tắc nghẽn
                RetryConfig {
                    max_attempts: 8,
                    initial_backoff_ms: 3000,
                    max_backoff_ms: 120000,
                    backoff_multiplier: 2.5,
                    jitter_factor: 0.3,
                }
            },
            9..=10 => { // Mạng tắc nghẽn nghiêm trọng
                RetryConfig {
                    max_attempts: 10,
                    initial_backoff_ms: 5000,
                    max_backoff_ms: 180000,
                    backoff_multiplier: 3.0,
                    jitter_factor: 0.4,
                }
            },
            _ => runtime_config,
        };
        
        // Cập nhật nếu cấu hình thay đổi
        if new_config.max_attempts != runtime_config.max_attempts ||
           new_config.initial_backoff_ms != runtime_config.initial_backoff_ms ||
           new_config.max_backoff_ms != runtime_config.max_backoff_ms ||
           new_config.backoff_multiplier != runtime_config.backoff_multiplier {
            
            // Sử dụng self.runtime_retry_config thay vì RUNTIME_RETRY_CONFIG
            if let Ok(mut config) = self.runtime_retry_config.write() {
                *config = new_config.clone();
            }
            
            info!("Tự động điều chỉnh cấu hình retry: mạng cấp độ {}/10, max_attempts={}, initial_backoff={}ms",
                 congestion_level, new_config.max_attempts, new_config.initial_backoff_ms);
        }
    }

    pub async fn report_trade_result(&mut self, result: TradeResult) -> Result<(), Box<dyn std::error::Error>> {
        // Lưu kết quả vào bộ nhớ đệm
        self.trade_results.push_back(result.clone());
        
        // Giới hạn kích thước bộ nhớ đệm
        while self.trade_results.len() > self.config.max_trade_history {
            self.trade_results.pop_front();
        }
        
        // Phân tích và điều chỉnh nếu đủ dữ liệu
        if self.trade_results.len() >= self.config.min_results_for_adjustment {
            self.analyze_and_adjust().await?;
        }
        
        // Cập nhật theo dõi liên tục cho sandwich attack
        if result.trade_type == TradeType::Sandwich {
            // Chuyển đổi TradeResult thành SandwichResult
            let sandwich_result = self.convert_trade_to_sandwich_result(&result).await?;
            self.monitor_ongoing_sandwich_opportunities(&sandwich_result).await?;
        }
        
        Ok(())
    }

    // Phương thức để phân tích và điều chỉnh tự động các tham số
    pub async fn analyze_and_adjust(&mut self) -> Result<()> {
        // Lấy thời gian hiện tại để so sánh với lần cập nhật cuối
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Kiểm tra xem đã đến thời gian cập nhật chưa
        let last_update = self.tuning_stats.timestamp;
        if current_time - last_update < self.tuning_config.update_interval.as_secs() {
            return Ok(());
        }
        
        // Phân tích hiệu suất hiện tại
        let current_performance = self.analyze_current_performance().await?;
        
        // Điều chỉnh các tham số dựa trên hiệu suất
        let success = self.adjust_parameters(current_performance).await?;
        
        // Cập nhật thống kê
        self.tuning_stats.total_adjustments += 1;
        if success {
            self.tuning_stats.successful_adjustments += 1;
        } else {
            self.tuning_stats.failed_adjustments += 1;
        }
        
        // Cập nhật hiệu suất trung bình
        self.tuning_stats.average_performance = (self.tuning_stats.average_performance * (self.tuning_stats.total_adjustments - 1) as f64 
            + current_performance) / self.tuning_stats.total_adjustments as f64;
        
        // Cập nhật timestamp
        self.tuning_stats.timestamp = current_time;
        
        Ok(())
    }
    
    // Phân tích hiệu suất hiện tại dựa trên các giao dịch gần đây
    async fn analyze_current_performance(&self) -> Result<f64> {
        // Nếu không có đủ kết quả giao dịch để phân tích
        if self.trade_results.len() < self.config.min_results_for_adjustment {
            return Err(anyhow!("Không đủ dữ liệu giao dịch để phân tích hiệu suất"));
        }
        
        // Phân tích các kết quả giao dịch
        let mut total_profit = 0.0;
        let mut count = 0;
        
        for result in &self.trade_results {
            if result.is_successful() {
                total_profit += result.get_profit_usd();
                count += 1;
            }
        }
        
        if count == 0 {
            return Ok(0.0);
        }
        
        // Tính hiệu suất trung bình
        let avg_profit = total_profit / count as f64;
        
        // Lấy mức độ tắc nghẽn mạng hiện tại
        let congestion_level = self.network_congestion_level.load(Ordering::Relaxed);
        
        // Điều chỉnh hiệu suất dựa trên mức độ tắc nghẽn mạng
        // Khi mạng tắc nghẽn cao, hiệu suất tốt sẽ có giá trị cao hơn
        let adjusted_performance = avg_profit * (1.0 + (congestion_level as f64 / 10.0));
        
        Ok(adjusted_performance)
    }
    
    // Điều chỉnh các tham số dựa trên hiệu suất
    async fn adjust_parameters(&mut self, performance: f64) -> Result<bool> {
        // Nếu hiệu suất quá thấp, không điều chỉnh
        if performance < 0.01 {
            return Ok(false);
        }
        
        // Điều chỉnh AI confidence threshold nếu có AI module
        if let Some(ai_module) = &self.ai_module {
            let mut ai = ai_module.lock().unwrap();
            let current_threshold = ai.get_confidence_threshold().await;
            
            // Tính toán ngưỡng mới dựa trên hiệu suất
            let performance_factor = performance.min(1.0).max(0.0);
            let adjustment = self.tuning_config.learning_rate * performance_factor;
            
            // Giới hạn trong phạm vi điều chỉnh
            let clamped_adjustment = adjustment.min(self.tuning_config.max_adjustment)
                                             .max(self.tuning_config.min_adjustment);
            
            // Áp dụng điều chỉnh
            let new_threshold = current_threshold * (1.0 + clamped_adjustment);
            let new_threshold = new_threshold.min(0.95).max(0.5); // Giới hạn trong khoảng hợp lý
            
            // Cập nhật ngưỡng mới
            if let Err(e) = ai.set_confidence_threshold(new_threshold).await {
                warn!("Không thể cập nhật ngưỡng tin cậy AI: {:?}", e);
                return Ok(false);
            }
            
            info!("Đã điều chỉnh ngưỡng tin cậy AI từ {:.2} thành {:.2}", current_threshold, new_threshold);
        }
        
        // Điều chỉnh các tham số Monte Carlo nếu có
        if let Some(monte) = &self.monte_equilibrium {
            let mut simulator = monte.lock().unwrap();
            
            // Điều chỉnh số lần mô phỏng dựa trên hiệu suất
            let current_sims = simulator.get_simulation_count();
            let performance_factor = performance.min(1.0).max(0.0);
            
            // Nếu hiệu suất cao, giảm số lần mô phỏng để tăng tốc độ
            // Nếu hiệu suất thấp, tăng số lần mô phỏng để tăng độ chính xác
            let adjustment_factor = if performance_factor > 0.8 {
                0.9 // Giảm 10%
            } else if performance_factor < 0.3 {
                1.2 // Tăng 20%
            } else {
                1.0 // Giữ nguyên
            };
            
            let new_sims = (current_sims as f64 * adjustment_factor) as u32;
            let new_sims = new_sims.min(10000).max(1000); // Giới hạn trong khoảng hợp lý
            
            simulator.set_simulation_count(new_sims);
            info!("Đã điều chỉnh số lần mô phỏng Monte Carlo từ {} thành {}", current_sims, new_sims);
        }
        
        // Điều chỉnh các tham số khác nếu cần
        
        Ok(true)
    }
    
    // Lấy thống kê auto-tuning hiện tại
    pub fn get_tuning_stats(&self) -> AutoTuningStats {
        self.tuning_stats.clone()
    }
    
    // Cập nhật cấu hình auto-tuning
    pub fn update_tuning_config(&mut self, config: AutoTuningConfig) {
        self.tuning_config = config;
        info!("Đã cập nhật cấu hình auto-tuning");
    }

    async fn monitor_ongoing_sandwich_opportunities(&self, last_result: &SandwichResult) -> Result<(), Box<dyn std::error::Error>> {
        // Nếu giao dịch sandwich gần đây thành công và có lợi nhuận tốt
        if last_result.success && last_result.profit_usd > self.config.min_sandwich_profit_usd {
            // Kiểm tra xem có cơ hội sandwich tiếp theo với cùng token không
            if let Some(mempool_tracker) = &self.mempool_tracker {
                let mempool = mempool_tracker.lock().unwrap();
                
                // Tìm các giao dịch mua lớn khác với cùng token
                if let Some(swaps) = mempool.pending_swaps.get(&last_result.token_address) {
                    let potential_victims: Vec<&PendingSwap> = swaps.iter()
                        .filter(|swap| swap.is_buy && swap.amount_usd >= self.config.min_sandwich_victim_usd)
                        .collect();
                    
                    if !potential_victims.is_empty() {
                        // Nếu có cơ hội mới, điều chỉnh tham số dựa trên kết quả gần nhất
                        let adjusted_params = self.adjust_sandwich_params_based_on_result(last_result).await?;
                        
                        // Gửi đề xuất mới đến Monte Equilibrium
                        if let Some(monte) = &self.monte_equilibrium {
                            let mut monte = monte.lock().unwrap();
                            
                            // Đặt các tham số ưu tiên dựa trên kết quả trước đó
                            monte.set_preferred_sandwich_params(&adjusted_params).await?;
                        }
                        
                        info!("Phát hiện cơ hội sandwich liên tiếp cho token {}, điều chỉnh tham số", 
                               last_result.token_address);
                    }
                }
            }
        }
        
        Ok(())
    }

    async fn adjust_sandwich_params_based_on_result(&self, result: &SandwichResult) -> Result<SandwichPreferredParams, Box<dyn std::error::Error>> {
        // Điều chỉnh tham số dựa trên kết quả trước đó
        
        // Nếu lợi nhuận cao, tăng tỷ lệ phần trăm frontrun
        let front_run_percent_adjustment = if result.profit_usd > 2.0 * self.config.min_sandwich_profit_usd {
            1.1 // Tăng 10%
        } else {
            1.0 // Giữ nguyên
        };
        
        // Nếu giao dịch frontrun mất nhiều thời gian, tăng gas multiplier
        let gas_multiplier_adjustment = if result.front_run_confirmation_time > 15000 { // 15 giây
            1.15 // Tăng 15%
        } else {
            1.0 // Giữ nguyên
        };
        
        // Tạo tham số ưu tiên mới
        let preferred_params = SandwichPreferredParams {
            front_run_amount_percent_multiplier: front_run_percent_adjustment,
            front_run_gas_multiplier_adjustment: gas_multiplier_adjustment,
            back_run_gas_multiplier_adjustment: 1.0, // Giữ nguyên
            use_flashbots: self.network_congestion_level.load(Ordering::SeqCst) > 7,
        };
        
        Ok(preferred_params)
    }

    async fn analyze_trade_performance(&self) -> Result<PerformanceMetrics, Box<dyn std::error::Error>> {
        // Thực hiện phân tích hiệu suất dựa trên các giao dịch gần đây
        let mut metrics = PerformanceMetrics::default();
        
        // Tạo placeholder cho việc phân tích
        if !self.trade_results.is_empty() {
            let mut success_count = 0;
            let mut ai_correct_count = 0;
            let mut total_profit = 0.0;
            let mut total_gas_cost = 0.0;
            
            for result in &self.trade_results {
                if result.is_successful() {
                    success_count += 1;
                    total_profit += result.get_profit_usd();
                }
                
                if result.was_ai_prediction_correct() {
                    ai_correct_count += 1;
                }
                
                total_gas_cost += result.get_gas_cost_usd();
            }
            
            metrics.success_rate = success_count as f64 / self.trade_results.len() as f64;
            metrics.avg_profit = if success_count > 0 { total_profit / success_count as f64 } else { 0.0 };
            metrics.ai_accuracy = ai_correct_count as f64 / self.trade_results.len() as f64;
            metrics.avg_gas_cost = total_gas_cost / self.trade_results.len() as f64;
        }
        
        Ok(metrics)
    }
    
    async fn adjust_gas_strategy(&self, 
        metrics: &PerformanceMetrics, 
        network_conditions: &NetworkConditions
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Placeholder implementation
        info!("Điều chỉnh chiến lược gas dựa trên hiệu suất và điều kiện mạng");
        Ok(())
    }
    
    async fn adjust_slippage_strategy(&self, 
        metrics: &PerformanceMetrics
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Placeholder implementation
        info!("Điều chỉnh chiến lược slippage dựa trên hiệu suất");
        Ok(())
    }
    
    async fn adjust_position_size_strategy(&self, 
        metrics: &PerformanceMetrics
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Placeholder implementation
        info!("Điều chỉnh chiến lược kích thước vị thế dựa trên hiệu suất");
        Ok(())
    }
    
    async fn save_updated_config(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Placeholder implementation
        info!("Lưu cấu hình tự động điều chỉnh mới");
        Ok(())
    }

    // Phương thức mới để chuyển đổi TradeResult thành SandwichResult
    async fn convert_trade_to_sandwich_result(&self, trade_result: &TradeResult) -> Result<SandwichResult, Box<dyn std::error::Error>> {
        // Tạo một SandwichResult đơn giản từ TradeResult
        let result = SandwichResult {
            token_address: trade_result.token_address.clone(),
            victim_tx_hash: trade_result.victim_tx_hash.clone().unwrap_or_default(),
            front_tx_hash: trade_result.tx_hash.clone(),
            back_tx_hash: None, // Không có thông tin
            success: trade_result.success,
            profit_usd: trade_result.profit_usd.unwrap_or(0.0),
            front_run_confirmation_time: 0, // Không có thông tin
            total_execution_time: 0, // Không có thông tin
            timestamp: trade_result.timestamp,
            profit: 0.0, // Không có thông tin chi tiết
            front_run_gas_cost: 0.0, // Không có thông tin chi tiết
            back_run_gas_cost: 0.0, // Không có thông tin chi tiết
            execution_time: 0, // Không có thông tin chi tiết
        };
        
        Ok(result)
    }
}

// Thêm structs cho các metrics
#[derive(Debug, Default, Clone)]
pub struct PerformanceMetrics {
    pub success_rate: f64,
    pub avg_profit: f64,
    pub ai_accuracy: f64,
    pub avg_gas_cost: f64,
    pub avg_execution_time: f64,
    pub revert_rate: f64,
}

#[derive(Debug, Default, Clone)]
pub struct NetworkConditions {
    pub avg_block_time: u64,
    pub congestion_level: u64,
    pub gas_price: u64,
    pub retry_success_rate: f64,
}

// Thêm trait extension cho TradeResult để hỗ trợ phân tích
trait TradeResultAnalysis {
    fn is_successful(&self) -> bool;
    fn get_profit_usd(&self) -> f64;
    fn was_ai_prediction_correct(&self) -> bool;
    fn get_gas_cost_usd(&self) -> f64;
}

impl TradeResultAnalysis for TradeResult {
    fn is_successful(&self) -> bool {
        self.success
    }
    
    fn get_profit_usd(&self) -> f64 {
        // Tạm thời trả về 0, cần cải thiện khi có thông tin thêm về lợi nhuận
        match self.amount_out {
            Some(ref amount_out) if self.price_per_token.is_some() => {
                // Tính toán lợi nhuận dựa trên amount_out và price_per_token
                let price = self.price_per_token.unwrap_or(0.0);
                // Giả định amount_out là số token, đơn giản hóa biến đổi từ String sang f64
                match amount_out.parse::<f64>() {
                    Ok(amount) => amount * price,
                    Err(_) => 0.0
                }
            },
            _ => 0.0
        }
    }
    
    fn was_ai_prediction_correct(&self) -> bool {
        // Cần triển khai logic phức tạp hơn khi có thông tin AI prediction
        // Hiện tại, giả định là đúng nếu giao dịch thành công
        self.success
    }
    
    fn get_gas_cost_usd(&self) -> f64 {
        // Ước lượng chi phí gas
        match self.gas_used {
            Some(gas) => {
                let gas_price_gwei = self.gas_price as f64 / 1_000_000_000.0; // Convert to Gwei
                let eth_price_usd = 3000.0; // Giả định, cần cập nhật với giá ETH thực tế
                let gas_cost_eth = (gas as f64 * gas_price_gwei) / 1_000_000_000.0;
                gas_cost_eth * eth_price_usd
            },
            None => 0.0
        }
    }
}

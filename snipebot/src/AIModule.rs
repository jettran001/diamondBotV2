use ethers::types::{Address, U256};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::Mutex;
use tracing::{info, warn};
use crate::error::TransactionError;
use crate::types::{
    TradingStrategy, 
    AIPrediction, 
    AIAnalysisResult, 
    TokenStatus, 
    TokenRiskAnalysis,
    PendingSwap,
    AIDecision
};
use async_trait::async_trait;
use std::sync::Arc;
use crate::mempool::MempoolWatcher;
use crate::MonteEquilibrium::GameTheoryOptimizer as MonteEquilibriumOptimizer;
use crate::TradeManager::TradeManager as TradeManagerType;
use crate::ChainAdapter;

/// Các dự đoán AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIPrediction {
    /// Xác suất tăng giá
    pub up_probability: f64,
    /// Xác suất giảm giá
    pub down_probability: f64,
    /// Xác suất đi ngang
    pub sideways_probability: f64,
    /// Chiến lược gợi ý
    pub suggested_strategy: TradingStrategy,
    /// Độ tin cậy (0-1)
    pub confidence: f64,
    /// Thời gian dự đoán
    pub timestamp: u64,
}

impl Default for AIPrediction {
    fn default() -> Self {
        Self {
            up_probability: 0.33,
            down_probability: 0.33,
            sideways_probability: 0.34,
            suggested_strategy: TradingStrategy::Monitor,
            confidence: 0.0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Kết quả phân tích AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAnalysisResult {
    pub token_address: String,
    pub risk_score: u8,
    pub price_trend: String, // "up", "down", "stable"
    pub confidence: f64,
    pub recommendation: String,
}

pub struct AIModule {
    model: AIModelEnum,
    config: AIModuleConfig,
    mempool_tracker: Option<Arc<Mutex<MempoolWatcher>>>,
    monte_equilibrium: Option<Arc<MonteEquilibriumOptimizer>>,
    trade_manager: Option<Arc<Mutex<TradeManagerType<dyn ChainAdapter + Send + Sync>>>>,
    bot_mode: BotMode,
    trade_history_db: Option<Arc<Mutex<TradeHistoryDB>>>,
}

pub struct AIModuleConfig {
    pub auto_trade_enabled: bool,
    pub auto_trade_threshold: f64,
    pub max_position_size_percent: f64,
    pub min_sandwich_victim_usd: f64,
    pub min_frontrun_target_usd: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BotMode {
    Manual,
    Auto,
    SemiAuto,
}

#[async_trait]
pub trait AIModel: Send + Sync {
    async fn predict(&self, features: HashMap<String, f64>) -> Result<AIPrediction, Box<dyn std::error::Error + Send + Sync>>;
}

// Thêm một enum để bọc các loại AIModel
#[derive(Clone)]
pub enum AIModelEnum {
    SimpleModel(Arc<SimpleAIModel>),
    NeuralModel(Arc<NeuralAIModel>),
    Default(Arc<DefaultAIModel>),
    // Thêm các loại model khác nếu cần
}

#[async_trait]
impl AIModel for AIModelEnum {
    async fn predict(&self, features: HashMap<String, f64>) -> Result<AIPrediction, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            AIModelEnum::SimpleModel(model) => model.predict(features).await,
            AIModelEnum::NeuralModel(model) => model.predict(features).await,
            AIModelEnum::Default(model) => model.predict(features).await,
        }
    }
}

// Cấu trúc SimpleAIModel
pub struct SimpleAIModel;

#[async_trait]
impl AIModel for SimpleAIModel {
    async fn predict(&self, features: HashMap<String, f64>) -> Result<AIPrediction, Box<dyn std::error::Error + Send + Sync>> {
        // Triển khai đơn giản
        Ok(AIPrediction::default())
    }
}

// Cấu trúc NeuralAIModel
pub struct NeuralAIModel;

#[async_trait]
impl AIModel for NeuralAIModel {
    async fn predict(&self, features: HashMap<String, f64>) -> Result<AIPrediction, Box<dyn std::error::Error + Send + Sync>> {
        // Triển khai phức tạp hơn
        Ok(AIPrediction::default())
    }
}

// Cấu trúc DefaultAIModel
pub struct DefaultAIModel;

#[async_trait]
impl AIModel for DefaultAIModel {
    async fn predict(&self, features: HashMap<String, f64>) -> Result<AIPrediction, Box<dyn std::error::Error + Send + Sync>> {
        // Triển khai mặc định
        Ok(AIPrediction::default())
    }
}

impl AIModule {
    pub fn new(
        model: AIModelEnum,
        config: AIModuleConfig,
        mempool_tracker: Option<Arc<Mutex<MempoolWatcher>>>,
        monte_equilibrium: Option<Arc<MonteEquilibriumOptimizer>>,
        trade_manager: Option<Arc<Mutex<TradeManagerType<dyn ChainAdapter + Send + Sync>>>>,
    ) -> Self {
        Self {
            model,
            config,
            mempool_tracker,
            monte_equilibrium,
            trade_manager,
            bot_mode: BotMode::Manual,
            trade_history_db: None,
        }
    }

    pub fn set_bot_mode(&mut self, mode: BotMode) {
        self.bot_mode = mode;
    }
    
    pub async fn analyze_token(
        &mut self, 
        token_address: &str, 
        token_status: &TokenStatus, 
        risk_analysis: Option<&TokenRiskAnalysis>
    ) -> Result<AIAnalysisResult, Box<dyn std::error::Error>> {
        // Kiểm tra token_address có hợp lệ không
        if token_address.trim().is_empty() || !token_address.starts_with("0x") {
            return Err(format!("Địa chỉ token không hợp lệ: {}", token_address).into());
        }
        
        // Kiểm tra token_status có đủ dữ liệu không
        if token_status.current_price.is_none() && token_status.price_usd.is_none() {
            warn!("Token {} thiếu thông tin giá", token_address);
        }
        
        // Tạo đầu vào AI
        let mut features = HashMap::new();
        
        // Thêm các tính năng từ TokenStatus
        features.insert("price".to_string(), token_status.current_price.unwrap_or(0.0));
        features.insert("volume_24h".to_string(), token_status.volume_24h.unwrap_or(0.0));
        features.insert("liquidity".to_string(), token_status.liquidity.unwrap_or(0.0));
        features.insert("holders_count".to_string(), token_status.holders_count.unwrap_or(0) as f64);
        
        // Thêm các tính năng từ RiskAnalysis
        if let Some(analysis) = risk_analysis {
            features.insert("risk_score".to_string(), analysis.risk_score as f64);
            features.insert("critical_issues".to_string(), analysis.critical_issues as f64);
            features.insert("high_issues".to_string(), analysis.high_issues as f64);
        }
        
        // Thu thập dữ liệu mempool một cách an toàn với timeout
        let mempool_data = self.get_mempool_data(token_address).await;
        for (key, value) in mempool_data {
            features.insert(key, value);
        }
        
        // Phân tích dữ liệu với AI model - với timeout để tránh treo
        let prediction = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.model.predict(features)
        ).await {
            Ok(result) => result?,
            Err(_) => {
                warn!("Timeout khi dự đoán AI cho token {}, đang dùng giá trị mặc định", token_address);
                AIPrediction::default() // Sử dụng giá trị mặc định nếu timeout
            }
        };
        
        // Kết quả phân tích
        let result = AIAnalysisResult {
            token_address: token_address.to_string(),
            risk_score: 0, // Assuming risk_score is not available in the prediction
            price_trend: String::new(), // Assuming price_trend is not available in the prediction
            confidence: prediction.confidence,
            recommendation: prediction.suggested_strategy.to_string(),
        };
        
        // Thực hiện hành động nếu đủ tin cậy với một task riêng biệt để không chặn luồng phân tích
        if prediction.confidence >= self.config.auto_trade_threshold 
            && self.config.auto_trade_enabled 
            && self.bot_mode == BotMode::Auto {
            
            let token_address_clone = token_address.to_string();
            let result_clone = result.clone();
            let self_clone = self.clone();
            
            tokio::spawn(async move {
                match self_clone.execute_ai_recommendation(&token_address_clone, &result_clone).await {
                    Ok(_) => {
                        info!("Đã thực hiện quyết định AI cho token {}", token_address_clone);
                    },
                    Err(e) => {
                        warn!("Lỗi khi thực hiện quyết định AI: {}", e);
                    }
                }
            });
        }
        
        // Ghi log trong span để dễ theo dõi
        let span = tracing::info_span!("analyze_token", token = token_address);
        let _guard = span.enter();
        info!("Phân tích token {}: buy_confidence={:.2}, prediction={}", 
             token_address, prediction.confidence, prediction.suggested_strategy);
        
        Ok(result)
    }

    // Helper method để lấy dữ liệu mempool một cách an toàn
    async fn get_mempool_data(&self, token_address: &str) -> HashMap<String, f64> {
        let mut data = HashMap::new();
        
        if let Some(mempool_tracker) = &self.mempool_tracker {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                async {
                    if let Ok(tracker) = mempool_tracker.try_lock() {
                        if let Some(metrics) = tracker.get_token_metrics(token_address) {
                            return Some((
                                metrics.buy_pressure as f64,
                                metrics.sell_pressure as f64,
                                metrics.large_buys_count as f64
                            ));
                        }
                    }
                    None
                }
            ).await {
                Ok(Some((buy, sell, large_buys))) => {
                    data.insert("buy_pressure".to_string(), buy);
                    data.insert("sell_pressure".to_string(), sell);
                    data.insert("large_buys_count".to_string(), large_buys);
                },
                _ => {
                    // Fallback to defaults if unavailable or timeout
                    data.insert("buy_pressure".to_string(), 0.0);
                    data.insert("sell_pressure".to_string(), 0.0);
                    data.insert("large_buys_count".to_string(), 0.0);
                }
            }
        }
        
        data
    }

    async fn execute_ai_recommendation(&self, token_address: &str, result: &AIAnalysisResult) -> Result<(), Box<dyn std::error::Error>> {
        if result.confidence >= self.config.auto_trade_threshold {
            // Sử dụng MonteEquilibrium để tối ưu hóa tham số giao dịch
            if let Some(optimizer) = &self.monte_equilibrium {
                // Tính toán phần trăm vị thế dựa trên tâm lý thị trường và dự đoán
                let position_size_percent = self.calculate_position_size_percent(
                    result.confidence,
                    0.0 // Assuming risk_reward_ratio is not available in the result
                ).await?;
                
                match result.recommendation.as_str() {
                    "pump_potential" if result.confidence > 0.8 => {
                        // Tính số lượng mua dựa trên tỷ lệ risk/reward
                        let base_amount = self.calculate_optimal_position_size(
                            token_address, 
                            0.0 // Assuming risk_reward_ratio is not available in the result
                        ).await?;
                        
                        // Tối ưu hóa tham số với timeout
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            optimizer.optimize_buy_parameters(token_address, &base_amount.to_string())
                        ).await {
                            Ok(Ok(optimize_result)) => {
                                // Chuyển tới TradeManager để thực hiện
                                if let Some(trade_manager) = &self.trade_manager {
                                    trade_manager.buy_token_with_optimized_params(
                                        token_address,
                                        &optimize_result.amount,
                                        optimize_result.gas_price
                                    ).await?;
                                }
                            },
                            Ok(Err(e)) => {
                                warn!("Lỗi khi tối ưu hóa tham số: {}", e);
                                return Err(format!("Lỗi tối ưu hóa: {}", e).into());
                            },
                            Err(_) => {
                                warn!("Timeout khi tối ưu hóa tham số giao dịch");
                                return Err("Timeout khi tối ưu hóa tham số giao dịch".into());
                            }
                        }
                    },
                    "short_term_buy" if result.confidence > 0.7 => {
                        // Implementation for short_term_buy
                        // This is a placeholder and should be implemented
                        return Ok(());
                    },
                    "sandwich_opportunity" if result.confidence > 0.75 => {
                        // Implementation for sandwich_opportunity
                        // This is a placeholder and should be implemented
                        return Ok(());
                    },
                    "arbitrage_opportunity" if result.confidence > 0.8 => {
                        // Implementation for arbitrage_opportunity
                        // This is a placeholder and should be implemented
                        return Ok(());
                    },
                    _ => {}
                }
            }
        }
        
        Ok(())
    }

    // Helper method để tính toán phần trăm vị thế
    async fn calculate_position_size_percent(&self, confidence: f64, risk_reward: f64) -> Result<f64, Box<dyn std::error::Error>> {
        // Tính toán dựa trên cả hai tham số
        let base_percent = confidence * 100.0;
        let risk_adjustment = if risk_reward > 3.0 {
            1.2 // tăng 20% nếu risk/reward rất tốt
        } else if risk_reward > 2.0 {
            1.1 // tăng 10% nếu risk/reward tốt
        } else if risk_reward < 1.0 {
            0.7 // giảm 30% nếu risk/reward không tốt
        } else {
            1.0 // giữ nguyên nếu risk/reward trung bình
        };
        
        // Giới hạn trong khoảng 5%-50% tùy theo cấu hình
        let max_position = self.config.max_position_size_percent;
        let result = (base_percent * risk_adjustment).min(max_position).max(5.0);
        
        Ok(result)
    }

    async fn determine_optimal_strategy(&self, prediction: &AIPrediction, token_address: &str) -> Result<TradingStrategy, Box<dyn std::error::Error>> {
        // Dựa vào kết quả dự đoán để chọn chiến lược tối ưu
        let mempool_activity = self.is_high_mempool_activity(token_address).await?;
        
        match prediction.suggested_strategy.as_str() {
            "pump_potential" if prediction.confidence > 0.8 => {
                Ok(TradingStrategy::Accumulate)
            },
            "short_term_buy" if prediction.confidence > 0.7 => {
                if mempool_activity {
                    Ok(TradingStrategy::MempoolFrontrun)
                } else {
                    Ok(TradingStrategy::LimitedBuy)
                }
            },
            "sandwich_opportunity" if prediction.confidence > 0.75 => {
                // Thêm kiểm tra xem có nạn nhân tiềm năng không
                if self.find_potential_sandwich_victim(token_address).await?.is_some() {
                    Ok(TradingStrategy::SandwichAttack)
                } else {
                    Ok(TradingStrategy::LimitedBuy)
                }
            },
            "arbitrage_opportunity" if prediction.confidence > 0.8 => {
                Ok(TradingStrategy::Arbitrage)
            },
            _ => Ok(TradingStrategy::Monitor),
        }
    }

    async fn is_high_mempool_activity(&self, token_address: &str) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(mempool) = &self.mempool_tracker {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                async {
                    let guard = mempool.try_lock();
                    if let Ok(tracker) = guard {
                        if let Some(metrics) = tracker.get_token_metrics(token_address) {
                            return metrics.buy_pressure > 3 || metrics.pending_tx_count > 5;
                        }
                    }
                    false
                }
            ).await {
                Ok(result) => return Ok(result),
                Err(_) => {
                    warn!("Timeout khi kiểm tra hoạt động mempool cho {}", token_address);
                    return Ok(false);
                }
            }
        }
        
        Ok(false)
    }

    async fn find_potential_sandwich_victim(&self, token_address: &str) -> Result<Option<PendingSwap>, Box<dyn std::error::Error>> {
        if let Some(mempool) = &self.mempool_tracker {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                async {
                    if let Ok(tracker) = mempool.try_lock() {
                        // Tìm các giao dịch mua lớn trong mempool
                        if let Some(swaps) = tracker.pending_swaps.get(token_address) {
                            return swaps.iter()
                                .filter(|swap| swap.is_buy && swap.amount_usd >= self.config.min_sandwich_victim_usd)
                                .max_by(|a, b| a.amount_usd.partial_cmp(&b.amount_usd).unwrap_or(std::cmp::Ordering::Equal))
                                .cloned();
                        }
                    }
                    None
                }
            ).await {
                Ok(result) => return Ok(result),
                Err(_) => {
                    warn!("Timeout khi tìm nạn nhân sandwich tiềm năng cho {}", token_address);
                    return Ok(None);
                }
            }
        }
        
        Ok(None)
    }

    async fn find_potential_frontrun_target(&self, token_address: &str) -> Result<Option<PendingSwap>, Box<dyn std::error::Error>> {
        if let Some(mempool) = &self.mempool_tracker {
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                async {
                    if let Ok(tracker) = mempool.try_lock() {
                        // Tìm các giao dịch mua lớn trong mempool có thể frontrun
                        if let Some(swaps) = tracker.pending_swaps.get(token_address) {
                            return swaps.iter()
                                .filter(|swap| swap.is_buy && swap.amount_usd >= self.config.min_frontrun_target_usd
                                      && swap.gas_price < U256::from(tracker.get_base_fee() * 2))
                                .max_by(|a, b| a.amount_usd.partial_cmp(&b.amount_usd).unwrap_or(std::cmp::Ordering::Equal))
                                .cloned();
                        }
                    }
                    None
                }
            ).await {
                Ok(result) => return Ok(result),
                Err(_) => {
                    warn!("Timeout khi tìm mục tiêu frontrun cho {}", token_address);
                    return Ok(None);
                }
            }
        }
        
        Ok(None)
    }

    #[deprecated(since = "1.1.0", note = "Sử dụng analyze_token thay thế")]
    pub async fn analyze_new_token(
        &mut self, 
        token_address: &str, 
        status: &TokenStatus, 
        risk_analysis: Option<&TokenRiskAnalysis>
    ) -> Result<AIDecision, Box<dyn std::error::Error>> {
        let result = self.analyze_token(token_address, status, risk_analysis).await?;
        
        // Chuyển đổi AIAnalysisResult thành AIDecision để tương thích ngược
        Ok(AIDecision {
            should_buy: result.confidence > 0.7,
            confidence: result.confidence,
            prediction: result.recommendation,
        })
    }
}

// Implement Clone for AIModule to support spawning tasks
impl Clone for AIModule {
    fn clone(&self) -> Self {
        // Create a simple clone for async operations
        Self {
            model: self.model.clone(),
            config: AIModuleConfig {
                auto_trade_enabled: self.config.auto_trade_enabled,
                auto_trade_threshold: self.config.auto_trade_threshold,
                max_position_size_percent: self.config.max_position_size_percent,
                min_sandwich_victim_usd: self.config.min_sandwich_victim_usd,
                min_frontrun_target_usd: self.config.min_frontrun_target_usd,
            },
            mempool_tracker: self.mempool_tracker.clone(),
            monte_equilibrium: self.monte_equilibrium.clone(),
            trade_manager: self.trade_manager.clone(),
            bot_mode: self.bot_mode.clone(),
            trade_history_db: self.trade_history_db.clone(),
        }
    }
}

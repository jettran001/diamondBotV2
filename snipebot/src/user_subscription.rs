use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubscriptionLevel {
    Free,
    Premium,
    VIP,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub level: SubscriptionLevel,
    pub start_date: u64,
    pub end_date: u64,
    pub auto_renew: bool,
}

impl Subscription {
    pub fn new(level: SubscriptionLevel, duration_days: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        Self {
            level,
            start_date: now,
            end_date: now + duration_days * 86400, // Số giây trong một ngày
            auto_renew: false,
        }
    }
    
    pub fn is_active(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        self.end_date >= now
    }
    
    pub fn extend(&mut self, duration_days: u64) {
        self.end_date += duration_days * 86400;
    }
}

// Cấu hình auto trade theo cấp độ người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeUserTradeConfig {
    pub enabled: bool,
    pub max_tokens_to_watch: u32,     // Giới hạn số lượng token theo dõi
    pub allow_yellow_tokens: bool,    // Cho phép giao dịch token vàng
    pub allow_green_tokens: bool,     // Cho phép giao dịch token xanh
    pub auto_sell_minutes: u64,       // Bán tự động sau x phút
    pub stop_loss_percent: f64,       // Ngưỡng stop loss (%)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumUserTradeConfig {
    pub enabled: bool,
    pub max_tokens_to_watch: u32,     // Số lượng token theo dõi (lớn hơn free)
    pub allow_yellow_tokens: bool,    // Cho phép giao dịch token vàng
    pub allow_green_tokens: bool,     // Cho phép giao dịch token xanh
    pub use_gas_optimizer: bool,      // Sử dụng gas tối ưu
    pub use_ai_analysis: bool,        // Sử dụng AI phân tích
    pub ai_confidence_threshold: f64, // Ngưỡng tin cậy AI
    pub auto_sell_minutes: u64,       // Bán tự động sau x phút
    pub take_profit_percent: f64,     // Ngưỡng take profit (%)
    pub stop_loss_percent: f64,       // Ngưỡng stop loss (%)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VIPUserTradeConfig {
    pub enabled: bool,
    pub max_tokens_to_watch: u32,     // Số lượng token theo dõi (không giới hạn)
    pub allow_yellow_tokens: bool,    // Cho phép giao dịch token vàng
    pub allow_green_tokens: bool,     // Cho phép giao dịch token xanh
    pub use_gas_optimizer: bool,      // Sử dụng gas tối ưu
    pub use_ai_analysis: bool,        // Sử dụng AI phân tích
    pub ai_confidence_threshold: f64, // Ngưỡng tin cậy AI
    pub use_mempool_watching: bool,   // Theo dõi mempool
    pub use_front_run: bool,          // Sử dụng front-run
    pub use_sandwich_mode: bool,      // Sử dụng sandwich mode
    pub max_positions: u32,           // Số lượng vị thế tối đa
    pub use_trailing_stop_loss: bool, // Sử dụng trailing stop loss
    pub trailing_stop_percent: f64,   // Ngưỡng trailing stop (%)
    pub take_profit_percent: f64,     // Ngưỡng take profit (%)
    pub stop_loss_percent: f64,       // Ngưỡng stop loss (%)
    pub sandwich_portion_percent: f64, // Phần trăm token dùng cho sandwich
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionTradeConfig {
    pub free_config: FreeUserTradeConfig,
    pub premium_config: PremiumUserTradeConfig,
    pub vip_config: VIPUserTradeConfig,
}

impl Default for SubscriptionTradeConfig {
    fn default() -> Self {
        Self {
            free_config: FreeUserTradeConfig {
                enabled: true,
                max_tokens_to_watch: 5,
                allow_yellow_tokens: true,
                allow_green_tokens: true,
                auto_sell_minutes: 30,
                stop_loss_percent: 15.0,
            },
            premium_config: PremiumUserTradeConfig {
                enabled: true,
                max_tokens_to_watch: 20,
                allow_yellow_tokens: true,
                allow_green_tokens: true,
                use_gas_optimizer: true,
                use_ai_analysis: true,
                ai_confidence_threshold: 0.6,
                auto_sell_minutes: 60,
                take_profit_percent: 25.0,
                stop_loss_percent: 10.0,
            },
            vip_config: VIPUserTradeConfig {
                enabled: true,
                max_tokens_to_watch: 100,
                allow_yellow_tokens: true,
                allow_green_tokens: true,
                use_gas_optimizer: true,
                use_ai_analysis: true,
                ai_confidence_threshold: 0.7,
                use_mempool_watching: true,
                use_front_run: true,
                use_sandwich_mode: true,
                max_positions: 10,
                use_trailing_stop_loss: true,
                trailing_stop_percent: 15.0,
                take_profit_percent: 30.0,
                stop_loss_percent: 7.0,
                sandwich_portion_percent: 30.0,
            },
        }
    }
}

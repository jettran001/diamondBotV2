// External imports
use ethers::prelude::*;
use axum::{
    routing::{post},
    http::StatusCode,
    Json, Router,
    extract::{State},
};

// Standard library imports
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Internal imports
use crate::user_subscription::{SubscriptionLevel, Subscription};
use crate::snipebot::SnipeBot;
use crate::api::AppState;
use crate::user::User;
use crate::types::*;

// Third party imports
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use tracing::{debug, info, warn, error};

// ====== SubscriptionManager Struct (từ snipebot/src/subscription_manager.rs) ======
pub struct SubscriptionManager {
    subscription_config: SubscriptionTradeConfig,
    current_user_level: SubscriptionLevel,
    active_subscriptions: Vec<UserSubscription>,
    // Các thuộc tính khác
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSubscription {
    pub user_id: String,
    pub level: SubscriptionLevel,
    pub start_date: u64,
    pub end_date: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionTradeConfig {
    pub max_simultaneous_trades: usize,
    pub max_daily_trades: usize,
    pub min_trade_interval_seconds: u64,
    pub allowed_chains: Vec<String>,
    pub max_gas_price_gwei: u64,
    pub max_slippage_percent: f64,
}

impl UserSubscription {
    pub fn new(user_id: String, level: SubscriptionLevel, duration_days: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        
        let end_date = now + (duration_days * 24 * 60 * 60);
        
        Self {
            user_id,
            level,
            start_date: now,
            end_date,
        }
    }
    
    pub fn is_active(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
            
        now <= self.end_date
    }
    
    pub fn remaining_days(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
            
        if now > self.end_date {
            0
        } else {
            (self.end_date - now) / (24 * 60 * 60)
        }
    }
    
    pub fn extend(&mut self, additional_days: u64) {
        self.end_date += additional_days * 24 * 60 * 60;
    }
}

impl Default for SubscriptionTradeConfig {
    fn default() -> Self {
        Self {
            max_simultaneous_trades: 1,
            max_daily_trades: 5,
            min_trade_interval_seconds: 3600, // 1 giờ
            allowed_chains: vec!["ethereum".to_string(), "bsc".to_string()],
            max_gas_price_gwei: 100,
            max_slippage_percent: 2.0,
        }
    }
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            subscription_config: SubscriptionTradeConfig::default(),
            current_user_level: SubscriptionLevel::Free,
            active_subscriptions: Vec::new(),
        }
    }
    
    pub fn set_subscription_config(&mut self, config: SubscriptionTradeConfig) {
        self.subscription_config = config;
    }
    
    pub fn update_user_level(&mut self, user_id: &str, level: SubscriptionLevel) -> Result<(), Box<dyn std::error::Error>> {
        // Tìm subscription hiện tại nếu có
        if let Some(subscription) = self.active_subscriptions.iter_mut().find(|s| s.user_id == user_id) {
            subscription.level = level;
            info!("Cập nhật cấp độ cho người dùng {} thành {:?}", user_id, level);
        } else {
            // Tạo subscription mới với thời hạn mặc định 30 ngày
            let new_subscription = UserSubscription::new(user_id.to_string(), level, 30);
            self.active_subscriptions.push(new_subscription);
            info!("Tạo subscription mới cho người dùng {} với cấp độ {:?}", user_id, level);
        }
        
        Ok(())
    }
    
    pub fn get_user_subscription(&self, user_id: &str) -> Option<&UserSubscription> {
        self.active_subscriptions.iter().find(|s| s.user_id == user_id)
    }
    
    pub fn extend_subscription(&mut self, user_id: &str, additional_days: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(subscription) = self.active_subscriptions.iter_mut().find(|s| s.user_id == user_id) {
            subscription.extend(additional_days);
            info!("Gia hạn subscription cho người dùng {} thêm {} ngày", user_id, additional_days);
            Ok(())
        } else {
            Err(format!("Không tìm thấy subscription cho người dùng {}", user_id).into())
        }
    }
    
    pub fn check_subscription_limits(&self, user_id: &str, trade_type: TradeType) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(subscription) = self.get_user_subscription(user_id) {
            if !subscription.is_active() {
                return Err(format!("Subscription của người dùng {} đã hết hạn", user_id).into());
            }
            
            // Kiểm tra các giới hạn dựa trên cấp độ và loại giao dịch
            match subscription.level {
                SubscriptionLevel::Free => {
                    // Giới hạn cho Free tier
                    match trade_type {
                        TradeType::AutoTrade => Err("Tài khoản Free không hỗ trợ Auto Trade".into()),
                        TradeType::ManualTrade => Ok(()),
                        TradeType::SnipeTrade => {
                            // Giới hạn 3 snipe trade mỗi ngày cho tài khoản Free
                            // Logic kiểm tra số lượng snipe trade trong ngày
                            Ok(())
                        }
                    }
                },
                SubscriptionLevel::Premium => {
                    // Giới hạn cho Premium tier
                    Ok(())
                },
                SubscriptionLevel::VIP => {
                    // Không giới hạn cho VIP
                    Ok(())
                }
            }
        } else {
            Err(format!("Không tìm thấy subscription cho người dùng {}", user_id).into())
        }
    }
    
    pub fn get_subscription_config(&self, level: SubscriptionLevel) -> SubscriptionTradeConfig {
        // Tùy chỉnh cấu hình dựa trên cấp độ
        match level {
            SubscriptionLevel::Free => {
                let mut config = self.subscription_config.clone();
                config.max_simultaneous_trades = 1;
                config.max_daily_trades = 3;
                config.allowed_chains = vec!["ethereum".to_string()];
                config
            },
            SubscriptionLevel::Premium => {
                let mut config = self.subscription_config.clone();
                config.max_simultaneous_trades = 3;
                config.max_daily_trades = 10;
                config
            },
            SubscriptionLevel::VIP => self.subscription_config.clone(),
        }
    }
    
    // Phương thức kiểm tra và dọn dẹp các subscription hết hạn
    pub fn cleanup_expired_subscriptions(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
            
        // Tạo danh sách subscription hết hạn
        let expired_subscriptions: Vec<String> = self.active_subscriptions
            .iter()
            .filter(|s| s.end_date < now)
            .map(|s| s.user_id.clone())
            .collect();
            
        for user_id in &expired_subscriptions {
            info!("Subscription của người dùng {} đã hết hạn", user_id);
        }
        
        // Loại bỏ các subscription hết hạn
        self.active_subscriptions.retain(|s| s.end_date >= now);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeType {
    AutoTrade,
    ManualTrade,
    SnipeTrade,
}

// ====== API Models và Routes (từ file subscription.rs gốc) ======

// Request models
#[derive(Deserialize)]
pub struct UpdateSubscriptionRequest {
    pub username: String,
    pub level: String,
    pub duration_days: u64,
}

#[derive(Deserialize)]
pub struct StartAutoTradeRequest {
    pub username: String,
}

// Response models
#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub status: String,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Serialize)]
pub struct SubscriptionData {
    pub level: String,
    pub start_date: u64,
    pub end_date: u64,
    pub is_active: bool,
}

// Hàm tạo routes cho subscription
pub fn subscription_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/subscription/update", post(update_subscription_handler))
        .route("/api/subscription/auto-trade/start", post(start_auto_trade_handler))
}

// Handler cập nhật subscription
async fn update_subscription_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> Result<Json<ApiResponse<SubscriptionData>>, (StatusCode, Json<ApiResponse<()>>)> {
    // Chuyển đổi chuỗi cấp độ thành enum SubscriptionLevel
    let level = match req.level.to_lowercase().as_str() {
        "free" => SubscriptionLevel::Free,
        "premium" => SubscriptionLevel::Premium,
        "vip" => SubscriptionLevel::VIP,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: "Cấp độ đăng ký không hợp lệ".to_string(),
                    data: None,
                }),
            ));
        }
    };

    // Cập nhật subscription
    let mut snipebot = state.snipebot.as_ref().clone();
    match snipebot.update_user_subscription(&req.username, level, req.duration_days).await {
        Ok(_) => {
            // Lấy thông tin subscription đã cập nhật
            let user_manager = state.user_manager.lock().await;
            if let Ok(subscription) = user_manager.get_user_subscription(&req.username) {
                let subscription_data = SubscriptionData {
                    level: format!("{:?}", subscription.level),
                    start_date: subscription.start_date,
                    end_date: subscription.end_date,
                    is_active: subscription.is_active(),
                };

                Ok(Json(ApiResponse {
                    status: "success".to_string(),
                    message: format!("Đã cập nhật cấp độ đăng ký cho người dùng {} thành {}", req.username, req.level),
                    data: Some(subscription_data),
                }))
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse {
                        status: "error".to_string(),
                        message: format!("Không tìm thấy người dùng {}", req.username),
                        data: None,
                    }),
                ))
            }
        },
        Err(e) => {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: format!("Lỗi khi cập nhật cấp độ đăng ký: {}", e),
                    data: None,
                }),
            ))
        }
    }
}

// Handler bắt đầu auto trade
async fn start_auto_trade_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartAutoTradeRequest>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let mut snipebot = state.snipebot.as_ref().clone();
    match snipebot.start_auto_trade(&req.username).await {
        Ok(_) => {
            Ok(Json(ApiResponse {
                status: "success".to_string(),
                message: format!("Đã bắt đầu auto trade cho người dùng {}", req.username),
                data: None,
            }))
        },
        Err(e) => {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: format!("Lỗi khi bắt đầu auto trade: {}", e),
                    data: None,
                }),
            ))
        }
    }
}

// Unit tests
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_user_subscription() {
        let user_id = "test_user".to_string();
        let level = SubscriptionLevel::Premium;
        let duration_days = 30;
        
        let subscription = UserSubscription::new(user_id.clone(), level, duration_days);
        
        assert_eq!(subscription.user_id, user_id);
        assert_eq!(subscription.level, level);
        assert!(subscription.is_active());
        assert!(subscription.remaining_days() <= duration_days);
    }
    
    #[test]
    fn test_subscription_manager() {
        let mut manager = SubscriptionManager::new();
        let user_id = "test_user";
        let level = SubscriptionLevel::Premium;
        
        // Kiểm tra thêm subscription mới
        assert!(manager.update_user_level(user_id, level).is_ok());
        
        // Kiểm tra lấy subscription
        let subscription = manager.get_user_subscription(user_id);
        assert!(subscription.is_some());
        if let Some(sub) = subscription {
            assert_eq!(sub.level, level);
            assert!(sub.is_active());
        }
        
        // Kiểm tra gia hạn subscription
        assert!(manager.extend_subscription(user_id, 30).is_ok());
        
        // Kiểm tra giới hạn giao dịch
        assert!(manager.check_subscription_limits(user_id, TradeType::ManualTrade).is_ok());
    }
}

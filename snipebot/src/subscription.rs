use axum::{
    routing::{post},
    http::StatusCode,
    Json, Router,
    extract::{State},
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::user_subscription::{SubscriptionLevel, Subscription};
use crate::snipebot::SnipeBot;
use crate::api::AppState;

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

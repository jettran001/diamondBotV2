use axum::{
    routing::{get, post},
    http::StatusCode,
    Json, Router,
    extract::{State, Path, Query, Extension},
    response::{IntoResponse, Response},
    middleware::{self, Next},
    http::Request,
    body::Body,
};
use std::sync::Arc;
use std::net::SocketAddr;
use serde::{Serialize, Deserialize};
use super::config::{Config, BotMode};
use super::nodes;
use super::snipebot::SnipeBot;
use super::storage::Storage;
use tracing::{info, warn, error};
use ethers::types::U256;
use super::user::{UserManager, UserRole, User};
use common::middleware::auth::{auth_middleware, create_token, Claims, logout, JWTAuthMiddleware, JWTAuthError};
use common::middleware::rate_limit::ip_rate_limit_middleware;
use common::middleware::UserRole;
use tower_http::cors::{CorsLayer, Any};
use crate::metrics::RETRY_METRICS;
use crate::utils::{RetryConfig, transaction_retry_config};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::types::{
    NetworkStats,
    UserInfo,
    SystemStats,
    WalletBalance,
    TokenBalance,
};
use crate::user_subscription::SubscriptionLevel;
use std::collections::HashMap;
use chrono::Utc;
use crate::utils::{
    safe_now, get_uptime, get_cpu_usage, get_memory_usage,
    get_disk_usage, get_network_stats, get_balance, perform_logout,
};
use crate::types::{
    NetworkStats,
    UserInfo,
    SystemStats,
    WalletBalance,
    TokenBalance,
};
use std::sync::{Mutex, RwLock};
use super::EndpointManager;

/// Cấu trúc phản hồi API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub status: String,
    pub message: String,
    pub data: Option<T>,
    pub timestamp: u64,
}

/// Cấu trúc lỗi API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub code: u32,
    pub status: String,
    pub message: String,
}

pub struct AppState {
    pub config: Config,
    pub storage: Arc<Storage>,
    pub snipebot: Arc<SnipeBot>,
    pub user_manager: Arc<tokio::sync::Mutex<UserManager>>,
    pub endpoint_manager: Arc<RwLock<EndpointManager>>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            status: "success".to_string(),
            message: "Operation successful".to_string(),
            data: Some(data),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            message: message.into(),
            data: None,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        match serde_json::to_value(&self) {
            Ok(json) => Json(json).into_response(),
            Err(err) => {
                let error_response = ApiResponse::<()>::error(format!("JSON serialization error: {}", err));
                Json(error_response).into_response()
            }
        }
    }
}

// Struct cho RetryMetrics
#[derive(Serialize, Deserialize)]
pub struct RetryMetrics {
    pub total_attempts: u64,
    pub successful_attempts: u64,
    pub failed_attempts: u64,
    pub retried_operations: u64,
    pub average_retries: f64,
    pub max_retries_hit: u64,
    pub rpc_errors: std::collections::HashMap<String, u64>,
    pub gas_adjustments: u64,
}

// Triển khai hàm register
async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<String>>, (StatusCode, Json<ApiErrorResponse>)> {
    let mut user_manager = state.user_manager.lock().await;
    
    // Kiểm tra thông tin đăng ký
    if payload.username.is_empty() || payload.password.is_empty() || payload.email.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorResponse {
                status: "error".to_string(),
                message: "Thông tin đăng ký không đầy đủ".to_string(),
            })
        ));
    }
    
    // Kiểm tra định dạng email
    if !payload.email.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorResponse {
                status: "error".to_string(),
                message: "Định dạng email không hợp lệ".to_string(),
            })
        ));
    }
    
    // Kiểm tra độ dài mật khẩu
    if payload.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorResponse {
                status: "error".to_string(),
                message: "Mật khẩu phải có ít nhất 8 ký tự".to_string(),
            })
        ));
    }
    
    // Thử tạo tài khoản
    match user_manager.create_user(&payload.username, &payload.password, &payload.email, UserRole::User) {
        Ok(_) => {
            // Lưu thay đổi
            if let Err(e) = user_manager.save_users().await {
                error!("Lỗi khi lưu thông tin người dùng: {}", e);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiErrorResponse {
                        status: "error".to_string(),
                        message: "Không thể lưu thông tin người dùng".to_string(),
                    })
                ));
            }
            
            Ok(Json(ApiResponse::success("Đăng ký thành công".to_string())))
        },
        Err(e) => {
            Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: e.to_string(),
                })
            ))
        }
    }
}

// Struct cho RegisterRequest
#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
    email: String,
}

// Struct cho LoginResponse
#[derive(Serialize)]
struct LoginResponse {
    token: String,
    refresh_token: String,
    user_info: UserInfo,
    expires_in: u64,
}

// Triển khai hàm update_subscription_handler
async fn update_subscription_handler(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdateSubscriptionRequest>,
) -> Result<Json<ApiResponse<SubscriptionData>>, (StatusCode, Json<ApiErrorResponse>)> {
    let username = claims.sub.clone();
    let mut user_manager = state.user_manager.lock().await;
    
    // Xác định cấp độ đăng ký
    let level = match payload.level.as_str() {
        "premium" => SubscriptionLevel::Premium,
        "vip" => SubscriptionLevel::VIP,
        _ => SubscriptionLevel::Free,
    };
    
    // Tính thời gian kết thúc đăng ký
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    let end_date = if let Some(days) = payload.duration_days {
        current_time + (days as u64 * 86400)
    } else {
        current_time + (30 * 86400) // Mặc định 30 ngày
    };
    
    // Cập nhật đăng ký
    match user_manager.update_user_subscription(&username, level, current_time, end_date) {
        Ok(subscription) => {
            // Lưu thay đổi
            if let Err(e) = user_manager.save_users().await {
                error!("Lỗi khi lưu thông tin người dùng: {}", e);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiErrorResponse {
                        status: "error".to_string(),
                        message: "Không thể lưu thông tin đăng ký".to_string(),
                    })
                ));
            }
            
            // Tạo response
            let subscription_data = SubscriptionData {
                level: format!("{:?}", subscription.level),
                start_date: subscription.start_date,
                end_date: subscription.end_date,
                is_active: subscription.is_active(),
            };
            
            Ok(Json(ApiResponse::success(subscription_data)))
        },
        Err(e) => {
            Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: e.to_string(),
                })
            ))
        }
    }
}

// Struct cho UpdateSubscriptionRequest
#[derive(Deserialize)]
struct UpdateSubscriptionRequest {
    level: String,
    duration_days: Option<u32>,
    payment_id: Option<String>,
}

// Danh sách các routes định nghĩa
fn wallet_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/wallet/balance", get(get_wallet_balance))
        .route("/api/wallet/transactions", get(get_wallet_transactions))
}

fn trading_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/token/:token_address", get(get_token_info))
        .route("/api/token/:token_address/stats", get(token_stats))
        .route("/api/token/approve", post(approve_token))
        .route("/api/token/trade", post(trade_token))
}

fn admin_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/stats", get(get_admin_stats))
        .route("/api/admin/user/:username/logout", post(admin_logout_user))
}

// Định nghĩa router chính
pub fn get_routes(app_state: Arc<AppState>) -> Router {
    // Public routes không cần xác thực
    let public_routes = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/auth/login", post(login))
        .route("/api/auth/register", post(register));
    
    // Routes cần xác thực 
    let auth_routes = Router::new()
        .route("/api/auth/me", get(get_current_user))
        .route("/api/auth/logout", post(logout_user))
        .merge(wallet_routes())
        .merge(trading_routes())
        .merge(admin_routes())
        .route("/api/subscription/status", get(get_subscription_status))
        .route("/api/subscription/update", post(update_subscription_handler))
        .route("/api/bot/mode", get(get_bot_mode))
        .route("/api/metrics/retry", get(get_retry_metrics));
    
    // Merge và áp dụng middleware
    Router::new()
        .merge(public_routes)
        .merge(auth_routes.layer(middleware::from_fn_with_state(app_state.clone(), auth_middleware)))
        .with_state(app_state)
}

// Thêm phương thức để tạo router từ AppState
pub async fn create_api_server(app_state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let app = get_routes(app_state);
    
    // Thêm CORS
    let cors = CorsLayer::new()
        .allow_headers(Any)
        .allow_methods(Any)
        .allow_origin(Any);
        
    let app = app.layer(cors);
    
    // Lấy port từ config
    let port = app_state.config.api_port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    
    info!("API server starting on {}", addr);
    
    // Cấu hình JWT middleware và rate limiter
    let jwt_secret = app_state.config.auth.jwt_secret.clone();
    let jwt_middleware = JWTAuthMiddleware::new(
        jwt_secret,
        |jti| {
            let blacklist = diamond_common::middleware::auth::TOKEN_BLACKLIST.read().unwrap();
            blacklist.is_blacklisted(jti)
        },
        |key, limit| async {
            diamond_common::middleware::rate_limit::check_rate_limit(key, limit, 60).await
        }.into(),
        |err| match err {
            JWTAuthError::InvalidToken(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Token không hợp lệ: {}", msg),
                })
            ),
            JWTAuthError::BlacklistedToken => (
                StatusCode::UNAUTHORIZED,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: "Token đã bị vô hiệu hóa".to_string(),
                })
            ),
            JWTAuthError::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: "Quá nhiều yêu cầu trong thời gian ngắn. Vui lòng thử lại sau.".to_string(),
                })
            ),
        },
        app_state.clone(),
    );
    
    // Cập nhật router với middleware mới
    let app = Router::new()
        .merge(Router::new()
            .route("/api/health", get(health_check))
            .route("/api/auth/login", post(login))
            .route("/api/auth/register", post(register)))
        .merge(
            auth_routes
                .merge(admin_routes())
                .merge(wallet_routes())
                // Thêm các middleware xác thực
                .layer(from_fn_with_state(app_state.clone(), jwt_middleware))
        )
        // Middleware chung cho tất cả routes
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO))
        )
        .with_state(app_state)
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            |state, req, next| async move {
                ip_rate_limit_middleware(
                    req,
                    next,
                    100, // limit
                    60,  // window_size in seconds
                    || (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(ApiErrorResponse {
                            status: "error".to_string(),
                            message: "Quá nhiều yêu cầu. Vui lòng thử lại sau.".to_string(),
                        })
                    ),
                ).await
            },
        ));
    
    // Khởi động server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}

// Thêm struct GasInfo
#[derive(Debug, Serialize)]
pub struct GasInfo {
    pub chain_id: u64,
    pub chain_name: String,
    pub current_price_gwei: f64,
    pub average_price_gwei: f64,
    pub congestion_level: String,
    pub eip1559_supported: bool,
    pub priority_fee_gwei: Option<f64>,
    pub max_fee_gwei: Option<f64>,
    pub optimal_priority_fee_gwei: Option<f64>,
    pub optimal_max_fee_gwei: Option<f64>,
    pub trend: Option<String>,
    pub timestamp: u64,
}

// Hàm xử lý API endpoint
async fn get_gas_info(
    State(state): State<Arc<AppState>>,
    Path(chain_id): Path<u64>,
) -> Result<Json<ApiResponse<GasInfo>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy chain adapter
    let chain_adapter = match state.snipebot.get_chain_adapter_by_id(chain_id) {
        Some(adapter) => adapter,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Chain ID {} không được hỗ trợ", chain_id),
                })
            ));
        }
    };
    
    // Lấy thông tin gas
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
        
    let current_gas_price = match chain_adapter.get_provider().get_gas_price().await {
        Ok(price) => price,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Không thể lấy gas price: {}", e),
                })
            ));
        }
    };
    
    // Chuyển wei thành gwei
    let current_price_gwei = current_gas_price.as_u128() as f64 / 1_000_000_000.0;
    
    // Lấy thông tin optimizer
    let (average_price_gwei, congestion_level, trend, priority_fee, max_fee) = 
        if let Some(optimizer) = chain_adapter.get_gas_optimizer() {
            let avg = optimizer.get_average_gas_price()
                .map(|p| p.as_u128() as f64 / 1_000_000_000.0)
                .unwrap_or(current_price_gwei);
                
            let congestion = optimizer.get_network_congestion().to_string();
            
            let trend = optimizer.analyze_gas_trend()
                .map(|(_, desc)| desc);
                
            // Cố gắng lấy fees EIP-1559 nếu được hỗ trợ
            let (priority_fee, max_fee) = if chain_adapter.get_config().eip1559_supported {
                match optimizer.get_optimal_eip1559_fees(chain_adapter).await {
                    Ok((priority, max)) => (
                        Some(priority.as_u128() as f64 / 1_000_000_000.0),
                        Some(max.as_u128() as f64 / 1_000_000_000.0)
                    ),
                    Err(_) => (None, None)
                }
            } else {
                (None, None)
            };
            
            (avg, congestion, trend, priority_fee, max_fee)
        } else {
            (current_price_gwei, "Unknown".to_string(), None, None, None)
        };
    
    // Tạo response
    let gas_info = GasInfo {
        chain_id,
        chain_name: chain_adapter.get_config().name.clone(),
        current_price_gwei,
        average_price_gwei,
        congestion_level,
        eip1559_supported: chain_adapter.get_config().eip1559_supported,
        priority_fee_gwei: priority_fee,
        max_fee_gwei: max_fee,
        optimal_priority_fee_gwei: priority_fee,
        optimal_max_fee_gwei: max_fee,
        trend,
        timestamp: current_time,
    };
    
    Ok(Json(ApiResponse {
        status: "success".to_string(),
        data: gas_info,
    }))
}

// Thêm route vào router
pub fn register_routes(router: &mut Router) {
    // ... các routes hiện tại ...
    
    // Thêm route gas info
    router.route("/api/gas/:chain_id", get(get_gas_info));
}

// Middleware kiểm tra quyền admin
async fn admin_auth<B>(
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    role_middleware(
        request, 
        next, 
        vec!["admin", "system"],
        || (
            StatusCode::FORBIDDEN,
            Json(ApiErrorResponse {
                status: "error".to_string(),
                message: "Bạn không có quyền thực hiện thao tác này".to_string(),
            })
        )
    ).await
}

// Health check endpoint - triển khai
async fn health_check() -> impl IntoResponse {
    Json(ApiResponse::success(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "server_time": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })))
}

// Triển khai get_admin_stats
async fn get_admin_stats(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<ApiResponse<AdminStats>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Kiểm tra quyền admin
    if claims.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiErrorResponse {
                status: "error".to_string(), 
                message: "Bạn không có quyền truy cập".to_string(),
            }),
        ));
    }
    
    // Lấy thông tin server
    let system_stats = get_system_stats().await;
    
    // Lấy thông tin người dùng
    let user_manager = state.user_manager.lock().await;
    let users = user_manager.get_all_users();
    let active_users = users.iter().filter(|u| u.active).count();
    let premium_users = users.iter().filter(|u| u.role == UserRole::Admin).count();
    
    // Lấy thông tin giao dịch
    let transactions = state.storage.get_all_transactions();
    let transactions_count = transactions.len();
    
    // Lấy thông tin của 24h qua
    let day_ago = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() - 86400;
    
    let daily_transactions = transactions.iter()
        .filter(|tx| tx.timestamp >= day_ago)
        .count();
    
    // Tạo response
    let admin_stats = AdminStats {
        total_users: users.len(),
        active_users,
        premium_users,
        total_transactions: transactions_count,
        daily_transactions,
        total_volume_usd: 0.0, // Tính sau
        daily_volume_usd: 0.0, // Tính sau
        system_stats,
    };
    
    Ok(Json(ApiResponse::success(admin_stats)))
}

// Struct định nghĩa cho admin stats
#[derive(Debug, Serialize)]
struct AdminStats {
    pub total_users: usize,
    pub active_users: usize,
    pub premium_users: usize,
    pub total_transactions: usize,
    pub daily_transactions: usize,
    pub total_volume_usd: f64,
    pub daily_volume_usd: f64,
    pub system_stats: SystemStats,
}

// Struct cho SystemStats
#[derive(Debug, Serialize)]
struct SystemStats {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub uptime_seconds: u64,
    pub node_connections: std::collections::HashMap<String, bool>,
}

// Hàm lấy thông tin hệ thống
async fn get_system_stats() -> Result<Json<ApiResponse<SystemStatsV2>>, ApiErrorResponse> {
    let stats = SystemStatsV2 {
        uptime: get_uptime(),
        cpu_usage: get_cpu_usage(),
        memory_usage: get_memory_usage(),
        disk_usage: get_disk_usage(),
        network_stats: get_network_stats(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Ok(Json(ApiResponse {
        status: "success".to_string(),
        message: "System stats retrieved successfully".to_string(),
        data: Some(stats),
        timestamp: utils::safe_now(),
    }))
}

async fn get_wallet_balance(claims: Claims) -> Result<Json<ApiResponse<WalletBalanceV2>>, ApiErrorResponse> {
    let balance_response = WalletBalanceV2 {
        address: claims.wallet_ids.first().cloned().unwrap_or_default(),
        balance: get_balance().await?,
        native_symbol: "ETH".to_string(),
    };

    Ok(Json(ApiResponse {
        status: "success".to_string(),
        message: "Wallet balance retrieved successfully".to_string(),
        data: Some(balance_response),
        timestamp: utils::safe_now(),
    }))
}

async fn logout_user(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Ghi log cho việc logout
    info!("Logout request with token: {}", token);
    
    // Gọi hàm logout từ middleware để invalid token
    common::middleware::auth::logout(token, &state.token_blacklist).await;
    
    Json(ApiResponse {
        status: "success".to_string(),
        message: "Đã đăng xuất thành công".to_string(),
        data: json!({}),
    })
}

// Triển khai admin_logout_user
async fn admin_logout_user(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(username): Path<String>,
) -> Result<Json<ApiResponse<String>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Kiểm tra quyền admin
    if claims.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiErrorResponse {
                status: "error".to_string(), 
                message: "Bạn không có quyền truy cập".to_string(),
            }),
        ));
    }
    
    // TODO: Triển khai logic đăng xuất người dùng
    // Ví dụ: invalidate tất cả token của người dùng này
    
    Ok(Json(ApiResponse::success(format!("Đã đăng xuất người dùng {}", username))))
}

// Triển khai get_wallet_transactions
async fn get_wallet_transactions(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ApiResponse<Vec<TransactionInfo>>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy tham số phân trang
    let page = params.get("page").and_then(|p| p.parse::<usize>().ok()).unwrap_or(1);
    let limit = params.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(10);
    
    // Lấy địa chỉ ví từ thông tin người dùng
    let wallet_address = claims.wallets.first().cloned().unwrap_or_default();
    
    // Lấy giao dịch từ storage
    let all_transactions = state.storage.get_all_transactions();
    
    // Lọc giao dịch của ví hiện tại
    let mut transactions: Vec<TransactionInfo> = all_transactions.iter()
        .filter(|tx| tx.from_address == wallet_address || tx.to_address == wallet_address)
        .map(|tx| TransactionInfo {
            hash: tx.transaction_hash.clone(),
            block_number: tx.block_number,
            timestamp: tx.timestamp,
            from_address: tx.from_address.clone(),
            to_address: tx.to_address.clone(),
            value: tx.value.clone(),
            gas_used: tx.gas_used,
            gas_price: tx.gas_price.clone(),
            status: tx.success.then(|| "Success".to_string()).unwrap_or("Failed".to_string()),
        })
        .collect();
    
    // Sắp xếp theo thời gian giảm dần
    transactions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    
    // Phân trang
    let start = (page - 1) * limit;
    let end = start + limit;
    let paged_transactions = transactions.into_iter()
        .skip(start)
        .take(limit)
        .collect();
    
    Ok(Json(ApiResponse::success(paged_transactions)))
}

// Struct cho TransactionInfo
#[derive(Debug, Serialize)]
struct TransactionInfo {
    pub hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub from_address: String,
    pub to_address: String,
    pub value: String,
    pub gas_used: u64,
    pub gas_price: String,
    pub status: String,
}

// Triển khai get_token_info
async fn get_token_info(
    State(state): State<Arc<AppState>>,
    Path(token_address): Path<String>,
) -> Result<Json<ApiResponse<TokenDetails>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy token info
    match state.snipebot.get_token_info(&token_address).await {
        Ok(token_info) => {
            // Lấy thông tin bổ sung
            let token_status = match state.snipebot.get_token_status(&token_address).await {
                Ok(status) => Some(status),
                Err(_) => None,
            };
            
            // Tạo response
            let token_details = TokenDetails {
                address: token_info.address,
                name: token_info.name.unwrap_or_else(|| "Unknown".to_string()),
                symbol: token_info.symbol,
                decimals: token_info.decimals,
                total_supply: "Unknown".to_string(), // TODO: Lấy total supply
                liquidity: token_status.as_ref().and_then(|s| s.liquidity),
                price_usd: token_status.as_ref().and_then(|s| s.price_usd),
                market_cap: token_status.as_ref().and_then(|s| s.market_cap),
                holders: token_status.as_ref().and_then(|s| s.holder_count),
                creation_time: token_status.as_ref().and_then(|s| s.creation_block.map(|b| b as i64)),
                pair: token_info.pair,
                router: Some(token_info.router),
            };
            
            Ok(Json(ApiSuccessResponse {
                status: "success".to_string(),
                data: serde_json::to_value(token_details).unwrap(),
            }))
        },
        Err(e) => {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Lỗi khi lấy thông tin token: {}", e),
                })
            ))
        }
    }
}

// Struct cho TokenDetails
#[derive(Debug, Serialize)]
struct TokenDetails {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: String,
    pub liquidity: Option<f64>,
    pub price_usd: Option<f64>,
    pub market_cap: Option<f64>,
    pub holders: Option<u64>,
    pub creation_time: Option<i64>,
    pub pair: Option<String>,
    pub router: Option<String>,
}

// Triển khai get_bot_mode
async fn get_bot_mode(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<BotModeInfo>>, (StatusCode, Json<ApiErrorResponse>)> {
    let mode = state.snipebot.get_mode();
    
    let bot_mode_info = BotModeInfo {
        mode: format!("{:?}", mode),
        auto_enabled: matches!(mode, BotMode::Auto),
        last_updated: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    
    Ok(Json(ApiSuccessResponse {
        status: "success".to_string(),
        data: serde_json::to_value(bot_mode_info).unwrap(),
    }))
}

// Struct cho BotModeInfo
#[derive(Debug, Serialize)]
struct BotModeInfo {
    pub mode: String,
    pub auto_enabled: bool,
    pub last_updated: u64,
}

// Triển khai approve_token
async fn approve_token(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<ApproveTokenRequestV2>,
) -> Result<Json<ApiResponse<String>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy địa chỉ token
    let token_address = payload.token_address;
    
    // Lấy địa chỉ spender (router)
    let spender_address = payload.spender_address.clone();
    
    // Parse số lượng token
    let amount = match U256::from_dec_str(&payload.amount) {
        Ok(amt) => amt,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Số lượng token không hợp lệ: {}", e),
                })
            ));
        }
    };
    
    // Thực hiện approve
    match state.snipebot.approve_token(&token_address, &spender_address, amount).await {
        Ok(tx_hash) => {
            Ok(Json(ApiResponse::success(tx_hash)))
        },
        Err(e) => {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Lỗi khi approve token: {}", e),
                })
            ))
        }
    }
}

// Struct cho ApproveTokenRequest
#[derive(Debug, Deserialize)]
pub struct ApproveTokenRequestV2 {
    pub token_address: String,
    pub spender_address: String,
    pub amount: String,
    pub wallet_address: Option<String>,
}

// Triển khai trade_token
async fn trade_token(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<TradeTokenRequestV2>,
) -> Result<Json<ApiResponse<String>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Parse thông tin giao dịch
    let token_address = payload.token_address;
    let amount = match U256::from_dec_str(&payload.amount) {
        Ok(amt) => amt,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: format!("Số lượng token không hợp lệ: {}", e),
                })
            ));
        }
    };
    
    let slippage = payload.slippage.unwrap_or(1.0); // 1% mặc định
    
    // Xác định loại giao dịch
    match payload.action.as_str() {
        "buy" => {
            // Mua token
            match state.snipebot.swap_eth_for_tokens(amount, &token_address, slippage).await {
                Ok(tx_hash) => {
                    Ok(Json(ApiResponse::success(tx_hash)))
                },
                Err(e) => {
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiErrorResponse {
                            status: "error".to_string(),
                            message: format!("Lỗi khi mua token: {}", e),
                        })
                    ))
                }
            }
        },
        "sell" => {
            // Bán token
            match state.snipebot.swap_tokens_for_eth(&token_address, amount, slippage).await {
                Ok(tx_hash) => {
                    Ok(Json(ApiResponse::success(tx_hash)))
                },
                Err(e) => {
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiErrorResponse {
                            status: "error".to_string(),
                            message: format!("Lỗi khi bán token: {}", e),
                        })
                    ))
                }
            }
        },
        _ => {
            Err((
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: "Loại giao dịch không hợp lệ. Chỉ hỗ trợ 'buy' hoặc 'sell'".to_string(),
                })
            ))
        }
    }
}

// Struct cho TradeTokenRequest
#[derive(Debug, Deserialize)]
pub struct TradeTokenRequestV2 {
    pub token_address: String,
    pub amount: String,
    pub action: String,
    pub slippage: Option<f64>,
    pub wallet_address: Option<String>,
}

/// Struct cho params truy vấn giao dịch
#[derive(Debug, Deserialize)]
struct TransactionParams {
    limit: Option<usize>,
    offset: Option<usize>,
    sort: Option<String>,
}

/// Struct cho tham số thông tin token
#[derive(Debug, Deserialize)]
struct TokenInfoParams {
    token_address: Option<String>,
    chain_id: Option<u64>,
}

/// Struct cho request phê duyệt token
#[derive(Debug, Deserialize)]
struct ApproveTokenRequest {
    token_address: String,
    spender: Option<String>,
    amount: Option<String>,
}

/// Struct cho request giao dịch token
#[derive(Debug, Deserialize)]
struct TradeTokenRequest {
    token_address: String,
    amount_in: String,
    action: String, // "buy" hoặc "sell"
    slippage: Option<f64>,
    deadline: Option<u64>,
}

/// Struct cho request đăng xuất từ admin
#[derive(Debug, Deserialize)]
struct AdminLogoutRequest {
    username: String,
}

/// Struct cho response thống kê admin
#[derive(Debug, Serialize)]
struct AdminStatsResponse {
    active_users: usize,
    total_trades: usize,
    successful_trades: usize,
    failed_trades: usize,
    system_stats: SystemStats,
}

/// Struct cho thông kê hệ thống
#[derive(Debug, Serialize)]
struct SystemStatsDuplicate {
    cpu_usage: f64,
    memory_usage: f64,
    disk_usage: f64,
    uptime: u64,
}

/// Thông tin số dư ví
#[derive(Debug, Serialize)]
struct WalletBalanceDuplicate {
    address: String,
    native_balance: String,
    native_token: String,
    token_balances: Vec<TokenBalanceDuplicate>,
    total_usd_value: String,
}

/// Thông tin số dư token
#[derive(Debug, Serialize)]
struct TokenBalanceDuplicate {
    token_address: String,
    symbol: String,
    name: String,
    balance: String,
    decimals: u8,
    usd_value: Option<String>,
}

/// Response cho thông tin giao dịch
#[derive(Debug, Serialize)]
struct TransactionResponse {
    tx_hash: String,
    from: String,
    to: String,
    value: String,
    gas_used: String,
    status: String,
    timestamp: u64,
    method: String,
}

/// Response cho thông tin token
#[derive(Debug, Serialize)]
struct TokenInfoResponse {
    address: String,
    name: String,
    symbol: String,
    decimals: u8,
    total_supply: String,
    price_usd: Option<String>,
    market_cap: Option<String>,
    liquidity: Option<String>,
    holders: Option<usize>,
    transactions: Option<usize>,
}

// Struct cho SystemStats
#[derive(Debug, Serialize)]
pub struct SystemStatsV2 {
    pub uptime: u64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_stats: NetworkStats,
    pub version: String,
}

// Struct cho WalletBalance
#[derive(Debug, Serialize)]
pub struct WalletBalanceV2 {
    pub address: String,
    pub chain_id: u64,
    pub native_balance: String,
    pub native_balance_usd: f64,
    pub tokens: Vec<TokenBalanceV2>,
}

// Struct cho TokenBalance
#[derive(Debug, Serialize)]
pub struct TokenBalanceV2 {
    pub token_address: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub balance: String,
    pub balance_usd: Option<f64>,
}

async fn system_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<SystemStatsV2>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy thời gian khởi động
    let uptime = match std::fs::read_to_string("/proc/uptime") {
        Ok(uptime) => {
            let uptime_parts: Vec<&str> = uptime.split_whitespace().collect();
            if uptime_parts.len() > 0 {
                let uptime_seconds = uptime_parts[0].parse::<f64>().unwrap_or(0.0);
                uptime_seconds as u64
            } else {
                0
            }
        },
        Err(_) => 0,
    };
    
    // Lấy thông tin hệ thống
    let cpu_usage = 0.5; // placeholder
    let memory_usage = 0.35; // placeholder
    let disk_usage = 0.25; // placeholder
    let active_requests = 10; // placeholder
    let success_rate = 0.95; // placeholder
    
    Ok(Json(ApiResponse {
        success: true,
        data: SystemStatsV2 {
            uptime,
            cpu_usage,
            memory_usage,
            disk_usage,
            active_requests,
            success_rate,
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        message: Some("System stats retrieved successfully".to_string()),
    }))
}

async fn wallet_balance(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<WalletBalanceV2>>, (StatusCode, Json<ApiErrorResponse>)> {
    // Lấy địa chỉ ví từ params hoặc từ claims
    let wallet_addr = if let Some(addr) = params.get("address") {
        addr.clone()
    } else {
        claims.wallet_address.clone()
    };
    
    // Kiểm tra địa chỉ ví hợp lệ
    if !wallet_addr.starts_with("0x") || wallet_addr.len() != 42 {
        return Err((
            StatusCode::BAD_REQUEST, 
            Json(ApiErrorResponse { 
                error: "Invalid wallet address".to_string(),
                code: 400,
            })
        ));
    }
    
    // Lấy số dư ETH
    let eth_balance = state.snipebot.get_native_balance().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorResponse {
                error: format!("Failed to get ETH balance: {}", e),
                code: 500,
            })
        )
    })?;
    
    // Format số dư ETH
    let eth_balance_formatted = format!("{}", ethers::utils::format_ether(eth_balance));
    
    // Quy đổi sang USD
    let eth_balance_usd = 1500.0 * ethers::utils::format_ether(eth_balance).parse::<f64>().unwrap_or(0.0);
    
    // Lấy danh sách token và số dư
    let tokens = vec![
        TokenBalanceV2 {
            token_address: "0x...".to_string(),
            token_symbol: "USDT".to_string(),
            token_decimals: 6,
            balance: "1000.0".to_string(),
            balance_usd: Some(1000.0),
        },
        TokenBalanceV2 {
            token_address: "0x...".to_string(),
            token_symbol: "USDC".to_string(),
            token_decimals: 6,
            balance: "500.0".to_string(),
            balance_usd: Some(500.0),
        },
    ];
    
    // Tạo response
    let token_balances: Vec<TokenBalanceV2> = tokens.into_iter()
        .collect();
    
    let balance_response = WalletBalanceV2 {
        address: wallet_addr.clone(),
        chain_id: state.config.chain_id,
        native_balance: eth_balance_formatted,
        native_balance_usd: eth_balance_usd,
        tokens: token_balances,
    };
    
    Ok(Json(ApiResponse {
        success: true,
        data: balance_response,
        message: Some("Wallet balance retrieved successfully".to_string()),
    }))
}

// Thêm hàm xử lý logout cho API
async fn logout(
    req: Request<Body>,
) -> Result<Json<ApiResponse<String>>, (StatusCode, Json<ApiErrorResponse>)> {
    let auth_header = req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    
    match crate::middleware::logout(auth_header).await {
        Ok(_) => Ok(Json(ApiResponse {
            status: "success".to_string(),
            data: "Đăng xuất thành công".to_string(),
        })),
        Err((status, _)) => Err((
            status,
            Json(ApiErrorResponse {
                status: "error".to_string(),
                message: "Không thể đăng xuất".to_string(),
            })
        ))
    }
}
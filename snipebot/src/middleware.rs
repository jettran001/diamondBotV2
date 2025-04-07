use axum::{
    http::{Request, StatusCode, header},
    middleware::Next,
    response::{Response, IntoResponse},
    extract::{State, TypedHeader, headers, Extension, Path},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use super::user::{UserManager, UserRole};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use crate::api::ApiErrorResponse;
use crate::config::Config;
use crate::storage::Storage;
use log::{info, error, warn, debug};
use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;
use chrono::{Duration, Utc};
use uuid::Uuid;
use serde_json;
use tracing::{info, error};
use crate::types::{Claims, UserInfo};

// Chuyển các định nghĩa sang diamond_common và re-export từ đó
pub use diamond_common::middleware::auth::*;
pub use diamond_common::middleware::rate_limit::*;

// Cảnh báo lỗi không phải dùng file này nữa
#[deprecated(
    since = "0.1.0",
    note = "Các phần này đã được di chuyển sang diamond_common::middleware, hãy import từ đó thay vì file này"
)]
pub struct DeprecatedMiddleware;

// Cache cho rate limit
pub static RATE_LIMIT_CACHE: Lazy<RwLock<HashMap<String, Vec<u64>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Cache cho token blacklist (đã đăng xuất hoặc hết hạn)
pub static TOKEN_BLACKLIST: Lazy<RwLock<HashMap<String, u64>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Cấu hình cho JWT
#[derive(Debug, Clone)]
pub struct JWTConfig {
    pub secret_key: String,
    pub expiration_hours: u64,
    pub refresh_expiration_days: u64,
}

impl Default for JWTConfig {
    fn default() -> Self {
        Self {
            secret_key: "superchain_secret_key".to_string(), // Nên lấy từ biến môi trường
            expiration_hours: 24,
            refresh_expiration_days: 30,
        }
    }
}

// Thông tin trong JWT token
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,      // User ID hoặc username
    pub role: String,     // Quyền: "user", "admin", etc.
    pub wallet_ids: Vec<String>, // Danh sách các wallet ID được phép truy cập
    pub exp: usize,       // Expiration time (timestamp)
    pub nbf: usize,       // Not before time (timestamp)
    pub iat: usize,       // Issued at time (timestamp)
    pub jti: String,      // JWT ID - dùng để quản lý blacklist
}

// Response khi đăng nhập thành công
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub user: UserInfo,
    pub expires_in: u64,
}

// Thông tin người dùng trả về - sửa để phù hợp với UserInfo từ common/middleware
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub username: String,
    pub email: String,
    pub role: String,
    pub wallets: Vec<String>,
    pub last_login: Option<u64>,
}

// Request refresh token
#[derive(Debug, Deserialize)]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

// Enum các loại role
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum UserRole {
    User,
    Admin,
    System,
}

impl From<&str> for UserRole {
    fn from(role: &str) -> Self {
        match role.to_lowercase().as_str() {
            "admin" => UserRole::Admin,
            "system" => UserRole::System,
            _ => UserRole::User,
        }
    }
}

// Cấu trúc dữ liệu cho rate limiting
pub struct RateLimitData {
    count: usize,
    window_start: u64,
}

// Quản lý blacklist token
pub struct TokenBlacklist {
    blacklisted_tokens: HashMap<String, u64>, // jti -> expiration time
    last_cleanup: u64,
}

impl TokenBlacklist {
    pub fn new() -> Self {
        let current_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!("Lỗi khi lấy thời gian hệ thống: {}", err);
                // Fallback
                1609459200 // 2021-01-01 00:00:00 UTC
            }
        };
        
        Self {
            blacklisted_tokens: HashMap::new(),
            last_cleanup: current_time,
        }
    }
    
    // Thêm token vào blacklist
    pub fn add_token(&mut self, jti: String, exp: u64) {
        self.blacklisted_tokens.insert(jti, exp);
        
        // Dọn dẹp blacklist mỗi giờ
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!("Lỗi khi lấy thời gian hệ thống: {}", err);
                // Fallback
                self.last_cleanup + 3600 // Cộng thêm 1 giờ so với lần dọn dẹp cuối cùng
            }
        };
            
        if now - self.last_cleanup > 3600 {
            self.cleanup();
            self.last_cleanup = now;
        }
    }
    
    // Kiểm tra token có trong blacklist không
    pub fn is_blacklisted(&self, jti: &str) -> bool {
        self.blacklisted_tokens.contains_key(jti)
    }
    
    // Dọn dẹp token hết hạn
    fn cleanup(&mut self) {
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(err) => {
                warn!("Lỗi khi lấy thời gian hệ thống: {}", err);
                // Fallback nếu lỗi, chỉ cần giữ tất cả token
                return;
            }
        };
            
        self.blacklisted_tokens.retain(|_, exp| *exp > now);
    }
}

// Lazy static cho JWT_SECRET và rate limiter
lazy_static::lazy_static! {
    static ref JWT_SECRET: String = std::env::var("JWT_SECRET").unwrap_or_else(|_| "super_secret_key_for_development_only".to_string());
    static ref RATE_LIMIT_WINDOWS: Arc<RwLock<HashMap<String, RateLimitData>>> = Arc::new(RwLock::new(HashMap::new()));
    static ref TOKEN_BLACKLIST: Arc<RwLock<TokenBlacklist>> = Arc::new(RwLock::new(TokenBlacklist::new()));
}

// Tạo JWT token
pub fn create_jwt_token(
    user_id: &str,
    role: UserRole,
    wallet_ids: Vec<String>,
    duration_hours: usize,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let expires_at = now + Duration::hours(duration_hours as i64);
    
    let claims = Claims {
        sub: user_id.to_string(),
        role: match role {
            UserRole::Admin => "admin".to_string(),
            UserRole::System => "system".to_string(),
            UserRole::User => "user".to_string(),
        },
        wallet_ids,
        exp: expires_at.timestamp() as usize,
        nbf: now.timestamp() as usize,
        iat: now.timestamp() as usize,
        jti: Uuid::new_v4().to_string(),
    };
    
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
}

// Giải mã JWT token
pub fn decode_jwt_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
        &Validation::default(),
    )?;
    
    Ok(token_data.claims)
}

/// Trích xuất token từ chuỗi xác thực 
fn extract_token(auth_header: &str) -> Option<String> {
    if auth_header.starts_with("Bearer ") {
        let token = auth_header.trim_start_matches("Bearer ").trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    None
}

// Middleware kiểm tra JWT
pub async fn auth_middleware<B>(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<headers::Authorization<headers::Bearer>>,
    mut request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    if request.uri().path().starts_with("/api/public") {
        return Ok(next.run(request).await);
    }

    if let Some(auth_header) = request.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = extract_token(auth_str) {
                match decode_jwt_token(&token) {
                    Ok(claims) => {
                        // Kiểm tra nếu token có trong danh sách đen
                        let blacklist = TOKEN_BLACKLIST.read().unwrap();
                        if blacklist.contains_key(&claims.jti) {
                            return Err((
                                StatusCode::UNAUTHORIZED,
                                Json(ApiErrorResponse {
                                    status: "error".to_string(),
                                    message: "Token đã bị vô hiệu hóa".to_string(),
                                })
                            ));
                        }
                        
                        // Kiểm tra rate limit
                        let ip = extract_client_ip(&request).unwrap_or_else(|| "unknown".to_string());
                        let rate_limit_key = format!("{}:{}", ip, claims.sub);
                        
                        if !check_rate_limit(&rate_limit_key, 100).await {
                            return Err((
                                StatusCode::TOO_MANY_REQUESTS,
                                Json(ApiErrorResponse {
                                    status: "error".to_string(),
                                    message: "Quá nhiều yêu cầu trong thời gian ngắn. Vui lòng thử lại sau.".to_string(),
                                })
                            ));
                        }
                        
                        // Gắn thông tin user vào request extensions
                        request.extensions_mut().insert(claims);
                        
                        // Tiếp tục xử lý request
                        Ok(next.run(request).await)
                    }
                    Err(_) => return Err(StatusCode::UNAUTHORIZED),
                }
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

// Middleware kiểm tra quyền admin
pub async fn admin_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    // Lấy thông tin claims từ request extension
    if let Some(claims) = request.extensions().get::<Claims>() {
        if claims.role == "admin" || claims.role == "system" {
            // Người dùng có quyền admin, tiếp tục xử lý
            return Ok(next.run(request).await);
        }
    }
    
    // Không có quyền admin
    Err((
        StatusCode::FORBIDDEN,
        Json(ApiErrorResponse {
            status: "error".to_string(),
            message: "Bạn không có quyền thực hiện thao tác này".to_string(),
        })
    ))
}

// Middleware kiểm tra quyền truy cập wallet
pub async fn wallet_access_middleware<B>(
    State(state): State<Arc<Mutex<UserManager>>>,
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    // Lấy thông tin claims từ request extension
    if let Some(claims) = request.extensions().get::<Claims>() {
        // Kiểm tra xem có phải admin không
        if claims.role == "admin" || claims.role == "system" {
            // Admin có quyền truy cập tất cả wallets
            return Ok(next.run(request).await);
        }
        
        // Lấy wallet ID từ path
        // Giả sử path có dạng /api/wallets/:address/...
        let path = request.uri().path();
        let parts: Vec<&str> = path.split('/').collect();
        
        // Tìm địa chỉ ví trong path
        let wallet_address = parts.iter()
            .skip_while(|&p| *p != "wallets")
            .nth(1)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiErrorResponse {
                        status: "error".to_string(),
                        message: "Không tìm thấy địa chỉ ví trong đường dẫn".to_string(),
                    })
                )
            })?;
        
        // Kiểm tra quyền truy cập
        let user_manager = state.lock().await;
        if !user_manager.has_wallet_access(wallet_address, &claims.sub) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: "Không có quyền truy cập ví này".to_string(),
                })
            ));
        }
    }
    
    // Tiếp tục request
    Ok(next.run(request).await)
}

// Hàm kiểm tra token trong danh sách đen
pub async fn check_token_blacklist(
    token_address: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let blacklist = TOKEN_BLACKLIST.read().unwrap();
    if blacklist.contains_key(token_address) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "error",
                "message": format!("Token {} is blacklisted", token_address)
            })),
        ));
    }
    Ok(())
}

// Hàm đăng xuất: sẽ thêm token vào danh sách đen
pub async fn logout(
    auth_header: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if let Some(token) = extract_token(auth_header) {
        // Trích xuất JWT claims
        match decode_jwt_token(&token) {
            Ok(claims) => {
                let mut blacklist = TOKEN_BLACKLIST.write().unwrap();
                blacklist.insert(claims.jti.clone(), claims.exp as u64);
                return Ok(());
            }
            Err(_) => {}
        }
    }

    Err((
        StatusCode::BAD_REQUEST,
        Json(json!({
            "status": "error",
            "message": "Invalid token"
        })),
    ))
}

// Hàm kiểm tra rate limit
pub async fn check_rate_limit(key: &str, limit: usize) -> bool {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            warn!("Lỗi khi lấy thời gian hệ thống: {}", err);
            // Fallback an toàn: cho phép request
            return true; 
        }
    };
        
    let window_size = 60; // 1 phút
    
    let mut rate_limits = RATE_LIMIT_WINDOWS.write().await;
    
    let should_allow = match rate_limits.get(key) {
        Some(data) => {
            // Kiểm tra xem window hiện tại có phải window cũ không
            if now - data.window_start >= window_size {
                // Tạo window mới
                rate_limits.insert(
                    key.to_string(),
                    RateLimitData {
                        count: 1,
                        window_start: now,
                    },
                );
                true
            } else if data.count < limit {
                // Tăng count trong window hiện tại
                rate_limits.insert(
                    key.to_string(),
                    RateLimitData {
                        count: data.count + 1,
                        window_start: data.window_start,
                    },
                );
                true
            } else {
                // Đã vượt quá limit
                false
            }
        },
        None => {
            // Tạo window mới
            rate_limits.insert(
                key.to_string(),
                RateLimitData {
                    count: 1,
                    window_start: now,
                },
            );
            true
        },
    };
    
    should_allow
}

// Hàm lấy IP từ request
pub fn extract_client_ip<B>(request: &Request<B>) -> Option<String> {
    request
        .headers()
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .or_else(|| {
            request
                .headers()
                .get("X-Real-IP")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
}

// Middleware xác thực API key cho các service khác gọi đến
pub async fn api_key_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    // Lấy API key từ header
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|value| value.to_str().ok());
    
    match api_key {
        Some(key) => {
            // Kiểm tra API key hợp lệ
            if key == "your_secure_api_key" { // Trong thực tế, lấy từ config hoặc DB
                // Tiếp tục request
                Ok(next.run(request).await)
            } else {
                Err((
                    StatusCode::UNAUTHORIZED,
                    Json(ApiErrorResponse {
                        status: "error".to_string(),
                        message: "API key không hợp lệ".to_string(),
                    })
                ))
            }
        },
        None => {
            Err((
                StatusCode::UNAUTHORIZED,
                Json(ApiErrorResponse {
                    status: "error".to_string(),
                    message: "Thiếu API key".to_string(),
                })
            ))
        }
    }
}

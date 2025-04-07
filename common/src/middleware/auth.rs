use axum::{
    http::{Request, header},
    middleware::Next,
    response::{Response, IntoResponse},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Duration as ChronoDuration, Utc};
use uuid::Uuid;
use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;

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
            secret_key: "diamond_secret_key".to_string(), // Nên lấy từ biến môi trường
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

// Thông tin người dùng trả về
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

// Quản lý blacklist token
pub struct TokenBlacklist {
    blacklisted_tokens: HashMap<String, u64>, // jti -> expiration time
    last_cleanup: u64,
}

impl Default for TokenBlacklist {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenBlacklist {
    pub fn new() -> Self {
        Self {
            blacklisted_tokens: HashMap::new(),
            last_cleanup: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    // Thêm token vào blacklist
    pub fn add_token(&mut self, jti: String, exp: u64) {
        self.blacklisted_tokens.insert(jti, exp);
        
        // Dọn dẹp blacklist mỗi giờ
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
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
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        self.blacklisted_tokens.retain(|_, exp| *exp > now);
    }
}

// Cache cho token blacklist (đã đăng xuất hoặc hết hạn)
pub static TOKEN_BLACKLIST: Lazy<RwLock<TokenBlacklist>> = Lazy::new(|| {
    RwLock::new(TokenBlacklist::new())
});

// Lazy static cho JWT_SECRET
lazy_static::lazy_static! {
    static ref JWT_SECRET: String = std::env::var("JWT_SECRET").unwrap_or_else(|_| "diamond_secret_key_for_development_only".to_string());
}

// Tạo JWT token
pub fn create_jwt_token(
    user_id: &str,
    role: UserRole,
    wallet_ids: Vec<String>,
    duration_hours: usize,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let expires_at = now + ChronoDuration::hours(duration_hours as i64);
    
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

// Logout - thêm token vào blacklist
pub async fn logout(token: &str) -> Result<(), String> {
    match decode_jwt_token(token) {
        Ok(claims) => {
            let mut blacklist = TOKEN_BLACKLIST.write().unwrap();
            blacklist.add_token(claims.jti, claims.exp as u64);
            Ok(())
        },
        Err(e) => Err(format!("Token không hợp lệ: {}", e)),
    }
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

// Middleware template cho xác thực API key cho các service khác gọi đến
pub async fn api_key_middleware<B, E>(
    request: Request<B>,
    next: Next<B>,
    api_key_validator: impl Fn(&str) -> bool,
    error_response: impl Fn() -> E,
) -> Result<Response, E> 
where
    E: IntoResponse,
{
    // Lấy API key từ header
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|value| value.to_str().ok());
    
    match api_key {
        Some(key) => {
            // Kiểm tra API key hợp lệ
            if api_key_validator(key) {
                // Tiếp tục request
                Ok(next.run(request).await)
            } else {
                Err(error_response())
            }
        },
        None => {
            Err(error_response())
        }
    }
}

// Tạo một cấu trúc tổng quát cho middleware xác thực JWT
pub struct JWTAuthMiddleware<T, E> {
    pub jwt_secret: String,
    pub blacklist_checker: fn(&str) -> bool,
    pub rate_limit_checker: fn(&str, usize) -> bool,
    pub error_handler: fn(JWTAuthError) -> E,
    pub state: T,
}

pub enum JWTAuthError {
    InvalidToken(String),
    BlacklistedToken,
    RateLimitExceeded,
}

impl<T, E> JWTAuthMiddleware<T, E> 
where
    T: Clone + Send + Sync + 'static,
    E: IntoResponse + 'static,
{
    pub fn new(
        jwt_secret: String,
        blacklist_checker: fn(&str) -> bool,
        rate_limit_checker: fn(&str, usize) -> bool,
        error_handler: fn(JWTAuthError) -> E,
        state: T,
    ) -> Self {
        Self {
            jwt_secret,
            blacklist_checker,
            rate_limit_checker,
            error_handler,
            state,
        }
    }
    
    /// Thực hiện xác thực JWT
    pub async fn into_middleware<B>(
        self,
        request: Request<B>,
        next: Next<B>,
    ) -> Result<Response, E> {
        // Lấy token từ header
        let auth_header = request.headers().get(header::AUTHORIZATION);
        
        let token = match auth_header {
            Some(header_value) => {
                let header_value = header_value.to_str().unwrap_or_default();
                if let Some(stripped) = header_value.strip_prefix("Bearer ") {
                    stripped
                } else {
                    return Err((self.error_handler)(JWTAuthError::InvalidToken("Invalid token format".to_string())));
                }
            },
            None => {
                return Err((self.error_handler)(JWTAuthError::InvalidToken("No authorization header".to_string())));
            }
        };
        
        // Giải mã token
        let claims = match decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        ) {
            Ok(token_data) => token_data.claims,
            Err(e) => {
                return Err((self.error_handler)(JWTAuthError::InvalidToken(format!("Token invalid: {}", e))));
            }
        };
        
        // Kiểm tra token có trong blacklist không
        if (self.blacklist_checker)(&claims.jti) {
            return Err((self.error_handler)(JWTAuthError::BlacklistedToken));
        }
        
        // Kiểm tra rate limit
        let rate_limit_key = format!("{}:{}", extract_client_ip(&request).unwrap_or_else(|| "unknown".to_string()), claims.sub);
        
        if !(self.rate_limit_checker)(&rate_limit_key, 100) {
            return Err((self.error_handler)(JWTAuthError::RateLimitExceeded));
        }
        
        // Gắn thông tin user vào request extensions
        let mut req = request;
        req.extensions_mut().insert(claims);
        req.extensions_mut().insert(self.state.clone());
        
        // Tiếp tục xử lý request
        Ok(next.run(req).await)
    }
}

// Middleware tổng quát cho kiểm tra quyền
pub async fn role_middleware<B, E>(
    request: Request<B>,
    next: Next<B>,
    required_roles: Vec<&'static str>,
    error_response: impl Fn() -> E,
) -> Result<Response, E>
where
    E: IntoResponse,
{
    // Lấy thông tin claims từ request extension
    if let Some(claims) = request.extensions().get::<Claims>() {
        if required_roles.iter().any(|&role| claims.role == role) {
            // Người dùng có quyền yêu cầu, tiếp tục xử lý
            return Ok(next.run(request).await);
        }
    }
    
    // Không có quyền yêu cầu
    Err(error_response())
} 
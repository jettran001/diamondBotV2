// External imports
use axum::{
    http::{Request, Response},
    middleware::Next,
    response::IntoResponse,
};

// Standard library imports
use std::{
    collections::HashMap,
    sync::Arc,
    fmt::{self, Display, Formatter},
    error::Error,
    any::Any,
};

// Third party imports
use anyhow::{Result, Context, anyhow};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::future::Future;
use tokio::time::{timeout, sleep};

/// Thông tin người dùng
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserInfo {
    /// ID người dùng
    pub id: String,
    /// Tên người dùng
    pub username: String,
    /// Danh sách vai trò
    pub roles: Vec<String>,
    /// Danh sách quyền
    pub permissions: Vec<String>,
    /// Metadata tùy chỉnh
    pub metadata: HashMap<String, String>,
}

/// Thông tin request
#[derive(Clone, Debug)]
pub struct RequestContext {
    /// Đường dẫn request
    pub path: String,
    /// Phương thức HTTP
    pub method: String,
    /// Thông tin người dùng
    pub user: Option<UserInfo>,
    /// ID request
    pub request_id: String,
    /// Thời gian bắt đầu
    pub start_time: DateTime<Utc>,
    /// IP client
    pub client_ip: Option<String>,
    /// Các thuộc tính tùy chỉnh
    pub attributes: HashMap<String, Arc<dyn Any + Send + Sync>>,
}

impl RequestContext {
    /// Tạo mới RequestContext
    pub fn new(path: &str, method: &str) -> Self {
        Self {
            path: path.to_string(),
            method: method.to_string(),
            user: None,
            request_id: Uuid::new_v4().to_string(),
            start_time: Utc::now(),
            client_ip: None,
            attributes: HashMap::new(),
        }
    }
    
    /// Đặt thuộc tính tùy chỉnh
    pub fn set_attribute<T: 'static + Send + Sync>(&mut self, key: &str, value: T) {
        self.attributes.insert(key.to_string(), Arc::new(value));
    }
    
    /// Lấy thuộc tính tùy chỉnh
    pub fn get_attribute<T: 'static + Send + Sync + Clone>(&self, key: &str) -> Option<Arc<T>> {
        self.attributes.get(key).and_then(|value| {
            value.downcast_ref::<T>().map(|v| Arc::new(v.clone()))
        })
    }
}

/// Loại kết quả request
pub enum RequestOutcome {
    /// Chuyển tiếp cho middleware tiếp theo
    Next,
    /// Dừng chuỗi middleware với kết quả
    Stop(Result<()>),
}

/// Middleware trait
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Xử lý request
    async fn process(&self, ctx: &mut RequestContext) -> RequestOutcome;
}

/// Middleware chain
pub struct MiddlewareChain {
    /// Danh sách middleware
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
    /// Tạo mới MiddlewareChain
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }
    
    /// Thêm middleware vào chuỗi
    pub fn add(&mut self, middleware: impl Middleware + 'static) -> &mut Self {
        self.middlewares.push(Arc::new(middleware));
        self
    }
    
    /// Thực thi chuỗi middleware
    pub async fn execute(&self, ctx: &mut RequestContext) -> Result<()> {
        for middleware in &self.middlewares {
            match middleware.process(ctx).await {
                RequestOutcome::Next => continue,
                RequestOutcome::Stop(result) => return result,
            }
        }
        
        Ok(())
    }
}

/// Authentication middleware
pub struct AuthMiddleware {
    /// Secret key cho JWT
    jwt_secret: String,
    /// Danh sách vai trò yêu cầu
    required_roles: Option<Vec<String>>,
}

impl AuthMiddleware {
    /// Tạo mới AuthMiddleware
    pub fn new(jwt_secret: &str) -> Self {
        Self {
            jwt_secret: jwt_secret.to_string(),
            required_roles: None,
        }
    }
    
    /// Yêu cầu các vai trò cụ thể
    pub fn require_roles(mut self, roles: Vec<String>) -> Self {
        self.required_roles = Some(roles);
        self
    }
    
    /// Xác thực token
    fn validate_token(&self, token: &str) -> Result<UserInfo> {
        // TODO: Implement JWT validation
        // Ví dụ giả định:
        if token == "invalid" {
            return Err(anyhow!("Invalid token"));
        }
        
        // Mô phỏng user info
        let user = UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            roles: vec!["user".to_string(), "admin".to_string()],
            permissions: vec!["read".to_string(), "write".to_string()],
            metadata: HashMap::new(),
        };
        
        Ok(user)
    }
}

#[async_trait]
impl Middleware for AuthMiddleware {
    async fn process(&self, ctx: &mut RequestContext) -> RequestOutcome {
        // Lấy token từ context (giả định đã được đặt bởi middleware trước đó)
        let token = match ctx.get_attribute::<String>("auth_token") {
            Some(token) => token.to_string(),
            None => return RequestOutcome::Stop(Err(anyhow!("No auth token provided"))),
        };
        
        // Xác thực token
        match self.validate_token(&token) {
            Ok(user) => {
                // Kiểm tra vai trò nếu cần
                if let Some(required_roles) = &self.required_roles {
                    let has_required_role = required_roles.iter()
                        .any(|role| user.roles.contains(role));
                        
                    if !has_required_role {
                        return RequestOutcome::Stop(Err(anyhow!("Insufficient permissions")));
                    }
                }
                
                // Lưu thông tin user vào context
                ctx.user = Some(user);
                RequestOutcome::Next
            },
            Err(e) => RequestOutcome::Stop(Err(e)),
        }
    }
}

/// Rate limiting middleware
pub struct RateLimitMiddleware {
    /// Số lượng request tối đa
    max_requests: u32,
    /// Kích thước window (giây)
    window_seconds: u64,
    /// Redis service
    redis_service: Option<Arc<dyn RedisProvider>>,
}

/// Redis provider interface
pub trait RedisProvider: Send + Sync + 'static {
    // Các phương thức cần thiết cho Redis
    fn get(&self, key: &str) -> Box<dyn Future<Output = Result<Option<String>>> + Send + '_>;
    fn set(&self, key: &str, value: &str) -> Box<dyn Future<Output = Result<()>> + Send + '_>;
    fn del(&self, key: &str) -> Box<dyn Future<Output = Result<()>> + Send + '_>;
    fn exists(&self, key: &str) -> Box<dyn Future<Output = Result<bool>> + Send + '_>;
    fn expire(&self, key: &str, seconds: usize) -> Box<dyn Future<Output = Result<()>> + Send + '_>;
}

impl RateLimitMiddleware {
    /// Tạo mới RateLimitMiddleware
    pub fn new(max_requests: u32, window_seconds: u64) -> Self {
        Self {
            max_requests,
            window_seconds,
            redis_service: None,
        }
    }
    
    /// Thêm Redis service
    pub fn with_redis(mut self, redis: Arc<dyn RedisProvider>) -> Self {
        self.redis_service = Some(redis);
        self
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn process(&self, ctx: &mut RequestContext) -> RequestOutcome {
        // Lấy key xác định client (IP hoặc user ID)
        let _client_key = match &ctx.user {
            Some(user) => format!("ratelimit:user:{}", user.id),
            None => match &ctx.client_ip {
                Some(ip) => format!("ratelimit:ip:{}", ip),
                None => return RequestOutcome::Stop(Err(anyhow!("No client identifier for rate limiting"))),
            },
        };
        
        // TODO: Implement rate limiting with Redis or in-memory
        // Giả định kiểm tra đơn giản (luôn thành công)
        RequestOutcome::Next
    }
}

/// Logging middleware
pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn process(&self, ctx: &mut RequestContext) -> RequestOutcome {
        // Log thông tin request khi bắt đầu
        info!(
            "Request started: {} {} [{}]",
            ctx.method,
            ctx.path,
            ctx.request_id
        );
        
        // Log thời gian khi kết thúc (được xử lý ở nơi khác)
        
        RequestOutcome::Next
    }
}

/// CORS middleware
pub struct CorsMiddleware {
    /// Danh sách origin được phép
    allowed_origins: Vec<String>,
    /// Cho phép credentials
    allow_credentials: bool,
    /// Danh sách header được phép
    allowed_headers: Vec<String>,
    /// Danh sách phương thức được phép
    allowed_methods: Vec<String>,
    /// Thời gian cache (giây)
    max_age: u32,
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CorsMiddleware {
    /// Tạo mới CorsMiddleware
    pub fn new() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allow_credentials: false,
            allowed_headers: vec!["*".to_string()],
            allowed_methods: vec!["*".to_string()],
            max_age: 86400,
        }
    }
    
    /// Thêm origin được phép
    pub fn allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }
    
    /// Thêm phương thức được phép
    pub fn allowed_methods(mut self, methods: Vec<String>) -> Self {
        self.allowed_methods = methods;
        self
    }
    
    /// Thêm header được phép
    pub fn allowed_headers(mut self, headers: Vec<String>) -> Self {
        self.allowed_headers = headers;
        self
    }
    
    /// Cho phép credentials
    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }
    
    /// Đặt thời gian cache
    pub fn max_age(mut self, seconds: u32) -> Self {
        self.max_age = seconds;
        self
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    async fn process(&self, _ctx: &mut RequestContext) -> RequestOutcome {
        // TODO: Implement CORS headers
        RequestOutcome::Next
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test RequestContext
    #[test]
    fn test_request_context() {
        let mut ctx = RequestContext::new("/test", "GET");
        assert_eq!(ctx.path, "/test");
        assert_eq!(ctx.method, "GET");
        assert!(ctx.user.is_none());
        assert!(ctx.client_ip.is_none());
    }

    /// Test AuthMiddleware
    #[test]
    fn test_auth_middleware() {
        let middleware = AuthMiddleware::new("secret");
        let user = middleware.validate_token("valid").unwrap();
        assert_eq!(user.id, "user123");
        assert_eq!(user.username, "testuser");
    }

    /// Test RateLimitMiddleware
    #[test]
    fn test_rate_limit_middleware() {
        let middleware = RateLimitMiddleware::new(100, 60);
        assert_eq!(middleware.max_requests, 100);
        assert_eq!(middleware.window_seconds, 60);
    }

    /// Test CorsMiddleware
    #[test]
    fn test_cors_middleware() {
        let middleware = CorsMiddleware::new();
        assert_eq!(middleware.allowed_origins, vec!["*"]);
        assert!(!middleware.allow_credentials);
        assert_eq!(middleware.allowed_headers, vec!["*"]);
        assert_eq!(middleware.allowed_methods, vec!["*"]);
        assert_eq!(middleware.max_age, 86400);
    }
} 
use std::sync::Arc;
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use log::info;
use chrono::{DateTime, Utc};

/// Thông tin người dùng
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Thông tin request
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub path: String,
    pub method: String,
    pub user: Option<UserInfo>,
    pub request_id: String,
    pub start_time: DateTime<Utc>,
    pub client_ip: Option<String>,
    pub attributes: std::collections::HashMap<String, Arc<dyn std::any::Any + Send + Sync>>,
}

impl RequestContext {
    pub fn new(path: &str, method: &str) -> Self {
        Self {
            path: path.to_string(),
            method: method.to_string(),
            user: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            start_time: Utc::now(),
            client_ip: None,
            attributes: std::collections::HashMap::new(),
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
    Next, // Chuyển tiếp cho middleware tiếp theo
    Stop(Result<()>), // Dừng chuỗi middleware với kết quả
}

/// Middleware trait
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn process(&self, ctx: &mut RequestContext) -> RequestOutcome;
}

/// Middleware chain
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
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
#[allow(dead_code)] // Thêm chú thích cho trường không đọc
pub struct AuthMiddleware {
    jwt_secret: String,
    required_roles: Option<Vec<String>>,
}

impl AuthMiddleware {
    pub fn new(jwt_secret: &str) -> Self {
        Self {
            jwt_secret: jwt_secret.to_string(),
            required_roles: None,
        }
    }
    
    pub fn require_roles(mut self, roles: Vec<String>) -> Self {
        self.required_roles = Some(roles);
        self
    }
    
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
            metadata: std::collections::HashMap::new(),
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
#[allow(dead_code)] // Thêm chú thích cho các trường không đọc
pub struct RateLimitMiddleware {
    max_requests: u32,
    window_seconds: u64,
    redis_service: Option<Arc<dyn RedisProvider>>,
}

/// Redis provider trait
pub trait RedisProvider: Send + Sync {
    // Các phương thức cần thiết cho Redis
}

impl RateLimitMiddleware {
    pub fn new(max_requests: u32, window_seconds: u64) -> Self {
        Self {
            max_requests,
            window_seconds,
            redis_service: None,
        }
    }
    
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
    allowed_origins: Vec<String>,
    allow_credentials: bool,
    allowed_headers: Vec<String>,
    allowed_methods: Vec<String>,
    max_age: u32,
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl CorsMiddleware {
    pub fn new() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allow_credentials: false,
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            allowed_methods: vec!["GET".to_string(), "POST".to_string(), "PUT".to_string(), "DELETE".to_string()],
            max_age: 86400, // 24 giờ
        }
    }
    
    pub fn allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }
    
    pub fn allowed_methods(mut self, methods: Vec<String>) -> Self {
        self.allowed_methods = methods;
        self
    }
    
    pub fn allowed_headers(mut self, headers: Vec<String>) -> Self {
        self.allowed_headers = headers;
        self
    }
    
    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }
    
    pub fn max_age(mut self, seconds: u32) -> Self {
        self.max_age = seconds;
        self
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    async fn process(&self, _ctx: &mut RequestContext) -> RequestOutcome {
        // TODO: Implement CORS checking and headers
        // Ví dụ đơn giản này luôn cho phép truy cập
        RequestOutcome::Next
    }
} 
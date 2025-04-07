use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use once_cell::sync::Lazy;

// Cấu trúc dữ liệu cho rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitData {
    pub count: usize,
    pub window_start: u64,
}

// Cache cho rate limit, sử dụng tokio::sync::RwLock cho async access
pub static RATE_LIMIT_CACHE: Lazy<RwLock<HashMap<String, RateLimitData>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Hàm kiểm tra rate limit
pub async fn check_rate_limit(key: &str, limit: usize, window_size: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut rate_limits = RATE_LIMIT_CACHE.write().await;
    
    if let Some(data) = rate_limits.get(key).cloned() {
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
    } else {
        // Tạo window mới
        rate_limits.insert(
            key.to_string(),
            RateLimitData {
                count: 1,
                window_start: now,
            },
        );
        true
    }
}

pub struct RateLimiter {
    window_size: u64,    // Seconds
    cleanup_interval: u64, // Seconds
    last_cleanup: u64,
}

impl RateLimiter {
    pub fn new(window_size: u64, cleanup_interval: u64) -> Self {
        Self {
            window_size,
            cleanup_interval,
            last_cleanup: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }
    }
    
    pub async fn check_limit(&mut self, key: &str, limit: usize) -> bool {
        let result = check_rate_limit(key, limit, self.window_size).await;
        
        // Dọn dẹp cache định kỳ
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now - self.last_cleanup > self.cleanup_interval {
            self.cleanup().await;
            self.last_cleanup = now;
        }
        
        result
    }
    
    async fn cleanup(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut cache = RATE_LIMIT_CACHE.write().await;
        
        // Xóa các entry hết hạn
        cache.retain(|_, data| now - data.window_start < self.window_size);
    }
}

// Custom middleware cho rate limit
pub async fn rate_limit_middleware<B, F, E>(
    request: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
    key_extractor: F,
    limit: usize,
    window_size: u64,
    error_response: impl Fn() -> E,
) -> Result<axum::response::Response, E>
where
    F: FnOnce(&axum::http::Request<B>) -> String,
    E: axum::response::IntoResponse,
{
    let key = key_extractor(&request);
    
    if check_rate_limit(&key, limit, window_size).await {
        Ok(next.run(request).await)
    } else {
        Err(error_response())
    }
}

// IP-based rate limiter
pub async fn ip_rate_limit_middleware<B, E>(
    request: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
    limit: usize,
    window_size: u64,
    error_response: impl Fn() -> E,
) -> Result<axum::response::Response, E>
where
    E: axum::response::IntoResponse,
{
    use crate::middleware::auth::extract_client_ip;
    
    let ip = extract_client_ip(&request).unwrap_or_else(|| "unknown".to_string());
    
    if check_rate_limit(&ip, limit, window_size).await {
        Ok(next.run(request).await)
    } else {
        Err(error_response())
    }
} 
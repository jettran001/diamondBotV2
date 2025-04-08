// External imports
use axum::{
    http::Request,
    middleware::Next,
    response::{Response, IntoResponse},
};

// Standard library imports
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
    fmt::{self, Display, Formatter},
    error::Error,
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::sync::RwLock;
use once_cell::sync::Lazy;
use tokio::time::{timeout, sleep};

/// Cấu trúc dữ liệu cho rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitData {
    /// Số lượng request trong window
    pub count: usize,
    /// Thời gian bắt đầu window
    pub window_start: u64,
}

/// Cache cho rate limit, sử dụng tokio::sync::RwLock cho async access
pub static RATE_LIMIT_CACHE: Lazy<RwLock<HashMap<String, RateLimitData>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

/// Hàm kiểm tra rate limit
///
/// # Arguments
///
/// * `key` - Key để phân biệt các request
/// * `limit` - Giới hạn số lượng request trong window
/// * `window_size` - Kích thước window (giây)
///
/// # Returns
///
/// * `bool` - True nếu request được phép, False nếu vượt quá limit
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

/// Rate limiter
pub struct RateLimiter {
    /// Kích thước window (giây)
    window_size: u64,
    /// Khoảng thời gian dọn dẹp (giây)
    cleanup_interval: u64,
    /// Thời gian dọn dẹp cuối
    last_cleanup: u64,
}

impl RateLimiter {
    /// Tạo mới RateLimiter
    pub fn new(window_size: u64, cleanup_interval: u64) -> Self {
        Self {
            window_size,
            cleanup_interval,
            last_cleanup: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }
    }
    
    /// Kiểm tra rate limit
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
    
    /// Dọn dẹp cache
    async fn cleanup(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut cache = RATE_LIMIT_CACHE.write().await;
        
        // Xóa các entry hết hạn
        cache.retain(|_, data| now - data.window_start < self.window_size);
    }
}

/// Custom middleware cho rate limit
///
/// # Arguments
///
/// * `request` - Request cần kiểm tra
/// * `next` - Middleware tiếp theo
/// * `key_extractor` - Hàm trích xuất key từ request
/// * `limit` - Giới hạn số lượng request trong window
/// * `window_size` - Kích thước window (giây)
/// * `error_response` - Hàm tạo response lỗi
///
/// # Returns
///
/// * `Result<Response, E>` - Response hoặc lỗi
pub async fn rate_limit_middleware<B, F, E>(
    request: Request<B>,
    next: Next<B>,
    key_extractor: F,
    limit: usize,
    window_size: u64,
    error_response: impl Fn() -> E,
) -> Result<Response, E>
where
    F: FnOnce(&Request<B>) -> String,
    E: IntoResponse,
{
    let key = key_extractor(&request);
    
    if check_rate_limit(&key, limit, window_size).await {
        Ok(next.run(request).await)
    } else {
        Err(error_response())
    }
}

/// IP-based rate limiter
///
/// # Arguments
///
/// * `request` - Request cần kiểm tra
/// * `next` - Middleware tiếp theo
/// * `limit` - Giới hạn số lượng request trong window
/// * `window_size` - Kích thước window (giây)
/// * `error_response` - Hàm tạo response lỗi
///
/// # Returns
///
/// * `Result<Response, E>` - Response hoặc lỗi
pub async fn ip_rate_limit_middleware<B, E>(
    request: Request<B>,
    next: Next<B>,
    limit: usize,
    window_size: u64,
    error_response: impl Fn() -> E,
) -> Result<Response, E>
where
    E: IntoResponse,
{
    use crate::middleware::auth::extract_client_ip;
    
    let ip = extract_client_ip(&request).unwrap_or_else(|| "unknown".to_string());
    
    if check_rate_limit(&ip, limit, window_size).await {
        Ok(next.run(request).await)
    } else {
        Err(error_response())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test rate limit
    #[test]
    fn test_rate_limit() {
        let key = "test_key";
        let limit = 5;
        let window_size = 60;
        
        // Test trong cùng một window
        for _ in 0..limit {
            assert!(check_rate_limit(key, limit, window_size).await);
        }
        
        // Test vượt quá limit
        assert!(!check_rate_limit(key, limit, window_size).await);
    }

    /// Test rate limiter
    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(60, 3600);
        let key = "test_key";
        let limit = 5;
        
        // Test trong cùng một window
        for _ in 0..limit {
            assert!(limiter.check_limit(key, limit).await);
        }
        
        // Test vượt quá limit
        assert!(!limiter.check_limit(key, limit).await);
    }

    /// Test rate limit middleware
    #[test]
    fn test_rate_limit_middleware() {
        let request = Request::new(());
        let key = "test_key";
        let limit = 5;
        let window_size = 60;
        
        // Test trong cùng một window
        for _ in 0..limit {
            assert!(rate_limit_middleware(
                request.clone(),
                Next::new(|| async { Response::new(()) }),
                |_| key.to_string(),
                limit,
                window_size,
                || Response::new(())
            ).await.is_ok());
        }
        
        // Test vượt quá limit
        assert!(rate_limit_middleware(
            request,
            Next::new(|| async { Response::new(()) }),
            |_| key.to_string(),
            limit,
            window_size,
            || Response::new(())
        ).await.is_err());
    }
} 
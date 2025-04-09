// External imports

// Standard library imports
use std::fmt;
use std::sync::Arc;

// Third party imports
use async_trait::async_trait;
use anyhow;

// Internal imports
use crate::types::*;

// Import các module middleware
mod auth;
mod core;
mod rate_limit;

// Re-export các module để người dùng có thể truy cập dễ dàng
pub use auth::*;
pub use core::*;
pub use rate_limit::*;

// Re-export UserRole để tiện sử dụng
pub use crate::models::user::UserRole;

/// Interface chung cho tất cả các middleware
#[async_trait]
pub trait Middleware {
    type Error: std::error::Error + Send + Sync + 'static;
    
    async fn process<B>(&self, request: Request<B>, next: Next<B>) -> Result<Response, Self::Error>
    where
        B: Send + 'static;
}

/// Interface cho các middleware
#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// Xử lý request
    async fn handle_request(&self, request: &Request) -> Result<Response, anyhow::Error>;
    
    /// Xử lý response
    async fn handle_response(&self, response: &Response) -> Result<Response, anyhow::Error>;
}

/// Request struct
#[derive(Debug, Clone)]
pub struct Request {
    /// Path của request
    pub path: String,
    /// Method của request
    pub method: String,
    /// Headers của request
    pub headers: Vec<(String, String)>,
    /// Body của request
    pub body: Vec<u8>,
}

/// Response struct
#[derive(Debug, Clone)]
pub struct Response {
    /// Status code
    pub status: u16,
    /// Headers của response
    pub headers: Vec<(String, String)>,
    /// Body của response
    pub body: Vec<u8>,
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test khởi tạo các middleware
    #[test]
    fn test_middleware_initialization() {
        let jwt_middleware = JWTAuthMiddleware::new(
            "secret".to_string(),
            |_| false,
            |_, _| true,
            |_| (),
            (),
        );
        assert_eq!(jwt_middleware.jwt_secret, "secret");

        let rate_limiter = RateLimiter::new(60, 3600);
        assert_eq!(rate_limiter.window_size, 60);
        assert_eq!(rate_limiter.cleanup_interval, 3600);
    }
} 
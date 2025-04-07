pub mod auth;
pub mod rate_limit;
pub mod core;

pub use auth::{
    AuthResponse, Claims, JWTAuthError, JWTAuthMiddleware, JWTConfig, 
    RefreshTokenRequest, TokenBlacklist, UserRole, api_key_middleware,
    create_jwt_token, decode_jwt_token, extract_client_ip, logout,
    role_middleware, TOKEN_BLACKLIST
};
pub use rate_limit::{RateLimiter, RateLimitData, check_rate_limit, rate_limit_middleware, ip_rate_limit_middleware};
pub use core::{
    Middleware, MiddlewareChain, RequestContext, RequestOutcome,
    AuthMiddleware, RateLimitMiddleware, LoggingMiddleware, CorsMiddleware,
    RedisProvider
}; 
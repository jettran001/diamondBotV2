# Module Middleware Dùng Chung

Module này cung cấp các middleware xác thực và kiểm soát tốc độ (rate limiting) có thể sử dụng trong toàn bộ dự án Diamond Chain.

## Cách sử dụng

### 1. Thêm dependency

Thêm dependency vào file Cargo.toml của dự án của bạn:

```toml
[dependencies]
diamond_common = { path = "../common" }
```

### 2. Xác thực JWT

```rust
use diamond_common::middleware::auth::{
    JWTAuthMiddleware, 
    JWTAuthError, 
    Claims, 
    UserRole
};
use axum::{Json, http::StatusCode};

// Tạo JWT middleware
let jwt_middleware = JWTAuthMiddleware::new(
    "your_jwt_secret".to_string(),
    |jti| {
        // Kiểm tra token có trong blacklist không
        let blacklist = diamond_common::middleware::auth::TOKEN_BLACKLIST.read().unwrap();
        blacklist.is_blacklisted(jti)
    },
    |key, limit| {
        // Kiểm tra rate limit
        diamond_common::middleware::rate_limit::check_rate_limit(key, limit, 60).await
    },
    |err| match err {
        // Xử lý lỗi
        JWTAuthError::InvalidToken(msg) => (
            StatusCode::UNAUTHORIZED,
            Json(/* Cấu trúc lỗi của bạn */)
        ),
        JWTAuthError::BlacklistedToken => (
            StatusCode::UNAUTHORIZED,
            Json(/* Cấu trúc lỗi của bạn */)
        ),
        JWTAuthError::RateLimitExceeded => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(/* Cấu trúc lỗi của bạn */)
        ),
    },
    app_state.clone(), // State của ứng dụng
);

// Sử dụng middleware với router
let app = Router::new()
    .merge(public_routes)
    .merge(
        protected_routes
            .layer(from_fn_with_state(app_state.clone(), jwt_middleware))
    )
    .with_state(app_state);
```

### 3. Kiểm tra quyền (Role)

```rust
use diamond_common::middleware::auth::role_middleware;

async fn admin_only<B>(
    request: Request<B>,
    next: Next<B>,
) -> Result<Response, impl IntoResponse> {
    role_middleware(
        request, 
        next, 
        vec!["admin", "system"],
        || (
            StatusCode::FORBIDDEN,
            Json(/* Cấu trúc lỗi của bạn */)
        )
    ).await
}

// Sử dụng với router
let admin_routes = Router::new()
    .route("/admin/users", get(list_users))
    .route_layer(middleware::from_fn(admin_only));
```

### 4. Giới hạn tốc độ (Rate Limiting)

```rust
use diamond_common::middleware::rate_limit::ip_rate_limit_middleware;

// IP-based rate limiting
let app = Router::new()
    .route("/api/public", get(public_endpoint))
    .route_layer(middleware::from_fn(|req, next| async move {
        ip_rate_limit_middleware(
            req,
            next,
            100, // limit requests
            60,  // within 60 seconds
            || (
                StatusCode::TOO_MANY_REQUESTS,
                Json(/* Cấu trúc lỗi của bạn */)
            ),
        ).await
    }));
```

### 5. Tạo và Xác thực Token JWT

```rust
use diamond_common::middleware::auth::{create_jwt_token, decode_jwt_token, UserRole};

// Tạo token
let token = create_jwt_token(
    "user123",
    UserRole::User,
    vec!["wallet1", "wallet2"],
    24 // hours
)?;

// Xác thực token
let claims = decode_jwt_token(&token)?;
```

### 6. Đăng xuất (thêm token vào blacklist)

```rust
use diamond_common::middleware::auth::logout;

async fn handle_logout(token: &str) -> Result<(), String> {
    logout(token).await
}
```

## Cấu trúc Module

- `auth.rs`: Chứa các middleware xác thực JWT, kiểm tra quyền, và blacklist.
- `rate_limit.rs`: Chứa các middleware giới hạn tốc độ truy cập API.
- `core.rs`: Chứa các middleware cơ bản như CORS, logging, và chain.

## Lưu ý

1. Các cấu trúc lỗi nên được tùy chỉnh theo từng ứng dụng.
2. Giá trị JWT secret nên được lấy từ biến môi trường hoặc file cấu hình.
3. Khi sử dụng trong môi trường sản xuất, hãy đảm bảo sử dụng HTTPS để bảo mật token.
4. Các middleware nên được sử dụng theo thứ tự phù hợp:
   - CORS middleware nên được sử dụng đầu tiên
   - Rate limiting middleware nên được sử dụng trước authentication
   - Authentication middleware nên được sử dụng trước role checking
   - Logging middleware nên được sử dụng cuối cùng

## Ví dụ đầy đủ

```rust
use diamond_common::middleware::{
    auth::{JWTAuthMiddleware, UserRole},
    rate_limit::ip_rate_limit_middleware,
    core::{CorsMiddleware, LoggingMiddleware},
};

// Tạo middleware chain
let mut chain = MiddlewareChain::new()
    .add(CorsMiddleware::new())
    .add(ip_rate_limit_middleware(100, 60))
    .add(JWTAuthMiddleware::new("secret"))
    .add(LoggingMiddleware);

// Sử dụng với router
let app = Router::new()
    .route("/api/public", get(public_endpoint))
    .route_layer(middleware::from_fn(|req, next| async move {
        chain.execute(&mut RequestContext::new(req.uri().path(), req.method().as_str())).await?;
        next.run(req).await
    }));
``` 
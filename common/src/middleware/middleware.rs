// CẢNH BÁO: TẬP TIN NÀY ĐÃ BỊ DI CHUYỂN
// =================================
//
// Tất cả mã middleware đã được di chuyển sang common::middleware
// Vui lòng sử dụng:
//   use common::middleware::auth::*; - Cho JWT và xác thực
//   use common::middleware::rate_limit::*; - Cho rate limiting
//
// Các thành phần chính trong common::middleware:
// 1. Auth:
//    - auth_middleware: Middleware xác thực JWT
//    - create_token: Tạo token JWT mới
//    - logout: Logout user, blacklist token
//    - JWTAuthMiddleware: Middleware xử lý xác thực
//
// 2. Rate Limiting:
//    - ip_rate_limit_middleware: Giới hạn request theo IP
//    - rate_limit_middleware: Middleware giới hạn rate
//
// File này được giữ lại chỉ để tương thích ngược và sẽ bị xóa trong phiên bản tới.
// Tất cả code mới nên sử dụng common::middleware thay vì module này.

#[deprecated(since = "0.2.0", note = "Please use common::middleware instead")]
pub use common::middleware::*;

// Re-export từ common crate
pub use common::middleware::auth::*;
pub use common::middleware::rate_limit::*; 
pub use common::middleware::core::*;

// Cảnh báo lỗi không phải dùng file này nữa
#[deprecated(
    since = "0.2.0",
    note = "Các phần này đã được di chuyển sang common::middleware, hãy import từ đó thay vì file này"
)]
pub struct DeprecatedMiddleware;

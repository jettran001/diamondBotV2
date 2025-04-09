
// External imports
use thiserror::Error;
use std::fmt;

/// Các lỗi liên quan đến mạng
#[derive(Error, Debug, Clone)]
pub enum NetworkError {
    #[error("Lỗi kết nối: {0}")]
    ConnectionError(String),
    
    #[error("Lỗi timeout sau {0} giây")]
    TimeoutError(u64),
    
    #[error("Lỗi xác thực: {0}")]
    AuthenticationError(String),
    
    #[error("Lỗi giao thức: {0}")]
    ProtocolError(String),
    
    #[error("Lỗi SSL/TLS: {0}")]
    TlsError(String),
    
    #[error("Lỗi dữ liệu: {0}")]
    DataError(String),
    
    #[error("Lỗi không xác định: {0}")]
    UnknownError(String),
}

impl NetworkError {
    /// Kiểm tra xem lỗi có thể retry không
    pub fn is_retryable(&self) -> bool {
        match self {
            NetworkError::ConnectionError(_) | NetworkError::TimeoutError(_) => true,
            _ => false,
        }
    }
    
    /// Lấy mô tả lỗi
    pub fn description(&self) -> String {
        match self {
            NetworkError::ConnectionError(msg) => format!("Lỗi kết nối: {}", msg),
            NetworkError::TimeoutError(seconds) => format!("Timeout sau {} giây", seconds),
            NetworkError::AuthenticationError(msg) => format!("Lỗi xác thực: {}", msg),
            NetworkError::ProtocolError(msg) => format!("Lỗi giao thức: {}", msg),
            NetworkError::TlsError(msg) => format!("Lỗi SSL/TLS: {}", msg),
            NetworkError::DataError(msg) => format!("Lỗi dữ liệu: {}", msg),
            NetworkError::UnknownError(msg) => format!("Lỗi không xác định: {}", msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_retryable() {
        let conn_error = NetworkError::ConnectionError("test".to_string());
        let auth_error = NetworkError::AuthenticationError("test".to_string());
        
        assert!(conn_error.is_retryable());
        assert!(!auth_error.is_retryable());
    }
}

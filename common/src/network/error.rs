// External imports
use anyhow::{Result, Context};
use thiserror::Error;

// Standard library imports
use std::{
    io,
    fmt::{self, Display, Formatter},
    error::Error as StdError,
};

/// Network error
#[derive(Debug, Error)]
pub enum NetworkError {
    /// Server đã chạy
    #[error("Server is already running")]
    ServerAlreadyRunning,
    
    /// Server chưa chạy
    #[error("Server is not running")]
    ServerNotRunning,
    
    /// Lỗi kết nối
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    /// Lỗi timeout
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    /// Lỗi xác thực
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    /// Lỗi giao thức
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    /// Lỗi serialization
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    /// Lỗi không xác định
    #[error("Unknown error: {0}")]
    Unknown(String),
    
    /// Lỗi I/O
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
}

impl NetworkError {
    /// Create a new connection error
    pub fn connection_error<T: ToString>(msg: T) -> Self {
        Self::ConnectionError(msg.to_string())
    }
    
    /// Create a new authentication error
    pub fn auth_error<T: ToString>(msg: T) -> Self {
        Self::AuthError(msg.to_string())
    }
    
    /// Create a new protocol error
    pub fn protocol_error<T: ToString>(msg: T) -> Self {
        Self::ProtocolError(msg.to_string())
    }
    
    /// Create a new serialization error
    pub fn serialization_error<T: ToString>(msg: T) -> Self {
        Self::SerializationError(msg.to_string())
    }
    
    /// Create a new unknown error
    pub fn unknown<T: ToString>(msg: T) -> Self {
        Self::Unknown(msg.to_string())
    }
}

/// Network result type
pub type NetworkResult<T> = Result<T, NetworkError>;

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test NetworkError
    #[test]
    fn test_network_error() {
        let io_error = NetworkError::IoError(io::Error::new(io::ErrorKind::Other, "test"));
        assert!(matches!(io_error, NetworkError::IoError(_)));
        
        let conn_error = NetworkError::connection_error("test");
        assert!(matches!(conn_error, NetworkError::ConnectionError(_)));
        
        let auth_error = NetworkError::auth_error("test");
        assert!(matches!(auth_error, NetworkError::AuthError(_)));
        
        let proto_error = NetworkError::protocol_error("test");
        assert!(matches!(proto_error, NetworkError::ProtocolError(_)));
        
        let serial_error = NetworkError::serialization_error("test");
        assert!(matches!(serial_error, NetworkError::SerializationError(_)));
        
        let unknown_error = NetworkError::unknown("test");
        assert!(matches!(unknown_error, NetworkError::Unknown(_)));
    }

    /// Test error messages
    #[test]
    fn test_error_messages() {
        let error = NetworkError::connection_error("failed to connect");
        assert_eq!(error.to_string(), "Connection error: failed to connect");
        
        let error = NetworkError::TimeoutError;
        assert_eq!(error.to_string(), "Timeout error");
        
        let error = NetworkError::NotConnected;
        assert_eq!(error.to_string(), "Not connected");
    }
} 
use thiserror::Error;
use std::io;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Timeout error")]
    TimeoutError,
    
    #[error("Not connected")]
    NotConnected,
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type NetworkResult<T> = Result<T, NetworkError>; 
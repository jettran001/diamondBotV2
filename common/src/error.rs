use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("HTTP error: {0}")]
    HTTP(String),
    
    #[error("Authentication error: {0}")]
    Auth(String),
    
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Rate limit exceeded")]
    RateLimit,
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
} 
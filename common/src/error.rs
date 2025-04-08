// Standard library imports

// Third party imports
use thiserror::Error;
use anyhow::Result;

/// Lỗi chung
#[derive(Debug, Error)]
pub enum CommonError {
    /// Lỗi không xác định
    #[error("Unknown error: {0}")]
    Unknown(String),
    /// Lỗi không tìm thấy
    #[error("Not found: {0}")]
    NotFound(String),
    /// Lỗi không hợp lệ
    #[error("Invalid: {0}")]
    Invalid(String),
    /// Lỗi không được phép
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    /// Lỗi không đủ tiền
    #[error("Insufficient funds: {0}")]
    InsufficientFunds(String),
    /// Lỗi không đủ gas
    #[error("Insufficient gas: {0}")]
    InsufficientGas(String),
    /// Lỗi không đủ nonce
    #[error("Insufficient nonce: {0}")]
    InsufficientNonce(String),
    /// Lỗi không đủ block
    #[error("Insufficient block: {0}")]
    InsufficientBlock(String),
    /// Lỗi không đủ time
    #[error("Insufficient time: {0}")]
    InsufficientTime(String),
    /// Lỗi không đủ retry
    #[error("Insufficient retry: {0}")]
    InsufficientRetry(String),
    /// Lỗi không đủ cache
    #[error("Insufficient cache: {0}")]
    InsufficientCache(String),
    /// Lỗi không đủ network
    #[error("Insufficient network: {0}")]
    InsufficientNetwork(String),
    /// Lỗi không đủ chain
    #[error("Insufficient chain: {0}")]
    InsufficientChain(String),
    /// Lỗi không đủ adapter
    #[error("Insufficient adapter: {0}")]
    InsufficientAdapter(String),
    /// Lỗi không đủ analyzer
    #[error("Insufficient analyzer: {0}")]
    InsufficientAnalyzer(String),
    /// Lỗi không đủ equilibrium
    #[error("Insufficient equilibrium: {0}")]
    InsufficientEquilibrium(String),
    /// Lỗi không đủ ai
    #[error("Insufficient ai: {0}")]
    InsufficientAI(String),
    /// Lỗi không đủ diamond
    #[error("Insufficient diamond: {0}")]
    InsufficientDiamond(String),
    /// Lỗi không đủ cache
    #[error("Insufficient cache manager: {0}")]
    InsufficientCacheManager(String),
    /// Lỗi không đủ retry
    #[error("Insufficient retry policy: {0}")]
    InsufficientRetryPolicy(String),
    /// Lỗi mạng
    #[error("Network error: {0}")]
    Network(String),
    /// Lỗi blockchain
    #[error("Blockchain error: {0}")]
    Blockchain(String),
    /// Lỗi ví
    #[error("Wallet error: {0}")]
    Wallet(String),
    /// Lỗi cấu hình
    #[error("Config error: {0}")]
    Config(String),
    /// Lỗi khác
    #[error("Other error: {0}")]
    Other(String),
}

/// Kiểu kết quả chung
pub type CommonResult<T> = Result<T, CommonError>;

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test CommonError
    #[test]
    fn test_common_error() {
        let error = CommonError::Unknown("test".to_string());
        assert_eq!(error.to_string(), "Unknown error: test");
    }
} 
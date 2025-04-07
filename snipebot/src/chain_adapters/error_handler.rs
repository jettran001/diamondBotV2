use std::error::Error;
use std::fmt;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use anyhow::{Result, Context};
use std::sync::Arc;
use tracing::{info, warn, error};
use ethers::providers::ProviderError;
use crate::chain_adapters::interfaces::ChainError;

// Re-export ChainError từ interfaces
pub use crate::chain_adapters::interfaces::ChainError;

/// Chuyển đổi từ chuỗi lỗi sang ChainError
pub fn parse_error_message(error_message: &str) -> ChainError {
    // Phân tích lỗi dựa trên nội dung
    if error_message.contains("insufficient funds") || error_message.contains("gas required exceeds allowance") {
        ChainError::InsufficientGas(error_message.to_string())
    } else if error_message.contains("nonce too low") || error_message.contains("nonce too high") {
        ChainError::NonceError(error_message.to_string())
    } else if error_message.contains("underpriced") || error_message.contains("gas price too low") {
        ChainError::Underpriced
    } else if error_message.contains("reverted") {
        ChainError::Revert(error_message.to_string())
    } else if error_message.contains("connection") || error_message.contains("network") {
        ChainError::ConnectionError(error_message.to_string())
    } else if error_message.contains("timed out") || error_message.contains("timeout") {
        ChainError::TimeoutError(30000) // Giả định timeout sau 30s
    } else if error_message.contains("rate limit") || error_message.contains("too many requests") {
        ChainError::RateLimitExceeded(error_message.to_string())
    } else if error_message.contains("not found") && error_message.contains("block") {
        ChainError::BlockNotFound(error_message.to_string())
    } else if error_message.contains("not found") && error_message.contains("transaction") {
        ChainError::TransactionNotFound(error_message.to_string())
    } else if error_message.contains("not found") && error_message.contains("contract") {
        ChainError::ContractNotFound(error_message.to_string())
    } else if error_message.contains("circuit breaker") {
        ChainError::CircuitBreakerTriggered(error_message.to_string())
    } else if error_message.contains("max retry") || error_message.contains("retry limit") {
        ChainError::MaxRetryReached(error_message.to_string())
    } else if error_message.contains("invalid transaction") {
        ChainError::TransactionError(error_message.to_string())
    } else if error_message.contains("gas cap") || error_message.contains("gas limit exceeded") {
        ChainError::GasCap
    } else {
        ChainError::Unknown(error_message.to_string())
    }
}

/// Chuyển đổi từ anyhow::Error sang ChainError
pub fn convert_anyhow_error(err: anyhow::Error) -> ChainError {
    ChainError::from_anyhow(err)
}

/// Kiểm tra lỗi có liên quan đến gas hay không
pub fn is_gas_related_error(error: &ChainError) -> bool {
    error.is_gas_related()
}

impl ChainError {
    /// Lấy thông tin chi tiết về lỗi
    pub fn details(&self) -> String {
        match self {
            Self::ConnectionError(msg) => format!("Connection error: {}", msg),
            Self::TimeoutError(ms) => format!("Request timed out after {} ms", ms),
            Self::RateLimitExceeded(msg) => format!("Rate limit exceeded: {}", msg),
            Self::InsufficientGas(msg) => format!("Insufficient gas: {}", msg),
            Self::Revert(msg) => format!("Transaction reverted: {}", msg),
            Self::NonceError(msg) => format!("Invalid nonce: {}", msg),
            Self::Underpriced => "Transaction underpriced".to_string(),
            Self::GasCap => "Gas price exceeds cap".to_string(),
            Self::NotImplemented => "Method not implemented".to_string(),
            Self::BlockNotFound(msg) => format!("Block not found: {}", msg),
            Self::TransactionNotFound(msg) => format!("Transaction not found: {}", msg),
            Self::ContractNotFound(msg) => format!("Contract not found at address: {}", msg),
            Self::CircuitBreakerTriggered(msg) => format!("Circuit breaker triggered: {}", msg),
            Self::MaxRetryReached(msg) => format!("Maximum retry attempts reached: {}", msg),
            Self::TransactionError(msg) => format!("Invalid transaction: {}", msg),
            Self::Unknown(msg) => format!("Chain error: {}", msg),
        }
    }
    
    /// Kiểm tra xem lỗi có liên quan đến gas không
    pub fn is_gas_related(&self) -> bool {
        is_gas_related_error(self)
    }
    
    /// Lấy số lần retry đã thực hiện (nếu có thông tin)
    pub fn retry_count(&self) -> Option<usize> {
        // Phân tích từ thông báo lỗi nếu có thông tin về retry
        if let Self::MaxRetryReached(msg) = self {
            if let Some(count_str) = msg.split("after").nth(1) {
                if let Some(count_str) = count_str.trim().split(' ').next() {
                    return count_str.parse::<usize>().ok();
                }
            }
        }
        
        None
    }
    
    /// Lấy gợi ý khắc phục lỗi
    pub fn recovery_suggestion(&self) -> String {
        match self {
            Self::ConnectionError(msg) => 
                format!("Kiểm tra kết nối mạng hoặc thử đổi RPC endpoint khác: {}", msg),
            Self::TimeoutError(ms) => 
                format!("Tăng thời gian chờ hoặc thử lại sau khi mạng ổn định hơn: {} ms", ms),
            Self::RateLimitExceeded(msg) => 
                format!("Giảm tần suất request hoặc sử dụng RPC endpoint khác: {}", msg),
            Self::InsufficientGas(msg) => 
                "Tăng gas limit hoặc gas price và thử lại".to_string(),
            Self::Revert(msg) => 
                "Kiểm tra lại logic contract và tham số gọi".to_string(),
            Self::NonceError(msg) => 
                "Đợi transaction trước hoàn tất hoặc reset nonce".to_string(),
            Self::Underpriced => 
                "Tăng gas price và thử lại".to_string(),
            Self::GasCap => 
                "Giảm gas price xuống dưới giới hạn của mạng".to_string(),
            Self::NotImplemented => 
                "Sử dụng tính năng thay thế hoặc chờ cập nhật".to_string(),
            Self::BlockNotFound(msg) => 
                "Kiểm tra lại số block hoặc hash".to_string(),
            Self::TransactionNotFound(msg) => 
                "Kiểm tra lại transaction hash hoặc đợi transaction được xác nhận".to_string(),
            Self::ContractNotFound(msg) => 
                "Kiểm tra địa chỉ contract trên block explorer".to_string(),
            Self::CircuitBreakerTriggered(msg) => 
                "Đợi circuit breaker reset hoặc sử dụng endpoint khác".to_string(),
            Self::MaxRetryReached(msg) => 
                "Tăng số lần retry hoặc kiểm tra lỗi gốc".to_string(),
            Self::TransactionError(msg) => 
                "Kiểm tra lại thông số transaction và định dạng dữ liệu".to_string(),
            Self::Unknown(msg) => 
                "Kiểm tra logs chi tiết và liên hệ hỗ trợ kỹ thuật".to_string(),
        }
    }
}

pub fn handle_chain_error<T>(result: Result<T>, operation: &str) -> Result<T> {
    result.with_context(|| format!("Failed to {}", operation))
}

pub fn handle_provider_error(error: ProviderError) -> ChainError {
    match error {
        ProviderError::JsonRpcClientError(e) => ChainError::ConnectionError(e.to_string()),
        ProviderError::EnsError(e) => ChainError::Unknown(e.to_string()),
        ProviderError::SerdeJson(e) => ChainError::Unknown(e.to_string()),
        ProviderError::HexError(e) => ChainError::Unknown(e.to_string()),
        ProviderError::HTTPError(e) => ChainError::ConnectionError(e.to_string()),
        _ => ChainError::Unknown(error.to_string()),
    }
} 
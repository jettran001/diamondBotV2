use std::error::Error;
use std::fmt;
use tracing::{info, warn, error};
use thiserror::Error;
use ethers::prelude::*;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Lỗi kết nối blockchain: {0}")]
    ConnectionError(String),
    
    #[error("Lỗi gas không đủ: {0}")]
    InsufficientGas(String),
    
    #[error("Lỗi slippage quá thấp: {0}")]
    SlippageError(String),
    
    #[error("Lỗi thanh khoản không đủ: {0}")]
    LowLiquidity(String),
    
    #[error("Lỗi token bị revert: {0}")]
    TokenRevert(String),
    
    #[error("Lỗi chờ giao dịch quá lâu (timeout): {0}")]
    TransactionTimeout(String),
    
    #[error("Lỗi wallet không đủ số dư: {0}")]
    InsufficientBalance(String),
    
    #[error("Lỗi approve token thất bại: {0}")]
    ApprovalFailed(String),
    
    #[error("Lỗi honeypot token: {0}")]
    HoneypotToken(String),
    
    #[error("Lỗi rút token thất bại: {0}")]
    WithdrawalFailed(String),
    
    #[error("Lỗi không xác định: {0}")]
    Unknown(String),
}

// Phân loại lỗi từ chuỗi lỗi của blockchain
pub fn classify_blockchain_error(error_msg: &str) -> TransactionError {
    let error_lowercase = error_msg.to_lowercase();
    
    // Phân loại theo các từ khóa
    if error_lowercase.contains("gas") && (error_lowercase.contains("low") || error_lowercase.contains("insufficient")) {
        TransactionError::InsufficientGas(error_msg.to_string())
    } else if error_lowercase.contains("slippage") || error_lowercase.contains("k") ||
              error_lowercase.contains("amount out") || error_lowercase.contains("output amount") {
        TransactionError::SlippageError(error_msg.to_string())
    } else if error_lowercase.contains("liquidity") {
        TransactionError::LowLiquidity(error_msg.to_string())
    } else if error_lowercase.contains("revert") || error_lowercase.contains("execution reverted") {
        TransactionError::TokenRevert(error_msg.to_string())
    } else if error_lowercase.contains("timeout") || error_lowercase.contains("not mined") {
        TransactionError::TransactionTimeout(error_msg.to_string())
    } else if error_lowercase.contains("balance") && error_lowercase.contains("insufficient") {
        TransactionError::InsufficientBalance(error_msg.to_string())
    } else if error_lowercase.contains("approve") || error_lowercase.contains("allowance") {
        TransactionError::ApprovalFailed(error_msg.to_string())
    } else if error_lowercase.contains("cannot estimate") || error_lowercase.contains("transfer") {
        TransactionError::HoneypotToken(error_msg.to_string())
    } else if error_lowercase.contains("withdraw") {
        TransactionError::WithdrawalFailed(error_msg.to_string())
    } else if error_lowercase.contains("connect") || error_lowercase.contains("rpc") || 
              error_lowercase.contains("timeout") || error_lowercase.contains("unavailable") {
        TransactionError::ConnectionError(error_msg.to_string())
    } else {
        TransactionError::Unknown(error_msg.to_string())
    }
}

// Thông tin cho việc khôi phục giao dịch
#[derive(Debug, Clone)]
pub struct TransactionRecoveryInfo {
    pub should_retry: bool,
    pub increase_gas: bool,
    pub increase_gas_percent: u8,
    pub decrease_slippage: bool,
    pub wait_time_before_retry_ms: u64,
    pub suggested_action: RecoveryAction,
}

#[derive(Debug, Clone)]
pub enum RecoveryAction {
    Retry,
    IncreaseGas,
    ReduceAmount,
    AbortTransaction,
    WaitAndRetry,
    EmergencySell,
}

// Lấy thông tin phục hồi dựa trên loại lỗi
pub fn get_recovery_info(error: &TransactionError) -> TransactionRecoveryInfo {
    match error {
        TransactionError::InsufficientGas(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: true,
            increase_gas_percent: 30,
            decrease_slippage: false,
            wait_time_before_retry_ms: 2000,
            suggested_action: RecoveryAction::IncreaseGas,
        },
        TransactionError::SlippageError(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: false,
            increase_gas_percent: 0,
            decrease_slippage: true,
            wait_time_before_retry_ms: 1000,
            suggested_action: RecoveryAction::Retry,
        },
        TransactionError::LowLiquidity(_) => TransactionRecoveryInfo {
            should_retry: false,
            increase_gas: false,
            increase_gas_percent: 0,
            decrease_slippage: false,
            wait_time_before_retry_ms: 0,
            suggested_action: RecoveryAction::AbortTransaction,
        },
        TransactionError::TokenRevert(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: true, 
            increase_gas_percent: 20,
            decrease_slippage: true,
            wait_time_before_retry_ms: 3000,
            suggested_action: RecoveryAction::Retry,
        },
        TransactionError::TransactionTimeout(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: true,
            increase_gas_percent: 50,
            decrease_slippage: false,
            wait_time_before_retry_ms: 5000,
            suggested_action: RecoveryAction::IncreaseGas,
        },
        TransactionError::InsufficientBalance(_) => TransactionRecoveryInfo {
            should_retry: false,
            increase_gas: false,
            increase_gas_percent: 0,
            decrease_slippage: false,
            wait_time_before_retry_ms: 0,
            suggested_action: RecoveryAction::AbortTransaction,
        },
        TransactionError::ApprovalFailed(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: true,
            increase_gas_percent: 30,
            decrease_slippage: false,
            wait_time_before_retry_ms: 2000,
            suggested_action: RecoveryAction::Retry,
        },
        TransactionError::HoneypotToken(_) => TransactionRecoveryInfo {
            should_retry: false,
            increase_gas: false,
            increase_gas_percent: 0,
            decrease_slippage: false,
            wait_time_before_retry_ms: 0,
            suggested_action: RecoveryAction::EmergencySell,
        },
        TransactionError::ConnectionError(_) => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: false,
            increase_gas_percent: 0,
            decrease_slippage: false,
            wait_time_before_retry_ms: 5000,
            suggested_action: RecoveryAction::WaitAndRetry,
        },
        _ => TransactionRecoveryInfo {
            should_retry: true,
            increase_gas: true,
            increase_gas_percent: 10,
            decrease_slippage: true,
            wait_time_before_retry_ms: 2000,
            suggested_action: RecoveryAction::Retry,
        },
    }
}

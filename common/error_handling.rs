// External imports
use ethers::core::types::{Address, H256, U256};
use ethers::prelude::*;

// Standard library imports
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime},
};

// Internal imports
use crate::types::{TradeConfig, RiskAnalysis};

// Third party imports
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Enum định nghĩa các loại lỗi giao dịch
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
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

/// Phân loại lỗi từ chuỗi lỗi của blockchain
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

/// Thông tin cho việc khôi phục giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecoveryInfo {
    pub should_retry: bool,
    pub increase_gas: bool,
    pub increase_gas_percent: u8,
    pub decrease_slippage: bool,
    pub wait_time_before_retry_ms: u64,
    pub suggested_action: RecoveryAction,
}

/// Các hành động có thể thực hiện để khôi phục giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryAction {
    Retry,
    IncreaseGas,
    ReduceAmount,
    AbortTransaction,
    WaitAndRetry,
    EmergencySell,
}

/// Lấy thông tin phục hồi dựa trên loại lỗi
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

// =============== Phần từ error_handler.rs ===============

/// Error handler trait
#[async_trait]
pub trait ErrorHandler: Send + Sync + 'static {
    /// Thêm lỗi
    async fn add_error(&self, error: BlockchainError) -> Result<()>;

    /// Lấy lỗi
    async fn get_error(&self, error_id: &str) -> Result<Option<BlockchainError>>;

    /// Lấy tất cả lỗi
    async fn get_errors(&self) -> Result<Vec<BlockchainError>>;

    /// Xóa lỗi
    async fn remove_error(&self, error_id: &str) -> Result<()>;

    /// Xóa tất cả lỗi
    async fn clear_errors(&self) -> Result<()>;
}

/// Lỗi blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainError {
    /// ID lỗi
    pub error_id: String,
    /// Tên lỗi
    pub name: String,
    /// Mô tả lỗi
    pub description: String,
    /// Trạng thái lỗi
    pub status: ErrorStatus,
    /// Loại lỗi giao dịch
    pub transaction_error: Option<String>,
    /// Thông tin phục hồi
    pub recovery_info: Option<String>,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Trạng thái lỗi
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorStatus {
    /// Chờ
    Pending,
    /// Đang xử lý
    Processing,
    /// Đã xử lý
    Resolved,
    /// Lỗi
    Error,
}

/// Cấu hình error handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandlerConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian lưu trữ
    pub retention_period: Duration,
}

impl Default for ErrorHandlerConfig {
    fn default() -> Self {
        Self {
            config_id: "default".to_string(),
            name: "Default Error Handler".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(86400), // 24 giờ
        }
    }
}

/// Basic error handler
#[derive(Debug, Clone)]
pub struct BasicErrorHandler {
    config: Arc<RwLock<ErrorHandlerConfig>>,
    errors: Arc<RwLock<HashMap<String, BlockchainError>>>,
}

impl BasicErrorHandler {
    /// Tạo error handler mới
    pub fn new(config: ErrorHandlerConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            errors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Tạo error handler mới với cấu hình mặc định
    pub fn default() -> Self {
        Self::new(ErrorHandlerConfig::default())
    }

    /// Lấy cấu hình hiện tại
    pub fn get_config(&self) -> Result<ErrorHandlerConfig> {
        let cfg = self.config.read()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa đọc cho cấu hình: {}", e))?;
        Ok(cfg.clone())
    }

    /// Cập nhật cấu hình
    pub fn update_config(&self, config: ErrorHandlerConfig) -> Result<()> {
        let mut cfg = self.config.write()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa ghi cho cấu hình: {}", e))?;
        *cfg = config;
        Ok(())
    }
}

#[async_trait]
impl ErrorHandler for BasicErrorHandler {
    async fn add_error(&self, error: BlockchainError) -> Result<()> {
        let mut errors = self.errors.write()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa ghi cho errors: {}", e))?;
        errors.insert(error.error_id.clone(), error);
        Ok(())
    }

    async fn get_error(&self, error_id: &str) -> Result<Option<BlockchainError>> {
        let errors = self.errors.read()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa đọc cho errors: {}", e))?;
        Ok(errors.get(error_id).cloned())
    }

    async fn get_errors(&self) -> Result<Vec<BlockchainError>> {
        let errors = self.errors.read()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa đọc cho errors: {}", e))?;
        Ok(errors.values().cloned().collect())
    }

    async fn remove_error(&self, error_id: &str) -> Result<()> {
        let mut errors = self.errors.write()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa ghi cho errors: {}", e))?;
        errors.remove(error_id);
        Ok(())
    }

    async fn clear_errors(&self) -> Result<()> {
        let mut errors = self.errors.write()
            .map_err(|e| anyhow::anyhow!("Không thể lấy khóa ghi cho errors: {}", e))?;
        errors.clear();
        Ok(())
    }
}

/// Xử lý lỗi giao dịch và chuyển đổi thành BlockchainError
pub async fn handle_transaction_error(
    error_handler: &impl ErrorHandler,
    error_msg: &str,
) -> Result<TransactionRecoveryInfo> {
    // Phân loại lỗi
    let transaction_error = classify_blockchain_error(error_msg);
    
    // Lấy thông tin phục hồi
    let recovery_info = get_recovery_info(&transaction_error);
    
    // Tạo BlockchainError
    let blockchain_error: BlockchainError = transaction_error.clone().into();
    
    // Thêm lỗi vào error handler
    error_handler.add_error(blockchain_error).await?;
    
    Ok(recovery_info)
}

/// Cập nhật trạng thái lỗi
pub async fn update_error_status(
    error_handler: &impl ErrorHandler,
    error_id: &str,
    new_status: ErrorStatus,
) -> Result<()> {
    // Lấy lỗi hiện tại
    if let Some(mut error) = error_handler.get_error(error_id).await? {
        // Cập nhật trạng thái
        error.status = new_status;
        error.updated_at = SystemTime::now();
        
        // Lưu lại lỗi
        error_handler.add_error(error).await?;
        
        Ok(())
    } else {
        Err(anyhow::anyhow!("Không tìm thấy lỗi với ID: {}", error_id))
    }
}

/// Lọc lỗi theo trạng thái
pub async fn filter_errors_by_status(
    error_handler: &impl ErrorHandler,
    status: ErrorStatus,
) -> Result<Vec<BlockchainError>> {
    let errors = error_handler.get_errors().await?;
    Ok(errors.into_iter().filter(|e| e.status == status).collect())
}

/// Dọn dẹp các lỗi cũ
pub async fn cleanup_old_errors(
    error_handler: &impl ErrorHandler,
    max_age_hours: u64,
) -> Result<u64> {
    let errors = error_handler.get_errors().await?;
    let now = SystemTime::now();
    let max_age = Duration::from_secs(max_age_hours * 3600);
    
    let mut removed_count = 0;
    
    for error in errors {
        if let Ok(age) = now.duration_since(error.created_at) {
            if age > max_age {
                error_handler.remove_error(&error.error_id).await?;
                removed_count += 1;
            }
        }
    }
    
    Ok(removed_count)
}

/// From trait để chuyển đổi TransactionError thành BlockchainError
impl From<TransactionError> for BlockchainError {
    fn from(error: TransactionError) -> Self {
        let error_name = match &error {
            TransactionError::ConnectionError(_) => "CONNECTION_ERROR",
            TransactionError::InsufficientGas(_) => "INSUFFICIENT_GAS",
            TransactionError::SlippageError(_) => "SLIPPAGE_ERROR",
            TransactionError::LowLiquidity(_) => "LOW_LIQUIDITY",
            TransactionError::TokenRevert(_) => "TOKEN_REVERT",
            TransactionError::TransactionTimeout(_) => "TRANSACTION_TIMEOUT",
            TransactionError::InsufficientBalance(_) => "INSUFFICIENT_BALANCE",
            TransactionError::ApprovalFailed(_) => "APPROVAL_FAILED",
            TransactionError::HoneypotToken(_) => "HONEYPOT_TOKEN",
            TransactionError::WithdrawalFailed(_) => "WITHDRAWAL_FAILED",
            TransactionError::Unknown(_) => "UNKNOWN_ERROR",
        };
        
        let error_id = format!("{}_{}", error_name, chrono::Utc::now().timestamp());
        
        let recovery_info = serde_json::to_string(&get_recovery_info(&error)).unwrap_or_default();
        
        Self {
            error_id,
            name: error_name.to_string(),
            description: error.to_string(),
            status: ErrorStatus::Pending,
            transaction_error: Some(error.to_string()),
            recovery_info: Some(recovery_info),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        }
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blockchain_error() {
        let error = BlockchainError {
            error_id: "TEST_ERROR_1".to_string(),
            name: "TEST_ERROR".to_string(),
            description: "Test error".to_string(),
            status: ErrorStatus::Pending,
            transaction_error: Some("Test transaction error".to_string()),
            recovery_info: Some("{}".to_string()),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };
        
        assert_eq!(error.name, "TEST_ERROR");
        assert_eq!(error.status, ErrorStatus::Pending);
    }

    #[test]
    fn test_error_status() {
        assert!(ErrorStatus::Pending != ErrorStatus::Resolved);
    }

    #[test]
    fn test_error_handler_config() {
        let config = ErrorHandlerConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            retention_period: Duration::from_secs(3600),
        };
        
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    #[tokio::test]
    async fn test_basic_error_handler() -> Result<()> {
        let handler = BasicErrorHandler::default();
        
        let error = BlockchainError {
            error_id: "TEST_ERROR_1".to_string(),
            name: "TEST_ERROR".to_string(),
            description: "Test error".to_string(),
            status: ErrorStatus::Pending,
            transaction_error: Some("Test transaction error".to_string()),
            recovery_info: Some("{}".to_string()),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };
        
        handler.add_error(error.clone()).await?;
        
        let retrieved = handler.get_error("TEST_ERROR_1").await?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "TEST_ERROR");
        
        handler.remove_error("TEST_ERROR_1").await?;
        
        let retrieved = handler.get_error("TEST_ERROR_1").await?;
        assert!(retrieved.is_none());
        
        Ok(())
    }

    #[test]
    fn test_transaction_error() {
        let error = TransactionError::InsufficientGas("Gas không đủ".to_string());
        assert_eq!(error.to_string(), "Lỗi gas không đủ: Gas không đủ");
    }

    #[test]
    fn test_classify_blockchain_error() {
        let error = classify_blockchain_error("Gas too low");
        assert!(matches!(error, TransactionError::InsufficientGas(_)));
        
        let error = classify_blockchain_error("Insufficient liquidity");
        assert!(matches!(error, TransactionError::LowLiquidity(_)));
    }

    #[test]
    fn test_get_recovery_info() {
        let error = TransactionError::InsufficientGas("Gas không đủ".to_string());
        let recovery = get_recovery_info(&error);
        
        assert!(recovery.should_retry);
        assert!(recovery.increase_gas);
        assert_eq!(recovery.increase_gas_percent, 30);
    }

    #[tokio::test]
    async fn test_handle_transaction_error() -> Result<()> {
        let handler = BasicErrorHandler::default();
        
        let recovery = handle_transaction_error(&handler, "Gas too low").await?;
        
        assert!(recovery.should_retry);
        assert!(recovery.increase_gas);
        
        let errors = handler.get_errors().await?;
        assert_eq!(errors.len(), 1);
        
        Ok(())
    }
}

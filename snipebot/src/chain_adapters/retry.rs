use anyhow::{Result, anyhow};
use std::time::Duration;
use tracing::{info, warn, debug};
use backoff::ExponentialBackoff;

use crate::chain_adapters::retry_policy::{RetryContext, create_default_retry_policy};
use crate::chain_adapters::connection_pool::{get_or_create_pool, ProviderGuard};
use crate::chain_adapters::interfaces::ChainError;

/// Enum liệt kê các loại lỗi phổ biến của blockchain
#[derive(Debug, Clone)]
pub enum BlockchainError {
    /// Lỗi kết nối RPC
    RPCConnection(String),
    
    /// Lỗi transaction bị từ chối
    TransactionRejected(String),
    
    /// Lỗi gas quá thấp
    LowGas(String),
    
    /// Lỗi nonce không hợp lệ
    InvalidNonce(String),
    
    /// Lỗi thiếu balance
    InsufficientBalance(String),
    
    /// Lỗi contract (reverted)
    ContractError(String),
    
    /// Lỗi timeout
    Timeout(String),
    
    /// Lỗi khác
    Other(String),
}

impl From<ethers::providers::ProviderError> for BlockchainError {
    fn from(err: ethers::providers::ProviderError) -> Self {
        match err {
            ProviderError::JsonRpcClientError(e) => {
                if e.to_string().contains("insufficient funds") {
                    BlockchainError::InsufficientBalance(e.to_string())
                } else if e.to_string().contains("gas required exceeds allowance") {
                    BlockchainError::InsufficientBalance(e.to_string())
                } else if e.to_string().contains("nonce") {
                    BlockchainError::InvalidNonce(e.to_string())
                } else if e.to_string().contains("underpriced") || e.to_string().contains("gas price") {
                    BlockchainError::LowGas(e.to_string())
                } else if e.to_string().contains("execution reverted") {
                    BlockchainError::ContractError(e.to_string())
                } else if e.to_string().contains("connection") || e.to_string().contains("timeout") || e.to_string().contains("rate limit") {
                    BlockchainError::RPCConnection(e.to_string())
                } else {
                    BlockchainError::Other(e.to_string())
                }
            },
            ProviderError::EnsError(_) => BlockchainError::Other(format!("ENS error: {}", err)),
            ProviderError::SerdeJson(e) => BlockchainError::Other(format!("JSON error: {}", e)),
            ProviderError::HexError(e) => BlockchainError::Other(format!("Hex error: {}", e)),
            _ => BlockchainError::Other(format!("Unknown provider error: {}", err)),
        }
    }
}

/// Kiểm tra xem lỗi có nên thử lại không
pub fn is_retryable(error: &BlockchainError) -> bool {
    match error {
        BlockchainError::RPCConnection(_) => true,
        BlockchainError::LowGas(_) => true,
        BlockchainError::InvalidNonce(_) => true,
        BlockchainError::Timeout(_) => true,
        _ => false,
    }
}

/// Tăng gas price dựa trên lỗi
pub fn adjust_gas_for_retry(error: &BlockchainError, current_gas_price: U256) -> U256 {
    match error {
        BlockchainError::LowGas(_) => {
            // Tăng gas price thêm 20%
            current_gas_price + (current_gas_price * U256::from(20) / U256::from(100))
        },
        _ => current_gas_price,
    }
}

/// Cấu hình backoff
pub fn create_backoff() -> ExponentialBackoff {
    let mut backoff = ExponentialBackoff::default();
    backoff.initial_interval = Duration::from_millis(500);
    backoff.max_interval = Duration::from_secs(30);
    backoff.multiplier = 2.0;
    backoff.max_elapsed_time = Some(Duration::from_secs(300)); // 5 phút
    backoff
}

/// Thực hiện hàm async với retry và backoff
pub async fn retry_async<F, Fut, T, E>(f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
    E: Into<anyhow::Error> + Debug,
{
    let mut backoff = create_backoff();
    let operation = || async {
        match f().await {
            Ok(v) => Ok(v),
            Err(e) => {
                let error = e.into();
                debug!("Lỗi: {:?}, thử lại sau backoff", error);
                Err(backoff::Error::transient(error))
            }
        }
    };
    
    backoff::future::retry(backoff, operation).await
        .map_err(|e| anyhow!("Quá số lần thử: {}", e))
}

/// Thực hiện hàm async với retry thông minh dựa trên loại lỗi blockchain
pub async fn retry_blockchain_operation<F, Fut, T>(
    operation_name: &str, 
    f: F,
    initial_gas_price: Option<U256>,
    max_retries: usize,
) -> Result<T>
where
    F: Fn(Option<U256>) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut current_gas_price = initial_gas_price;
    let mut retries = 0;
    let max_retries = if max_retries == 0 { 3 } else { max_retries };
    
    loop {
        match f(current_gas_price).await {
            Ok(result) => {
                if retries > 0 {
                    info!("Thành công sau {} lần thử lại cho operation '{}'", retries, operation_name);
                }
                return Ok(result);
            },
            Err(e) => {
                let error_str = e.to_string();
                let blockchain_error = if error_str.contains("insufficient funds") {
                    BlockchainError::InsufficientBalance(error_str)
                } else if error_str.contains("gas required exceeds allowance") {
                    BlockchainError::InsufficientBalance(error_str)
                } else if error_str.contains("nonce") {
                    BlockchainError::InvalidNonce(error_str)
                } else if error_str.contains("underpriced") || error_str.contains("gas price") {
                    BlockchainError::LowGas(error_str)
                } else if error_str.contains("execution reverted") {
                    BlockchainError::ContractError(error_str)
                } else if error_str.contains("connection") || error_str.contains("timeout") || error_str.contains("rate limit") {
                    BlockchainError::RPCConnection(error_str)
                } else {
                    BlockchainError::Other(error_str)
                };
                
                retries += 1;
                if retries >= max_retries || !is_retryable(&blockchain_error) {
                    warn!("Không thể thực hiện '{}' sau {} lần thử: {:?}", 
                        operation_name, retries, blockchain_error);
                    return Err(anyhow!("Lỗi khi thực hiện '{}': {:?}", operation_name, blockchain_error));
                }
                
                // Điều chỉnh gas price nếu cần
                if let Some(gas) = current_gas_price {
                    current_gas_price = Some(adjust_gas_for_retry(&blockchain_error, gas));
                    info!("Tăng gas price lên {} gwei cho lần thử tiếp theo", 
                        current_gas_price.unwrap() / U256::exp10(9));
                }
                
                // Tính thời gian đợi với backoff strategy
                let wait_time = ((retries as u64).pow(2) * 500).min(30000); // 0.5s, 2s, 4.5s, 8s, ...
                info!("Thử lại '{}' lần {} sau {}ms: {:?}", 
                    operation_name, retries, wait_time, blockchain_error);
                    
                tokio::time::sleep(Duration::from_millis(wait_time)).await;
            }
        }
    }
}

/// Thực hiện RPC call với rotation URL nếu gặp lỗi kết nối
pub async fn with_rpc_rotation<F, Fut, T>(
    primary_url: &str, 
    backup_urls: &[&str], 
    f: F
) -> Result<T> 
where
    F: Fn(&str) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let primary_result = f(primary_url).await;
    
    if primary_result.is_ok() {
        return primary_result;
    }
    
    let primary_error = primary_result.unwrap_err();
    
    // Chỉ rotate RPC URL nếu lỗi là lỗi kết nối
    if !primary_error.to_string().contains("connection") 
        && !primary_error.to_string().contains("timeout")
        && !primary_error.to_string().contains("rate limit") {
        return Err(primary_error);
    }
    
    warn!("Lỗi kết nối đến RPC chính {}: {}", primary_url, primary_error);
    
    // Thử với các backup URLs
    for (i, backup_url) in backup_urls.iter().enumerate() {
        info!("Thử kết nối đến RPC backup {}: {}", i + 1, backup_url);
        match f(backup_url).await {
            Ok(result) => {
                info!("Thành công với RPC backup {}", backup_url);
                return Ok(result);
            },
            Err(e) => {
                warn!("Lỗi kết nối đến RPC backup {}: {}", backup_url, e);
            }
        }
    }
    
    // Nếu tất cả đều thất bại, trả về lỗi ban đầu
    Err(primary_error)
}

/// Hàm kiểm tra tính khả dụng của các URL RPC
pub async fn check_rpc_availability(urls: &[&str]) -> Vec<(String, bool, u128)> {
    let mut results = Vec::new();
    
    for url in urls {
        let start_time = std::time::Instant::now();
        let is_available = match ethers::providers::Provider::<ethers::providers::Http>::try_from(*url) {
            Ok(provider) => {
                match provider.get_block_number().await {
                    Ok(_) => true,
                    Err(_) => false,
                }
            },
            Err(_) => false,
        };
        
        let latency = start_time.elapsed().as_millis();
        results.push((url.to_string(), is_available, latency));
    }
    
    // Sắp xếp theo tính khả dụng và độ trễ
    results.sort_by(|a, b| {
        match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.2.cmp(&b.2),
        }
    });
    
    results
}

/// Pool các URL RPC tối ưu
pub struct RPCPool {
    /// Danh sách URLs của RPC, kèm theo trọng số
    urls: Vec<(String, u32)>,
    
    /// Thời điểm kiểm tra lần cuối
    last_checked: std::sync::RwLock<std::time::Instant>,
    
    /// Kết quả kiểm tra lần cuối
    availability: std::sync::RwLock<Vec<(String, bool, u128)>>,
}

impl RPCPool {
    /// Tạo pool mới với danh sách URLs
    pub fn new(urls: Vec<(String, u32)>) -> Self {
        Self {
            urls,
            last_checked: std::sync::RwLock::new(std::time::Instant::now() - Duration::from_secs(3600)),
            availability: std::sync::RwLock::new(Vec::new()),
        }
    }
    
    /// Lấy URL RPC tốt nhất
    pub async fn get_best_url(&self) -> Result<String> {
        // Kiểm tra lại availability nếu đã quá 10 phút
        {
            let last_checked = self.last_checked.read().unwrap();
            if last_checked.elapsed() > Duration::from_secs(600) {
                drop(last_checked);
                self.check_availability().await?;
            }
        }
        
        // Lấy URL đầu tiên có sẵn
        let availability = self.availability.read().unwrap();
        for (url, is_available, _) in &*availability {
            if *is_available {
                return Ok(url.clone());
            }
        }
        
        // Nếu không có URL nào khả dụng, trả về URL đầu tiên
        if !self.urls.is_empty() {
            return Ok(self.urls[0].0.clone());
        }
        
        Err(anyhow!("Không có URL RPC nào khả dụng"))
    }
    
    /// Kiểm tra tính khả dụng của các URLs
    pub async fn check_availability(&self) -> Result<()> {
        // Lấy danh sách URLs
        let urls: Vec<&str> = self.urls.iter().map(|(url, _)| url.as_str()).collect();
        
        // Kiểm tra
        let results = check_rpc_availability(&urls).await;
        
        // Cập nhật kết quả
        {
            let mut availability = self.availability.write().unwrap();
            *availability = results;
        }
        
        // Cập nhật thời điểm kiểm tra
        {
            let mut last_checked = self.last_checked.write().unwrap();
            *last_checked = std::time::Instant::now();
        }
        
        Ok(())
    }
    
    /// Đánh dấu URL không khả dụng
    pub fn mark_unavailable(&self, url: &str) {
        let mut availability = self.availability.write().unwrap();
        
        for item in &mut *availability {
            if item.0 == url {
                item.1 = false;
                break;
            }
        }
    }
    
    /// Lấy tất cả URLs
    pub fn all_urls(&self) -> Vec<String> {
        self.urls.iter().map(|(url, _)| url.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_retry_async() {
        let mut attempts = 0;
        
        let result = retry_async(|| async {
            attempts += 1;
            if attempts < 3 {
                Err::<(), _>(anyhow!("Lỗi thử nghiệm"))
            } else {
                Ok(())
            }
        }).await;
        
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
    }
    
    #[tokio::test]
    async fn test_retry_blockchain_operation() {
        let mut attempts = 0;
        
        let result = retry_blockchain_operation(
            "test_operation",
            |gas_price| async move {
                attempts += 1;
                if attempts < 3 {
                    if attempts == 1 {
                        Err(anyhow!("underpriced transaction"))
                    } else {
                        Err(anyhow!("connection error"))
                    }
                } else {
                    Ok(())
                }
            },
            Some(U256::from(1000000000)), // 1 gwei
            5
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
    }
} 
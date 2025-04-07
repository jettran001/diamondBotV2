use reqwest::{Client, Error as ReqwestError};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use std::time::Duration;
use ethers::types::{Address, U256, H256, Bytes};
use log::{info, warn, debug};
use url::Url;

/// Định nghĩa các lỗi liên quan đến FlashBots
#[derive(Debug, thiserror::Error)]
pub enum FlashbotsError {
    #[error("Lỗi mạng: {0}")]
    NetworkError(String),
    
    #[error("Lỗi khi gửi bundle: {0}")]
    SendError(String),
    
    #[error("Lỗi phân tích phản hồi: {0}")]
    ParseError(String),
    
    #[error("Lỗi HTTP request: {0}")]
    RequestError(String),
    
    #[error("Lỗi timeout: {0}")]
    TimeoutError(String),
    
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl From<ReqwestError> for FlashbotsError {
    fn from(err: ReqwestError) -> Self {
        FlashbotsError::RequestError(err.to_string())
    }
}

impl From<serde_json::Error> for FlashbotsError {
    fn from(err: serde_json::Error) -> Self {
        FlashbotsError::ParseError(err.to_string())
    }
}

/// Cấu trúc biểu diễn một bundle FlashBots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundle {
    pub transactions: Vec<Bytes>,
    pub target_block: u64,
    pub min_block: u64,
    pub max_block: u64,
    pub revert_on_fail: bool,
}

/// Cấu trúc thông tin về bundle FlashBots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundleStats {
    pub bundle_hash: String,
    pub simulated: bool,
    pub simulation_success: bool,
    pub simulation_error: Option<String>,
    pub included_block: Option<u64>,
}

/// Gửi bundle lên FlashBots relay
pub async fn send_bundle(&self, bundle: &FlashbotsBundle) -> Result<String, FlashbotsError> {
    let mut attempts = 0;
    let max_attempts = 3;
    let timeout_duration = std::time::Duration::from_secs(1);
    
    loop {
        attempts += 1;
        
        let send_result = tokio::time::timeout(
            timeout_duration,
            self._send_bundle_internal(bundle)
        ).await;
        
        match send_result {
            Ok(result) => return result,
            Err(_) => {
                warn!("Timeout khi gửi bundle FlashBots (attempt {}/{})", attempts, max_attempts);
                if attempts >= max_attempts {
                    return Err(FlashbotsError::NetworkError("Đã hết số lần thử gửi bundle FlashBots".into()));
                }
                // Chờ một khoảng thời gian ngắn trước khi thử lại
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Lấy thông tin về một bundle đã gửi
pub async fn get_bundle_stats(&self, bundle_hash: &str) -> Result<FlashbotsBundleStats, FlashbotsError> {
    let mut attempts = 0;
    let max_attempts = 3;
    let timeout_duration = std::time::Duration::from_secs(1);
    
    loop {
        attempts += 1;
        
        let stats_result = tokio::time::timeout(
            timeout_duration,
            self._get_bundle_stats_internal(bundle_hash)
        ).await;
        
        match stats_result {
            Ok(result) => return result,
            Err(_) => {
                warn!("Timeout khi lấy thông tin bundle (attempt {}/{})", attempts, max_attempts);
                if attempts >= max_attempts {
                    return Err(FlashbotsError::NetworkError("Đã hết số lần thử lấy thông tin bundle".into()));
                }
                // Chờ một khoảng thời gian ngắn trước khi thử lại
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Phương thức nội bộ để gửi bundle
async fn _send_bundle_internal(&self, bundle: &FlashbotsBundle) -> Result<String, FlashbotsError> {
    // Chuẩn bị request
    let payload = self.prepare_bundle_payload(bundle)?;
    
    // Thực hiện gửi request
    let result = self.client.post(&self.relay_endpoint).json(&payload).send().await;
    
    match result {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_else(|_| "Không thể đọc lỗi".to_string());
                return Err(FlashbotsError::SendError(format!("Status {}: {}", status, error_text)));
            }
            
            // Parse response
            let bundle_response: serde_json::Value = response.json().await?;
            
            // Extract bundle hash
            bundle_response["bundleHash"].as_str()
                .ok_or_else(|| FlashbotsError::ParseError("Không tìm thấy bundleHash".to_string()))
                .map(|s| s.to_string())
        },
        Err(e) => Err(FlashbotsError::RequestError(format!("Lỗi HTTP request: {}", e))),
    }
}

/// Phương thức nội bộ để lấy thông tin bundle
async fn _get_bundle_stats_internal(&self, bundle_hash: &str) -> Result<FlashbotsBundleStats, FlashbotsError> {
    // Tạo URL
    let stats_url = format!("{}/bundle-stats", self.relay_endpoint);
    
    // Chuẩn bị query parameters
    let params = [("bundleHash", bundle_hash)];
    
    // Thực hiện request
    let result = self.client.get(&stats_url).query(&params).send().await;
    
    match result {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_else(|_| "Không thể đọc lỗi".to_string());
                return Err(FlashbotsError::RequestError(format!("Status {}: {}", status, error_text)));
            }
            
            // Parse response
            let stats: serde_json::Value = response.json().await?;
            
            // Convert to stats struct
            self.parse_bundle_stats(&stats)
        },
        Err(e) => Err(FlashbotsError::RequestError(format!("Lỗi HTTP request: {}", e))),
    }
} 
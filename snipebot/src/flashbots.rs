// External imports
use ethers::{
    types::{Address, U256, H256, Bytes, TransactionRequest},
    signers::{LocalWallet, Signer},
    middleware::SignerMiddleware,
    providers::Provider,
};

// Standard library imports
use std::{
    time::Duration,
    sync::Arc,
    str::FromStr,
};

// Third party imports
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::{Client, Error as ReqwestError};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, warn, debug, error};
use tokio::time::timeout;
use url::Url;

/// Định nghĩa các lỗi liên quan đến FlashBots
#[derive(Debug, Error)]
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
    /// Danh sách các giao dịch đã ký
    pub transactions: Vec<Bytes>,
    /// Block mục tiêu mong muốn
    pub target_block: u64,
    /// Block tối thiểu (bắt đầu)
    pub min_block: u64,
    /// Block tối đa (kết thúc)
    pub max_block: u64,
    /// Phản hồi nếu gặp lỗi
    pub revert_on_fail: bool,
}

/// Cấu trúc thông tin về bundle FlashBots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundleStats {
    /// Hash của bundle
    pub bundle_hash: String,
    /// Đã mô phỏng hay chưa
    pub simulated: bool,
    /// Mô phỏng thành công
    pub simulation_success: bool,
    /// Lỗi mô phỏng (nếu có)
    pub simulation_error: Option<String>,
    /// Block chứa bundle (nếu đã được khai thác)
    pub included_block: Option<u64>,
}

/// Cấu trúc kết quả của việc gửi bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundleResponse {
    /// Hash của bundle
    pub bundle_hash: String,
}

/// Cấu hình cho FlashBots provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsConfig {
    /// URL của relay endpoint
    pub relay_endpoint: String,
    /// Private key của ETH address để ký các bundle
    pub flashbots_key: String,
    /// Số lần thử lại tối đa khi gửi bundle
    pub max_attempts: u32,
    /// Thời gian chờ tối đa cho mỗi request (giây)
    pub timeout_seconds: u64,
}

impl Default for FlashbotsConfig {
    fn default() -> Self {
        Self {
            relay_endpoint: "https://relay.flashbots.net".to_string(),
            flashbots_key: "".to_string(),
            max_attempts: 3,
            timeout_seconds: 1,
        }
    }
}

/// Định nghĩa trait cho FlashBots provider
#[async_trait]
pub trait FlashbotsBundleProvider: Send + Sync {
    /// Gửi bundle lên FlashBots relay
    async fn submit_flashbots_bundle(&self, transactions: Vec<TransactionRequest>) -> Result<H256, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Kiểm tra trạng thái của bundle đã gửi
    async fn check_bundle_status(&self, bundle_hash: &H256) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;
}

/// FlashBots provider
pub struct FlashbotsProvider {
    /// HTTP client
    client: Client,
    /// Cấu hình FlashBots
    config: FlashbotsConfig,
    /// Endpoint của relay
    relay_endpoint: String,
    /// Wallet để ký các bundle
    flashbots_signer: LocalWallet,
}

impl FlashbotsProvider {
    /// Tạo FlashbotsProvider mới
    pub fn new(config: FlashbotsConfig) -> Result<Self> {
        // Tạo HTTP client
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|e| anyhow!("Không thể tạo HTTP client: {}", e))?;
            
        // Tạo wallet từ private key
        let flashbots_signer = LocalWallet::from_str(&config.flashbots_key)
            .map_err(|e| anyhow!("Private key không hợp lệ: {}", e))?;
            
        Ok(Self {
            client,
            config: config.clone(),
            relay_endpoint: config.relay_endpoint,
            flashbots_signer,
        })
    }
    
    /// Chuẩn bị payload cho bundle
    fn prepare_bundle_payload(&self, bundle: &FlashbotsBundle) -> Result<serde_json::Value, FlashbotsError> {
        let payload = serde_json::to_value(bundle)
            .map_err(|e| FlashbotsError::ParseError(format!("Lỗi khi chuyển đổi bundle thành JSON: {}", e)))?;
            
        Ok(payload)
    }
    
    /// Phân tích kết quả stats từ JSON
    fn parse_bundle_stats(&self, stats: &serde_json::Value) -> Result<FlashbotsBundleStats, FlashbotsError> {
        let bundle_hash = stats["bundleHash"].as_str()
            .ok_or_else(|| FlashbotsError::ParseError("Không tìm thấy bundleHash".to_string()))?;
            
        let simulated = stats["simulated"].as_bool().unwrap_or(false);
        let simulation_success = stats["simulationSuccess"].as_bool().unwrap_or(false);
        let simulation_error = stats["simulationError"].as_str().map(|s| s.to_string());
        let included_block = stats["includedBlock"].as_u64();
        
        Ok(FlashbotsBundleStats {
            bundle_hash: bundle_hash.to_string(),
            simulated,
            simulation_success,
            simulation_error,
            included_block,
        })
    }

    /// Chuyển đổi TransactionRequest sang Bytes đã ký
    async fn prepare_signed_transactions(&self, transactions: Vec<TransactionRequest>) -> Result<Vec<Bytes>, FlashbotsError> {
        let mut signed_txs = Vec::with_capacity(transactions.len());
        
        for tx in transactions {
            // Chuyển thành typed transaction
            let typed_tx = tx.into();
            
            // Ký giao dịch với wallet
            let signature = self.flashbots_signer.sign_transaction(&typed_tx).await
                .map_err(|e| FlashbotsError::Other(anyhow!("Lỗi khi ký giao dịch: {}", e)))?;
                
            // Kết hợp giao dịch và chữ ký
            let signed_tx = typed_tx.rlp_signed(&signature);
            
            signed_txs.push(signed_tx);
        }
        
        Ok(signed_txs)
    }
    
    /// Gửi bundle lên FlashBots relay
    pub async fn send_bundle(&self, bundle: &FlashbotsBundle) -> Result<String, FlashbotsError> {
        let mut attempts = 0;
        let max_attempts = self.config.max_attempts;
        let timeout_duration = Duration::from_secs(self.config.timeout_seconds);
        
        loop {
            attempts += 1;
            
            let send_result = timeout(
                timeout_duration,
                self._send_bundle_internal(bundle)
            ).await;
            
            match send_result {
                Ok(result) => return result,
                Err(_) => {
                    warn!("Timeout khi gửi bundle FlashBots (attempt {}/{})", attempts, max_attempts);
                    if attempts >= max_attempts as usize {
                        return Err(FlashbotsError::NetworkError("Đã hết số lần thử gửi bundle FlashBots".into()));
                    }
                    // Chờ một khoảng thời gian ngắn trước khi thử lại
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
    
    /// Tạo bundle từ danh sách giao dịch
    pub async fn create_bundle_from_transactions(&self, transactions: Vec<TransactionRequest>, target_block: u64) -> Result<FlashbotsBundle, FlashbotsError> {
        // Chuẩn bị các giao dịch đã ký
        let signed_txs = self.prepare_signed_transactions(transactions).await?;
        
        // Tạo bundle
        let bundle = FlashbotsBundle {
            transactions: signed_txs,
            target_block,
            min_block: target_block,
            max_block: target_block + 2, // Thường thử 3 block liên tiếp
            revert_on_fail: true,
        };
        
        Ok(bundle)
    }

    /// Lấy thông tin về một bundle đã gửi
    pub async fn get_bundle_stats(&self, bundle_hash: &str) -> Result<FlashbotsBundleStats, FlashbotsError> {
        let mut attempts = 0;
        let max_attempts = self.config.max_attempts;
        let timeout_duration = Duration::from_secs(self.config.timeout_seconds);
        
        loop {
            attempts += 1;
            
            let stats_result = timeout(
                timeout_duration,
                self._get_bundle_stats_internal(bundle_hash)
            ).await;
            
            match stats_result {
                Ok(result) => return result,
                Err(_) => {
                    warn!("Timeout khi lấy thông tin bundle (attempt {}/{})", attempts, max_attempts);
                    if attempts >= max_attempts as usize {
                        return Err(FlashbotsError::NetworkError("Đã hết số lần thử lấy thông tin bundle".into()));
                    }
                    // Chờ một khoảng thời gian ngắn trước khi thử lại
                    tokio::time::sleep(Duration::from_millis(100)).await;
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
}

// Triển khai FlashbotsBundleProvider trait
#[async_trait]
impl FlashbotsBundleProvider for FlashbotsProvider {
    async fn submit_flashbots_bundle(&self, transactions: Vec<TransactionRequest>) -> Result<H256, Box<dyn std::error::Error + Send + Sync>> {
        // Lấy block hiện tại + 1 làm block mục tiêu
        let target_block = 0; // Trong ứng dụng thực tế, lấy này từ RPC provider
        
        // Tạo bundle từ danh sách giao dịch
        let bundle = self.create_bundle_from_transactions(transactions, target_block).await?;
        
        // Gửi bundle
        let bundle_hash = self.send_bundle(&bundle).await?;
        
        // Chuyển đổi string hash thành H256
        let hash = H256::from_str(&bundle_hash)
            .map_err(|e| FlashbotsError::ParseError(format!("Bundle hash không hợp lệ: {}", e)))?;
            
        Ok(hash)
    }
    
    async fn check_bundle_status(&self, bundle_hash: &H256) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Chuyển đổi H256 thành string
        let hash_str = format!("{:?}", bundle_hash);
        
        // Lấy thông tin của bundle
        let stats = self.get_bundle_stats(&hash_str).await?;
        
        // Kiểm tra xem bundle đã được đưa vào block chưa
        Ok(stats.included_block.is_some())
    }
} 
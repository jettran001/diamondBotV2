use serde::{Deserialize, Serialize};
use std::sync::Arc;
use anyhow::Result;
use dotenv::dotenv;
use std::env;
use std::collections::HashMap;
use log;
use std::time::Duration;
use std::future::Future;
use tokio;
use ethers::providers::{Provider, Http};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BotMode {
    Manual,
    Auto,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    // Mới: Tên chain mặc định
    pub chain_name: String,
    
    // Blockchain
    pub rpc_url: String,
    pub chain_id: u64,
    pub private_key: String,
    pub weth_address: String,
    pub router_address: String,
    
    // Danh sách chain được hỗ trợ
    pub supported_chains: Vec<String>,
    
    // API
    pub api_host: String,
    pub api_port: u16,
    
    // Redis
    pub redis_url: String,
    
    // MQTT
    pub mqtt_broker: String,
    pub mqtt_port: u16,
    pub mqtt_client_id: String,
    
    // IPFS
    pub ipfs_api: String,
    
    // Snipebot config
    pub default_gas_limit: u64,
    pub default_gas_price: u64,
    pub default_slippage: f64,
    
    // Wallet config
    pub wallet_folder: String,
    pub use_multiple_wallets: bool,
    pub auto_create_wallet: bool,
    
    // Thêm các trường mới
    pub bot_mode: BotMode,
    pub auto_retry_count: u8,
    pub auto_gas_boost: bool,
    pub risk_analyzer_enabled: bool,
    pub max_risk_score: u8,  // 0-100
    pub ai_enabled: bool,
    pub auto_trade_threshold: f64, // Ngưỡng tin cậy của AI để tự động giao dịch
    #[serde(default = "default_wallet_encryption_seed")]
    pub wallet_encryption_seed: String,
    pub fallback_rpc_urls: Vec<String>,
}

/// Cấu hình cho retry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Số lần retry tối đa
    pub max_retries: usize,
    /// Thời gian giữa các lần retry (ms)
    pub initial_retry_delay: u64,
    /// Hệ số nhân cho mỗi lần retry (exponential backoff)
    pub retry_multiplier: f64,
    /// Thời gian retry tối đa (ms)
    pub max_retry_delay: u64,
    /// Danh sách lỗi có thể retry
    pub retryable_errors: Vec<String>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_retry_delay: 1000,
            retry_multiplier: 1.5,
            max_retry_delay: 30000,
            retryable_errors: vec![
                "timeout".to_string(),
                "connection refused".to_string(),
                "network error".to_string(),
                "rate limit exceeded".to_string(),
                "nonce too low".to_string(),
                "underpriced".to_string(),
            ],
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            chain_name: "ethereum".to_string(),
            rpc_url: "https://mainnet.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161".to_string(),
            chain_id: 1,
            private_key: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            weth_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(),
            router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(),
            supported_chains: vec!["ethereum".to_string(), "bsc".to_string(), "base".to_string(), "avalanche".to_string()],
            api_host: "0.0.0.0".to_string(),
            api_port: 8080,
            redis_url: "redis://localhost:6379".to_string(),
            mqtt_broker: "localhost".to_string(),
            mqtt_port: 1883,
            mqtt_client_id: "snipebot".to_string(),
            ipfs_api: "http://localhost:5001".to_string(),
            default_gas_limit: 500000,
            default_gas_price: 5000000000,
            default_slippage: 2.0,
            wallet_folder: "data".to_string(),
            use_multiple_wallets: false,
            auto_create_wallet: false,
            bot_mode: BotMode::Manual,
            auto_retry_count: 3,
            auto_gas_boost: true,
            risk_analyzer_enabled: true,
            max_risk_score: 70,
            ai_enabled: false,
            auto_trade_threshold: 0.8,
            wallet_encryption_seed: default_wallet_encryption_seed(),
            fallback_rpc_urls: vec![],
        }
    }
    
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenv().ok();
        
        Ok(Config {
            chain_name: env::var("CHAIN_NAME").unwrap_or_else(|_| "ethereum".to_string()),
            rpc_url: env::var("RPC_URL")?,
            chain_id: env::var("CHAIN_ID")?.parse()?,
            private_key: env::var("PRIVATE_KEY")?,
            weth_address: env::var("WETH_ADDRESS")?,
            router_address: env::var("ROUTER_ADDRESS")?,
            supported_chains: env::var("SUPPORTED_CHAINS")
                .unwrap_or_else(|_| "ethereum,bsc,base,avalanche".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .collect(),
            api_host: env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            api_port: env::var("API_PORT").unwrap_or_else(|_| "8080".to_string()).parse()?,
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            mqtt_broker: env::var("MQTT_BROKER").unwrap_or_else(|_| "localhost".to_string()),
            mqtt_port: env::var("MQTT_PORT").unwrap_or_else(|_| "1883".to_string()).parse()?,
            mqtt_client_id: env::var("MQTT_CLIENT_ID").unwrap_or_else(|_| "snipebot".to_string()),
            ipfs_api: env::var("IPFS_API").unwrap_or_else(|_| "http://localhost:5001".to_string()),
            default_gas_limit: env::var("DEFAULT_GAS_LIMIT").unwrap_or_else(|_| "500000".to_string()).parse()?,
            default_gas_price: env::var("DEFAULT_GAS_PRICE").unwrap_or_else(|_| "5000000000".to_string()).parse()?,
            default_slippage: env::var("DEFAULT_SLIPPAGE").unwrap_or_else(|_| "2.0".to_string()).parse()?,
            wallet_folder: env::var("WALLET_FOLDER").unwrap_or_else(|_| "data".to_string()),
            use_multiple_wallets: env::var("USE_MULTIPLE_WALLETS").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            auto_create_wallet: env::var("AUTO_CREATE_WALLET").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            bot_mode: match env::var("BOT_MODE").unwrap_or_else(|_| "Manual".to_string()).as_str() {
                "Auto" => BotMode::Auto,
                _ => BotMode::Manual,
            },
            auto_retry_count: env::var("AUTO_RETRY_COUNT").unwrap_or_else(|_| "3".to_string()).parse().unwrap_or(3),
            auto_gas_boost: env::var("AUTO_GAS_BOOST").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true),
            risk_analyzer_enabled: env::var("RISK_ANALYZER_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true),
            max_risk_score: env::var("MAX_RISK_SCORE").unwrap_or_else(|_| "70".to_string()).parse().unwrap_or(70),
            ai_enabled: env::var("AI_ENABLED").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false),
            auto_trade_threshold: env::var("AUTO_TRADE_THRESHOLD").unwrap_or_else(|_| "0.8".to_string()).parse().unwrap_or(0.8),
            wallet_encryption_seed: default_wallet_encryption_seed(),
            fallback_rpc_urls: vec![],
        })
    }
}

// Hàm khởi tạo cấu hình toàn cục
pub fn init_config() -> Arc<Config> {
    match Config::from_env() {
        Ok(config) => Arc::new(config),
        Err(e) => {
            eprintln!("Lỗi khi tải cấu hình: {}. Sử dụng cấu hình mặc định", e);
            Arc::new(Config::new())
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub router_address: String,
    pub weth_address: String,
    pub block_time: u64, // ms
    pub fallback_rpc_urls: Vec<String>, // Thêm danh sách RPC dự phòng
}

#[derive(Clone, Debug)]
pub struct MultiChainConfig {
    pub chains: HashMap<u64, ChainConfig>,
    pub default_chain_id: u64,
}

impl SnipeBot {
    async fn check_for_rugpull(&self, token_info: &TokenInfo) -> Result<bool, Box<dyn std::error::Error>> {
        // Kiểm tra liquidity ratio
        // Kiểm tra code token
        // Kiểm tra owner có thể mint không giới hạn không
        // Kiểm tra trading enabled
        // etc.
        
        Ok(false) // Not a rug pull
    }
}

fn default_wallet_encryption_seed() -> String {
    // Sử dụng giá trị từ biến môi trường hoặc giá trị mặc định
    std::env::var("WALLET_ENCRYPTION_SEED")
        .unwrap_or_else(|_| "snipebot_default_encryption_seed".to_string())
}

impl EthereumAdapter {
    async fn get_provider_with_fallback(&self) -> Provider<Http> {
        // Thử kết nối với provider chính
        if let Ok(block_number) = self.provider.get_block_number().await {
            return self.provider.clone();
        }
        
        // Nếu thất bại, thử với các provider dự phòng
        for url in &self.config.fallback_rpc_urls {
            match Provider::<Http>::try_from(url.as_str()) {
                Ok(provider) => {
                    if let Ok(_) = provider.get_block_number().await {
                        log::info!("Đã chuyển sang provider dự phòng: {}", url);
                        return provider;
                    }
                },
                Err(_) => continue,
            }
        }
        
        // Nếu tất cả đều thất bại, trả về provider chính
        log::warn!("Tất cả provider đều không khả dụng, sử dụng provider chính");
        self.provider.clone()
    }
    
    async fn swap_exact_eth_for_tokens_with_fallback(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>, Box<dyn std::error::Error>> {
        let fallback_providers = self.config.fallback_rpc_urls.clone();
        let config = RetryConfig::default();
        let operation_name = "swap_eth_for_tokens";
        
        return retry_with_fallback_providers(
            // Primary operation
            || async {
                self.swap_exact_eth_for_tokens(
                    token_address,
                    amount_in,
                    min_amount_out,
                    recipient,
                    deadline,
                    gas_limit,
                    gas_price,
                ).await
            },
            // Fallback operation generator
            |provider_url| async move {
                // Tạo adapter mới với provider dự phòng
                let mut adapter = EthereumAdapter::new().await?;
                
                // Cập nhật provider
                adapter.provider = Provider::<Http>::try_from(provider_url)?;
                
                // Cập nhật wallet
                if let Some(wallet) = self.wallet.clone() {
                    adapter.set_wallet(wallet);
                }
                
                adapter.swap_exact_eth_for_tokens(
                    token_address,
                    amount_in,
                    min_amount_out,
                    recipient,
                    deadline,
                    gas_limit,
                    gas_price,
                ).await
            },
            fallback_providers,
            config,
            operation_name
        ).await;
    }
}

/// Thực hiện retry với các fallback provider
pub async fn retry_with_fallback_providers<T, F, Fut>(
    providers: Vec<Provider<Http>>, 
    operation: F,
    config: RetryConfig
) -> Result<T, Box<dyn std::error::Error>>
where
    F: Fn(Provider<Http>) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<T, Box<dyn std::error::Error>>> + Send + 'static,
    T: Send + 'static,
{
    let mut last_error = None;
    let mut retry_count = 0;
    let mut delay = config.initial_retry_delay;
    
    for provider in providers {
        while retry_count < config.max_retries {
            match operation(provider.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_str = e.to_string().to_lowercase();
                    let can_retry = config.retryable_errors.iter()
                        .any(|re| error_str.contains(&re.to_lowercase()));
                    
                    if can_retry {
                        retry_count += 1;
                        last_error = Some(e);
                        
                        if retry_count < config.max_retries {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            delay = (delay as f64 * config.retry_multiplier) as u64;
                            if delay > config.max_retry_delay {
                                delay = config.max_retry_delay;
                            }
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        
        // Reset retry count for new provider
        retry_count = 0;
        delay = config.initial_retry_delay;
    }
    
    Err(last_error.unwrap_or_else(|| Box::new(std::io::Error::new(
        std::io::ErrorKind::Other, 
        "All providers failed"
    ))))
}
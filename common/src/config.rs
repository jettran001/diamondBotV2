// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
    path::PathBuf,
};

// Third party imports
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Config trait
#[async_trait]
pub trait Config: Send + Sync + 'static {
    /// Lấy giá trị config
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>>;

    /// Lưu giá trị config
    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()>;

    /// Xóa giá trị config
    async fn remove(&self, key: &str) -> Result<()>;

    /// Xóa tất cả giá trị config
    async fn clear(&self) -> Result<()>;
}

/// Config entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry<T> {
    /// Giá trị
    pub value: T,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Thời gian cập nhật
    pub updated_at: SystemTime,
}

/// Basic config
#[derive(Debug, Clone)]
pub struct BasicConfig {
    config: Arc<RwLock<ConfigConfig>>,
    entries: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

/// Cấu hình config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

impl BasicConfig {
    /// Tạo config mới
    pub fn new(config: ConfigConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Config for BasicConfig {
    async fn get<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let entries = self.entries.read().unwrap();
        if let Some(data) = entries.get(key) {
            let entry: ConfigEntry<T> = bincode::deserialize(data)?;
            return Ok(Some(entry.value));
        }
        Ok(None)
    }

    async fn set<T: Serialize + for<'de> Deserialize<'de>>(&self, key: &str, value: T) -> Result<()> {
        let now = SystemTime::now();
        let entry = ConfigEntry {
            value,
            created_at: now,
            updated_at: now,
        };
        let data = bincode::serialize(&entry)?;
        let mut entries = self.entries.write().unwrap();
        entries.insert(key.to_string(), data);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.clear();
        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test ConfigEntry
    #[test]
    fn test_config_entry() {
        let now = SystemTime::now();
        let entry = ConfigEntry {
            value: "test".to_string(),
            created_at: now,
            updated_at: now,
        };
        assert_eq!(entry.value, "test");
    }

    /// Test ConfigConfig
    #[test]
    fn test_config_config() {
        let config = ConfigConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
    }

    /// Test BasicConfig
    #[test]
    fn test_basic_config() {
        let config = ConfigConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
        };
        let basic_config = BasicConfig::new(config);
        assert!(basic_config.config.read().unwrap().config_id == "test");
    }
}

/// Cấu hình chung của hệ thống
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Tên ứng dụng
    pub app_name: String,
    
    /// Phiên bản
    pub version: String,
    
    /// Port cho gateway API
    pub gateway_port: Option<u16>,
    
    /// Đường dẫn thư mục dữ liệu
    pub data_dir: PathBuf,
    
    /// Đường dẫn thư mục log
    pub log_dir: PathBuf,
    
    /// URL Redis
    pub redis_url: String,
    
    /// Mật khẩu Redis
    pub redis_password: Option<String>,
    
    /// Cấu hình blockchain
    pub blockchain: BlockchainConfig,
    
    /// Cấu hình mạng
    pub network: NetworkConfig,
    
    /// Cấu hình snipebot
    pub snipebot: SnipebotConfig,
    
    /// Cấu hình wallet
    pub wallet: WalletConfig,
    
    /// Cấu hình AI
    pub ai: AIConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_name: "DiamondChain".to_string(),
            version: "0.1.0".to_string(),
            gateway_port: Some(8000),
            data_dir: PathBuf::from("./data"),
            log_dir: PathBuf::from("./logs"),
            redis_url: "redis://127.0.0.1:6379".to_string(),
            redis_password: None,
            blockchain: BlockchainConfig::default(),
            network: NetworkConfig::default(),
            snipebot: SnipebotConfig::default(),
            wallet: WalletConfig::default(),
            ai: AIConfig::default(),
        }
    }
}

impl Config {
    /// Tạo cấu hình từ file
    pub fn from_file(path: &str) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("Không thể mở file cấu hình {}", path))?;
        
        let config: Config = serde_json::from_reader(file)
            .with_context(|| format!("Không thể parse JSON từ file cấu hình {}", path))?;
        
        Ok(config)
    }
    
    /// Lưu cấu hình vào file
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let file = std::fs::File::create(path)
            .with_context(|| format!("Không thể tạo file cấu hình {}", path))?;
        
        serde_json::to_writer_pretty(file, self)
            .with_context(|| format!("Không thể serialize config sang file {}", path))?;
        
        Ok(())
    }
}

/// Cấu hình Blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// Chain ID mặc định
    pub default_chain_id: u64,
    
    /// Danh sách RPC URLs
    pub rpc_urls: Vec<String>,
    
    /// Số lần retry tối đa
    pub max_retries: u32,
    
    /// Thời gian giữa các lần retry (giây)
    pub retry_interval: u64,
    
    /// Timeout (giây)
    pub timeout: u64,
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            default_chain_id: 1,
            rpc_urls: vec!["http://localhost:8545".to_string()],
            max_retries: 3,
            retry_interval: 5,
            timeout: 30,
        }
    }
}

/// Cấu hình Network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Master node URL
    pub master_url: String,
    
    /// Master node port
    pub master_port: u16,
    
    /// QUIC port
    pub quic_port: u16,
    
    /// Websocket port
    pub websocket_port: u16,
    
    /// gRPC port
    pub grpc_port: u16,
    
    /// Số lượng nodes
    pub nodes_count: u16,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            master_url: "127.0.0.1".to_string(),
            master_port: 9000,
            quic_port: 9001,
            websocket_port: 9002,
            grpc_port: 9003,
            nodes_count: 2,
        }
    }
}

/// Cấu hình Snipebot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnipebotConfig {
    /// API port
    pub api_port: u16,
    
    /// Max gas price (gwei)
    pub max_gas_price: u64,
    
    /// Slippage (%)
    pub slippage: f64,
    
    /// Số lượng transaction đồng thời tối đa
    pub max_concurrent_txs: u32,
}

impl Default for SnipebotConfig {
    fn default() -> Self {
        Self {
            api_port: 8080,
            max_gas_price: 100,
            slippage: 0.5,
            max_concurrent_txs: 5,
        }
    }
}

/// Cấu hình Wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Đường dẫn keystore
    pub keystore_path: PathBuf,
    
    /// Thời gian cache private key (phút)
    pub cache_timeout: u64,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            keystore_path: PathBuf::from("./data/keystore"),
            cache_timeout: 30,
        }
    }
}

/// Cấu hình AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    /// API URL
    pub api_url: String,
    
    /// API key
    pub api_key: Option<String>,
    
    /// Số lượng mô hình chạy song song
    pub concurrent_models: u32,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:5000".to_string(),
            api_key: None,
            concurrent_models: 2,
        }
    }
} 
use std::sync::Arc;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use log::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use std::collections::HashMap;
use tokio::time::{Duration, interval};
use crate::slave_node::extension_system::{Extension, ExtensionMetadata, ExtensionVersion, ExtensionContext, ExtensionStatus, ExtensionDependency};

/// Thông tin phiên bản extension
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensionVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ExtensionVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
    
    pub fn to_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
    
    pub fn from_string(version_str: &str) -> Result<Self> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid version format. Expected x.y.z"));
        }
        
        let major = parts[0].parse::<u32>()?;
        let minor = parts[1].parse::<u32>()?;
        let patch = parts[2].parse::<u32>()?;
        
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
    
    pub fn is_compatible_with(&self, other: &ExtensionVersion) -> bool {
        // Kiểm tra tương thích ngữ nghĩa
        // Major version khác nhau thì không tương thích
        if self.major != other.major {
            return false;
        }
        
        // Minor version của self phải >= minor version của other
        if self.minor < other.minor {
            return false;
        }
        
        true
    }
}

/// Metadata của extension
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: ExtensionVersion,
    pub author: String,
    pub dependencies: Vec<ExtensionDependency>,
    pub required_permissions: Vec<String>,
    pub tags: Vec<String>,
    pub config_schema: Option<serde_json::Value>,
}

/// Extension dependency
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensionDependency {
    pub extension_id: String,
    pub min_version: ExtensionVersion,
    pub optional: bool,
}

/// Trạng thái của extension
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ExtensionStatus {
    Active,
    Inactive,
    Error(String),
    Loading,
}

/// Context của extension
#[derive(Clone)]
pub struct ExtensionContext {
    pub metadata: ExtensionMetadata,
    pub config: serde_json::Value,
    pub storage: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    pub node_info: NodeInfo,
}

/// Thông tin Node
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub network_version: String,
    pub capabilities: Vec<String>,
    pub connected_peers: usize,
}

/// Cấu hình Mempool Monitor
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MempoolMonitorConfig {
    pub monitor_interval_sec: u64,
    pub alert_threshold: usize,
    pub chains_to_monitor: Vec<u64>,
}

impl Default for MempoolMonitorConfig {
    fn default() -> Self {
        Self {
            monitor_interval_sec: 30,
            alert_threshold: 5000,
            chains_to_monitor: vec![1, 56, 137], // Ethereum, BSC, Polygon
        }
    }
}

/// Thống kê mempool
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MempoolStats {
    pub chain_id: u64,
    pub pending_transactions: usize,
    pub gas_price_stats: GasPriceStats,
    pub transaction_rate: f64, // giao dịch/giây
    pub congestion_level: CongestionLevel,
    pub timestamp: u64,
}

/// Thống kê giá gas
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GasPriceStats {
    pub avg_gas_price: String, // Gwei
    pub min_gas_price: String, // Gwei
    pub max_gas_price: String, // Gwei
    pub median_gas_price: String, // Gwei
}

/// Mức độ tắc nghẽn
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CongestionLevel {
    Low,
    Medium,
    High,
    Extreme,
}

/// Mempool Monitor Extension
pub struct MempoolMonitorExtension {
    metadata: ExtensionMetadata,
    config: RwLock<MempoolMonitorConfig>,
    context: RwLock<Option<ExtensionContext>>,
    stats: RwLock<HashMap<u64, MempoolStats>>, // chain_id -> stats
    running: RwLock<bool>,
}

impl MempoolMonitorExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            id: "mempool_monitor".to_string(),
            name: "Mempool Monitor".to_string(),
            description: "Giám sát mempool trên nhiều blockchain và cung cấp thống kê".to_string(),
            version: ExtensionVersion::new(1, 0, 0),
            author: "DiamondChain Team".to_string(),
            dependencies: vec![],
            required_permissions: vec!["blockchain_query".to_string()],
            tags: vec!["mempool".to_string(), "monitoring".to_string()],
            config_schema: Some(serde_json::to_value(MempoolMonitorConfig::default()).unwrap()),
        };
        
        Self {
            metadata,
            config: RwLock::new(MempoolMonitorConfig::default()),
            context: RwLock::new(None),
            stats: RwLock::new(HashMap::new()),
            running: RwLock::new(false),
        }
    }
    
    /// Lấy thống kê từ mempool cho một blockchain cụ thể
    async fn fetch_mempool_stats(&self, chain_id: u64) -> Result<MempoolStats> {
        // Trong thực tế, sẽ fetch từ blockchain node
        // Ở đây mô phỏng dữ liệu
        
        let pending_tx = match chain_id {
            1 => 3000 + (rand::random::<usize>() % 5000), // Ethereum
            56 => 8000 + (rand::random::<usize>() % 10000), // BSC
            137 => 4000 + (rand::random::<usize>() % 3000), // Polygon
            _ => 1000 + (rand::random::<usize>() % 2000),
        };
        
        let min_gas = 10 + (rand::random::<u64>() % 30);
        let max_gas = min_gas + 50 + (rand::random::<u64>() % 100);
        let avg_gas = min_gas + (max_gas - min_gas) / 2;
        let median_gas = avg_gas + ((rand::random::<i64>() % 20) - 10) as u64;
        
        let tx_rate = pending_tx as f64 / 15.0; // Giả định block time 15s
        
        let congestion_level = if pending_tx < 2000 {
            CongestionLevel::Low
        } else if pending_tx < 5000 {
            CongestionLevel::Medium
        } else if pending_tx < 10000 {
            CongestionLevel::High
        } else {
            CongestionLevel::Extreme
        };
        
        Ok(MempoolStats {
            chain_id,
            pending_transactions: pending_tx,
            gas_price_stats: GasPriceStats {
                avg_gas_price: avg_gas.to_string(),
                min_gas_price: min_gas.to_string(),
                max_gas_price: max_gas.to_string(),
                median_gas_price: median_gas.to_string(),
            },
            transaction_rate: tx_rate,
            congestion_level,
            timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }
    
    /// Kiểm tra và gửi cảnh báo nếu cần
    async fn check_and_alert(&self, stats: &MempoolStats) -> Result<()> {
        let config = self.config.read().await;
        
        if stats.pending_transactions > config.alert_threshold {
            // Trong thực tế, sẽ gửi cảnh báo qua webhooks hoặc message queue
            warn!(
                "ALERT: Chain {} has high mempool congestion. Pending txs: {}, Level: {:?}",
                stats.chain_id, stats.pending_transactions, stats.congestion_level
            );
            
            // Lưu cảnh báo vào storage của extension
            if let Some(ctx) = self.context.read().await.as_ref() {
                let mut storage = ctx.storage.write().await;
                let alert_key = format!("alert:{}:{}", stats.chain_id, stats.timestamp);
                let alert_data = serde_json::to_vec(stats)?;
                storage.insert(alert_key, alert_data);
            }
        }
        
        Ok(())
    }
    
    /// Bắt đầu task giám sát
    async fn start_monitoring(&self) -> Result<()> {
        let config = self.config.read().await.clone();
        *self.running.write().await = true;
        
        let chains = config.chains_to_monitor.clone();
        let interval_duration = Duration::from_secs(config.monitor_interval_sec);
        
        // Tạo task giám sát
        let stats = self.stats.clone();
        let running = self.running.clone();
        let self_clone = Arc::new(self.clone());
        
        tokio::spawn(async move {
            let mut interval_timer = interval(interval_duration);
            
            while *running.read().await {
                interval_timer.tick().await;
                
                for &chain_id in &chains {
                    match self_clone.fetch_mempool_stats(chain_id).await {
                        Ok(chain_stats) => {
                            // Cập nhật thống kê
                            stats.write().await.insert(chain_id, chain_stats.clone());
                            
                            // Kiểm tra và gửi cảnh báo
                            if let Err(e) = self_clone.check_and_alert(&chain_stats).await {
                                error!("Error sending alert for chain {}: {}", chain_id, e);
                            }
                            
                            debug!("Updated mempool stats for chain {}", chain_id);
                        },
                        Err(e) => {
                            error!("Failed to fetch mempool stats for chain {}: {}", chain_id, e);
                        }
                    }
                }
            }
        });
        
        info!("Mempool monitoring started for chains: {:?}", chains);
        Ok(())
    }
    
    /// Dừng task giám sát
    async fn stop_monitoring(&self) -> Result<()> {
        *self.running.write().await = false;
        info!("Mempool monitoring stopped");
        Ok(())
    }
}

#[async_trait]
impl Extension for MempoolMonitorExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }
    
    async fn init(&self, context: ExtensionContext) -> Result<()> {
        // Lưu context
        *self.context.write().await = Some(context.clone());
        
        // Đọc cấu hình từ context
        if let Some(config_value) = context.config.get("mempool_monitor") {
            if let Ok(config) = serde_json::from_value::<MempoolMonitorConfig>(config_value.clone()) {
                *self.config.write().await = config;
            }
        }
        
        info!("Mempool Monitor Extension initialized");
        Ok(())
    }
    
    async fn activate(&self) -> Result<()> {
        // Khởi động task giám sát
        self.start_monitoring().await?;
        info!("Mempool Monitor Extension activated");
        Ok(())
    }
    
    async fn deactivate(&self) -> Result<()> {
        // Dừng task giám sát
        self.stop_monitoring().await?;
        info!("Mempool Monitor Extension deactivated");
        Ok(())
    }
    
    async fn handle_message(&self, message: &[u8]) -> Result<Option<Vec<u8>>> {
        // Xử lý message
        let message_str = std::str::from_utf8(message)?;
        
        match message_str {
            "get_stats" => {
                let stats = self.stats.read().await.clone();
                let stats_json = serde_json::to_vec(&stats)?;
                Ok(Some(stats_json))
            },
            _ if message_str.starts_with("get_chain_stats:") => {
                let parts: Vec<&str> = message_str.split(':').collect();
                if parts.len() >= 2 {
                    if let Ok(chain_id) = parts[1].parse::<u64>() {
                        let stats = self.stats.read().await;
                        if let Some(chain_stats) = stats.get(&chain_id) {
                            return Ok(Some(serde_json::to_vec(chain_stats)?));
                        }
                    }
                }
                Ok(None)
            },
            _ => {
                warn!("Unknown message: {}", message_str);
                Ok(None)
            }
        }
    }
    
    async fn update_config(&self, config: serde_json::Value) -> Result<()> {
        // Cập nhật cấu hình
        if let Ok(new_config) = serde_json::from_value::<MempoolMonitorConfig>(config) {
            let restart_required = {
                let current_config = self.config.read().await;
                current_config.monitor_interval_sec != new_config.monitor_interval_sec
                    || current_config.chains_to_monitor != new_config.chains_to_monitor
            };
            
            // Cập nhật cấu hình
            *self.config.write().await = new_config;
            
            // Khởi động lại nếu cần
            if restart_required && *self.running.read().await {
                self.stop_monitoring().await?;
                self.start_monitoring().await?;
            }
            
            info!("Mempool Monitor config updated");
        }
        
        Ok(())
    }
}

impl Clone for MempoolMonitorExtension {
    fn clone(&self) -> Self {
        // Chỉ clone để sử dụng trong Arc, không thực sự copy state
        Self {
            metadata: self.metadata.clone(),
            config: RwLock::new(self.config.try_read().unwrap_or_default().clone()),
            context: RwLock::new(None),
            stats: RwLock::new(HashMap::new()),
            running: RwLock::new(*self.running.try_read().unwrap_or(&false)),
        }
    }
}

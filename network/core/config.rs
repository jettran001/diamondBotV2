// External imports
use anyhow::{Result, Context};

// Standard library imports
use std::{
    collections::HashMap,
    fs,
    path::Path,
    time::Duration,
};

// Third party imports
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use serde_json;

/// Loại node trong mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeType {
    /// Node master
    Master,
    /// Node slave
    Slave,
    /// Node server
    Server,
}

impl Default for NodeType {
    fn default() -> Self {
        Self::Slave
    }
}

/// Loại storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageType {
    /// Storage trong bộ nhớ
    Memory,
    /// Storage trong Redis
    Redis,
    /// Storage trong file
    File,
}

impl Default for StorageType {
    fn default() -> Self {
        Self::Memory
    }
}

/// Cấu hình mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    // Cấu hình chung
    /// ID của node
    pub node_id: String,
    /// Loại node
    pub node_type: NodeType,
    /// Địa chỉ lắng nghe
    pub listen_address: String,
    /// Port lắng nghe
    pub listen_port: u16,
    
    // WebSocket
    /// Có bật WebSocket không
    pub ws_enabled: bool,
    /// Port WebSocket
    pub ws_port: u16,
    /// Đường dẫn WebSocket
    pub ws_path: String,
    
    // gRPC
    /// Có bật gRPC không
    pub grpc_enabled: bool,
    /// Port gRPC
    pub grpc_port: u16,
    
    // QUIC
    /// Có bật QUIC không
    pub quic_enabled: bool,
    /// Port QUIC
    pub quic_port: u16,
    /// Đường dẫn certificate
    pub cert_path: Option<String>,
    /// Đường dẫn private key
    pub key_path: Option<String>,
    
    // Discovery
    /// Danh sách node seed
    pub seed_nodes: Vec<String>,
    /// Có tự động discovery không
    pub auto_discovery: bool,
    /// Khoảng thời gian discovery (giây)
    pub discovery_interval: Duration,
    
    // Storage
    /// Loại storage
    pub storage_type: StorageType,
    /// Cấu hình storage
    pub storage_config: HashMap<String, String>,
    
    // Monitoring
    /// Có bật Prometheus không
    pub prometheus_enabled: bool,
    /// Port Prometheus
    pub prometheus_port: u16,
    /// Level logging
    pub logging_level: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            node_id: Uuid::new_v4().to_string(),
            node_type: NodeType::default(),
            listen_address: "0.0.0.0".to_string(),
            listen_port: 8080,
            
            ws_enabled: true,
            ws_port: 8081,
            ws_path: "/ws".to_string(),
            
            grpc_enabled: true,
            grpc_port: 8082,
            
            quic_enabled: true,
            quic_port: 8083,
            cert_path: None,
            key_path: None,
            
            seed_nodes: vec![],
            auto_discovery: true,
            discovery_interval: Duration::from_secs(60),
            
            storage_type: StorageType::default(),
            storage_config: HashMap::new(),
            
            prometheus_enabled: true,
            prometheus_port: 9090,
            logging_level: "info".to_string(),
        }
    }
}

impl NetworkConfig {
    /// Tạo mới cấu hình mạng
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Đọc cấu hình từ file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context("Failed to read config file")?;
        serde_json::from_str(&content)
            .context("Failed to parse config file")
    }
    
    /// Lưu cấu hình vào file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
        fs::write(path, content)
            .context("Failed to write config file")
    }

    /// Kiểm tra cấu hình hợp lệ
    pub fn validate(&self) -> Result<()> {
        // Kiểm tra port
        if self.listen_port == 0 {
            return Err(anyhow::anyhow!("Invalid listen port"));
        }
        if self.ws_enabled && self.ws_port == 0 {
            return Err(anyhow::anyhow!("Invalid WebSocket port"));
        }
        if self.grpc_enabled && self.grpc_port == 0 {
            return Err(anyhow::anyhow!("Invalid gRPC port"));
        }
        if self.quic_enabled && self.quic_port == 0 {
            return Err(anyhow::anyhow!("Invalid QUIC port"));
        }
        if self.prometheus_enabled && self.prometheus_port == 0 {
            return Err(anyhow::anyhow!("Invalid Prometheus port"));
        }

        // Kiểm tra certificate và key
        if self.quic_enabled {
            if self.cert_path.is_none() {
                return Err(anyhow::anyhow!("QUIC enabled but no certificate path provided"));
            }
            if self.key_path.is_none() {
                return Err(anyhow::anyhow!("QUIC enabled but no private key path provided"));
            }
        }

        Ok(())
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Test default config
    #[test]
    fn test_default_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.node_type, NodeType::Slave);
        assert_eq!(config.listen_address, "0.0.0.0");
        assert_eq!(config.listen_port, 8080);
        assert_eq!(config.storage_type, StorageType::Memory);
        assert_eq!(config.discovery_interval, Duration::from_secs(60));
    }

    /// Test config validation
    #[test]
    fn test_config_validation() {
        let mut config = NetworkConfig::default();
        assert!(config.validate().is_ok());

        config.listen_port = 0;
        assert!(config.validate().is_err());

        config.listen_port = 8080;
        config.quic_enabled = true;
        assert!(config.validate().is_err());

        config.cert_path = Some("cert.pem".to_string());
        assert!(config.validate().is_err());

        config.key_path = Some("key.pem".to_string());
        assert!(config.validate().is_ok());
    }

    /// Test config file operations
    #[test]
    fn test_config_file() {
        let config = NetworkConfig::default();
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        config.save_to_file(path).unwrap();
        let loaded = NetworkConfig::from_file(path).unwrap();

        assert_eq!(loaded.node_id, config.node_id);
        assert_eq!(loaded.node_type, config.node_type);
        assert_eq!(loaded.listen_address, config.listen_address);
        assert_eq!(loaded.listen_port, config.listen_port);
    }
} 
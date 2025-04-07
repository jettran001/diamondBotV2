use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    // Cấu hình chung
    pub node_id: String,
    pub node_type: String, // "master", "slave", "server"
    pub listen_address: String,
    pub listen_port: u16,
    
    // WebSocket
    pub ws_enabled: bool,
    pub ws_port: u16,
    pub ws_path: String,
    
    // gRPC
    pub grpc_enabled: bool,
    pub grpc_port: u16,
    
    // QUIC
    pub quic_enabled: bool,
    pub quic_port: u16,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    
    // Discovery
    pub seed_nodes: Vec<String>,
    pub auto_discovery: bool,
    pub discovery_interval: u64, // Giây
    
    // Storage
    pub storage_type: String, // "memory", "redis", "file"
    pub storage_config: HashMap<String, String>,
    
    // Monitoring
    pub prometheus_enabled: bool,
    pub prometheus_port: u16,
    pub logging_level: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            node_id: uuid::Uuid::new_v4().to_string(),
            node_type: "slave".to_string(),
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
            discovery_interval: 60,
            
            storage_type: "memory".to_string(),
            storage_config: HashMap::new(),
            
            prometheus_enabled: true,
            prometheus_port: 9090,
            logging_level: "info".to_string(),
        }
    }
}

impl NetworkConfig {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }
    
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
} 
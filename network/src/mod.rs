// External imports
use ethers::types::U256;

// Re-export từ core
pub mod core;
pub use core::*;

// Chỉ định các module khác trong network
pub mod websocket;
pub mod grpc;
pub mod websocket_server;
pub mod grpc_server;
pub mod redis_service;
pub mod quic;
pub mod nodes;

/// Cấu trúc điểm cuối
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub chain_id: u64,
    pub status: EndpointStatus,
    pub max_gas_price: U256,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::NetworkConfig;

    #[test]
    fn test_endpoint() {
        let config = NetworkConfig::default();
        let endpoint = Endpoint {
            url: config.rpc_url.clone(),
            chain_id: config.chain_id,
            status: EndpointStatus::Active,
            max_gas_price: config.max_gas_price,
        };

        assert_eq!(endpoint.url, "http://localhost:8545");
        assert_eq!(endpoint.chain_id, 1);
        assert_eq!(endpoint.status, EndpointStatus::Active);
    }
} 
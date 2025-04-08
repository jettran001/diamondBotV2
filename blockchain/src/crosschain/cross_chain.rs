// External imports
use ethers::{
    types::{Address, H256, U256},
    providers::{Provider, Http},
    contract::{Contract, ContractFactory},
    abi::{Abi, Token},
    utils::keccak256,
};

// Standard library imports
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    fmt::{self, Display, Formatter},
    error::Error,
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::{timeout, sleep};

// Internal imports
mod chain_config;
mod bridge_contract;

use chain_config::{ChainConfig, ChainId};
use bridge_contract::{BridgeContract, BridgeEvent};

/// Bridge cross-chain cho token
#[async_trait]
pub trait CrossChainBridge: Send + Sync + 'static {
    /// Chuyển token qua bridge
    /// 
    /// # Arguments
    /// 
    /// * `token` - Địa chỉ token
    /// * `from_chain` - Chain nguồn
    /// * `to_chain` - Chain đích
    /// * `amount` - Số lượng token
    /// * `recipient` - Địa chỉ nhận
    /// 
    /// # Returns
    /// 
    /// * `Result<H256>` - Hash của giao dịch bridge
    async fn bridge_token(
        &self,
        token: &str,
        from_chain: ChainId,
        to_chain: ChainId,
        amount: U256,
        recipient: &str,
    ) -> Result<H256>;

    /// Lấy phí bridge
    /// 
    /// # Arguments
    /// 
    /// * `from_chain` - Chain nguồn
    /// * `to_chain` - Chain đích
    /// 
    /// # Returns
    /// 
    /// * `Result<U256>` - Phí bridge
    async fn get_bridge_fee(
        &self,
        from_chain: ChainId,
        to_chain: ChainId,
    ) -> Result<U256>;
}

/// Implementation của CrossChainBridge
#[derive(Clone)]
pub struct CrossChainBridgeImpl {
    /// Cấu hình các chain được hỗ trợ
    pub supported_chains: Arc<RwLock<HashMap<ChainId, ChainConfig>>>,
    /// Hợp đồng bridge cho từng cặp chain
    pub bridge_contracts: Arc<RwLock<HashMap<(ChainId, ChainId), BridgeContract>>>,
    /// ID chủ sở hữu
    pub owner_id: String,
    /// Thời gian cập nhật cuối cùng
    pub last_update: u64,
    /// ID của bridge
    pub id: String,
    /// Thời gian tạo
    pub created_at: u64,
}

impl CrossChainBridgeImpl {
    /// Khởi tạo bridge mới
    /// 
    /// # Arguments
    /// 
    /// * `owner_id` - ID chủ sở hữu
    /// 
    /// # Returns
    /// 
    /// * `Self` - Instance mới của CrossChainBridgeImpl
    pub fn new(owner_id: String) -> Self {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        Self {
            supported_chains: Arc::new(RwLock::new(HashMap::new())),
            bridge_contracts: Arc::new(RwLock::new(HashMap::new())),
            owner_id,
            last_update: current_time,
            id: format!("bridge_{}", current_time),
            created_at: current_time,
        }
    }
    
    /// Thêm chain mới vào danh sách hỗ trợ
    /// 
    /// # Arguments
    /// 
    /// * `chain_id` - ID của chain
    /// * `config` - Cấu hình chain
    /// 
    /// # Returns
    /// 
    /// * `Result<()>` - Kết quả thêm chain
    pub fn add_chain(&self, chain_id: ChainId, config: ChainConfig) -> Result<()> {
        let mut chains = self.supported_chains.write().unwrap();
        chains.insert(chain_id, config);
        
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        info!("Added new chain: {}", chain_id);
        
        Ok(())
    }
    
    /// Thêm hợp đồng bridge cho cặp chain
    /// 
    /// # Arguments
    /// 
    /// * `from_chain` - Chain nguồn
    /// * `to_chain` - Chain đích
    /// * `contract` - Hợp đồng bridge
    /// 
    /// # Returns
    /// 
    /// * `Result<()>` - Kết quả thêm hợp đồng
    pub fn add_bridge_contract(
        &self,
        from_chain: ChainId,
        to_chain: ChainId,
        contract: BridgeContract,
    ) -> Result<()> {
        let mut contracts = self.bridge_contracts.write().unwrap();
        contracts.insert((from_chain, to_chain), contract);
        
        info!("Added bridge contract for {} -> {}", from_chain, to_chain);
        
        Ok(())
    }
}

#[async_trait]
impl CrossChainBridge for CrossChainBridgeImpl {
    async fn bridge_token(
        &self,
        token: &str,
        from_chain: ChainId,
        to_chain: ChainId,
        amount: U256,
        recipient: &str,
    ) -> Result<H256> {
        // Kiểm tra chain có được hỗ trợ không
        let chains = self.supported_chains.read().unwrap();
        if !chains.contains_key(&from_chain) || !chains.contains_key(&to_chain) {
            return Err(anyhow::anyhow!("Chain not supported"));
        }
        
        // Lấy hợp đồng bridge
        let contracts = self.bridge_contracts.read().unwrap();
        let contract = contracts.get(&(from_chain, to_chain))
            .ok_or_else(|| anyhow::anyhow!("Bridge contract not found"))?;
            
        // Gọi hàm bridge token
        let tx_hash = contract.bridge_token(token, amount, recipient).await?;
        
        info!("Bridged {} tokens from {} to {}: {}", 
            amount, from_chain, to_chain, tx_hash);
            
        Ok(tx_hash)
    }
    
    async fn get_bridge_fee(
        &self,
        from_chain: ChainId,
        to_chain: ChainId,
    ) -> Result<U256> {
        // Kiểm tra chain có được hỗ trợ không
        let chains = self.supported_chains.read().unwrap();
        if !chains.contains_key(&from_chain) || !chains.contains_key(&to_chain) {
            return Err(anyhow::anyhow!("Chain not supported"));
        }
        
        // Lấy hợp đồng bridge
        let contracts = self.bridge_contracts.read().unwrap();
        let contract = contracts.get(&(from_chain, to_chain))
            .ok_or_else(|| anyhow::anyhow!("Bridge contract not found"))?;
            
        // Lấy phí bridge
        let fee = contract.get_bridge_fee().await?;
        
        info!("Bridge fee from {} to {}: {}", from_chain, to_chain, fee);
        
        Ok(fee)
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;
    use std::str::FromStr;

    /// Test khởi tạo bridge
    #[test]
    fn test_new() {
        let bridge = CrossChainBridgeImpl::new("alice".to_string());
        
        assert_eq!(bridge.owner_id, "alice");
        assert!(bridge.supported_chains.read().unwrap().is_empty());
        assert!(bridge.bridge_contracts.read().unwrap().is_empty());
        assert!(!bridge.id.is_empty());
        assert!(bridge.created_at > 0);
        assert_eq!(bridge.last_update, bridge.created_at);
    }

    /// Test thêm chain
    #[test]
    fn test_add_chain() {
        let bridge = CrossChainBridgeImpl::new("alice".to_string());
        
        let config = ChainConfig {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 1,
            name: "Ethereum".to_string(),
        };
        
        assert!(bridge.add_chain(1, config.clone()).is_ok());
        assert_eq!(
            bridge.supported_chains.read().unwrap().get(&1).unwrap(),
            &config
        );
        assert!(bridge.last_update > bridge.created_at);
    }

    /// Test thêm hợp đồng bridge
    #[test]
    fn test_add_bridge_contract() {
        let bridge = CrossChainBridgeImpl::new("alice".to_string());
        
        let contract = BridgeContract::new(
            Address::from_str("0x1234567890123456789012345678901234567890").unwrap(),
            "http://localhost:8545".to_string(),
        );
        
        assert!(bridge.add_bridge_contract(1, 2, contract.clone()).is_ok());
        assert_eq!(
            bridge.bridge_contracts.read().unwrap().get(&(1, 2)).unwrap(),
            &contract
        );
    }
}

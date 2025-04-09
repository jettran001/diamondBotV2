use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    types::{Address, U256, Transaction, Filter, H256, BlockId, BlockNumber, Bytes},
    abi::{self, Token},
    core::types::{Address, U256, H256},
};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use log::{info, warn, error, debug};
use tokio::time::sleep;
use std::collections::HashMap;
use crate::chain_adapters::base::ChainConfig;
use async_trait::async_trait;
use std::marker::Send;
use diamond_blockchain::core::{
    BlockchainService, 
    BlockchainParams, 
    TokenInfo, 
    TransactionInfo, 
    TransactionStatus,
    create_blockchain_service,
    ContractInfo,
    ContractManager,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainParams {
    pub chain_id: u64,
    pub rpc_url: String,
    pub explorer_url: String,
}

/// Wrapper BlockchainMonitor từ diamond_blockchain
pub struct BlockchainMonitor {
    /// Service từ module blockchain
    service: Arc<dyn BlockchainService>,
}

impl BlockchainMonitor {
    /// Khởi tạo từ params
    pub fn new(params: BlockchainParams) -> Result<Self> {
        let service = create_blockchain_service(params)?;
        Ok(Self { 
            service: Arc::new(service)
        })
    }
    
    /// Lấy block mới nhất
    pub async fn get_latest_block(&self) -> Result<u64> {
        self.service.get_latest_block().await
    }
    
    /// Lấy thông tin token
    pub async fn get_token_info(&self, token_address: &str) -> Result<TokenInfo> {
        self.service.get_token_info(token_address).await
    }
    
    /// Lấy transaction receipt
    pub async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<ethers::types::TransactionReceipt>> {
        self.service.get_transaction_receipt(tx_hash).await
    }
    
    /// Đợi transaction được xác nhận
    pub async fn wait_for_transaction(&self, tx_hash: &str, timeout_secs: u64) -> Result<Option<ethers::types::TransactionReceipt>> {
        self.service.wait_for_transaction(tx_hash, timeout_secs).await
    }
    
    /// Lấy gas price
    pub async fn get_gas_price(&self) -> Result<U256> {
        self.service.get_gas_price().await
    }
    
    /// Lấy số dư ETH
    pub async fn get_eth_balance(&self, address: &str) -> Result<U256> {
        self.service.get_eth_balance(address).await
    }
    
    /// Gọi hàm của contract
    pub async fn call_contract_function<T: ethers::abi::Detokenize + 'static>(
        &self,
        contract_address: &str,
        function_name: &str,
        function_params: Vec<ethers::abi::Token>,
        abi_path: &str
    ) -> Result<T> {
        self.service.call_contract_function(
            contract_address,
            function_name,
            function_params,
            abi_path
        ).await
    }
}

/// Re-export BlockchainParams từ module blockchain/core
pub use diamond_blockchain::core::BlockchainParams;

/// Re-export TokenInfo từ module blockchain/core
pub use diamond_blockchain::core::TokenInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: U256,
}

// Contract management section (merged from contracts.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    pub address: String,
    pub name: String,
    pub abi: String,
    pub chain_id: u64,
    pub created_at: u64,
    pub last_used: u64,
    pub verified: bool,
}

pub struct ContractManager {
    contracts: Vec<ContractInfo>,
    path: String,
}

impl ContractManager {
    pub async fn new(path: &str) -> Result<Self> {
        let manager = Self {
            contracts: Vec::new(),
            path: path.to_string(),
        };
        
        // Load contracts from file if exists
        // Similar to WalletManager
        
        Ok(manager)
    }
    
    pub fn add_contract(&mut self, contract: ContractInfo) -> Result<()> {
        // Check if contract already exists
        if self.contracts.iter().any(|c| c.address.to_lowercase() == contract.address.to_lowercase() && c.chain_id == contract.chain_id) {
            return Err(anyhow!("Contract already exists"));
        }
        
        self.contracts.push(contract);
        Ok(())
    }
    
    pub fn get_contract(&self, address: &str, chain_id: u64) -> Option<&ContractInfo> {
        self.contracts.iter().find(|c| c.address.to_lowercase() == address.to_lowercase() && c.chain_id == chain_id)
    }
    
    pub fn get_all_contracts(&self) -> &[ContractInfo] {
        &self.contracts
    }
    
    pub fn get_contracts_by_chain(&self, chain_id: u64) -> Vec<&ContractInfo> {
        self.contracts.iter().filter(|c| c.chain_id == chain_id).collect()
    }
    
    // Cập nhật phương thức interact để sử dụng tốt hơn với generics và lifetime
    pub async fn interact<M, T>(&self, client: Arc<M>, address: &str, function: &str, args: Vec<T>) -> Result<Bytes> 
    where
        M: Middleware + 'static,
        T: Into<Token> + Send + Sync,
    {
        // Lấy chain ID từ client
        let chain_id = client.get_chainid().await?;
        
        // Tìm thông tin contract
        let contract_info = self.get_contract(address, chain_id.as_u64())
            .ok_or_else(|| anyhow!("Contract not found"))?;
        
        // Parse địa chỉ contract
        let contract_address = Address::from_str(address)?;
        
        // Parse ABI của contract
        let abi: ethers::abi::Abi = serde_json::from_str(&contract_info.abi)?;
        
        // Tạo contract instance
        let contract = Contract::new(contract_address, abi, client);
        
        // Chuyển đổi tham số
        let params: Vec<Token> = args.into_iter()
            .map(|arg| arg.into())
            .collect();
        
        // Gọi phương thức
        let result = contract.method(function, params)?
            .call()
            .await?;
        
        Ok(result)
    }
    
    // Thêm phiên bản trực tiếp với tham số là Token, tránh việc chuyển đổi type
    pub async fn interact_with_tokens<M>(&self, client: Arc<M>, address: &str, function: &str, args: Vec<Token>) -> Result<Bytes> 
    where
        M: Middleware + 'static,
    {
        // Lấy chain ID từ client
        let chain_id = client.get_chainid().await?;
        
        // Tìm thông tin contract
        let contract_info = self.get_contract(address, chain_id.as_u64())
            .ok_or_else(|| anyhow!("Contract not found"))?;
        
        // Parse địa chỉ contract
        let contract_address = Address::from_str(address)?;
        
        // Parse ABI của contract
        let abi: ethers::abi::Abi = serde_json::from_str(&contract_info.abi)?;
        
        // Tạo contract instance
        let contract = Contract::new(contract_address, abi, client);
        
        // Gọi phương thức với tham số đã được chuyển đổi sẵn
        let result = contract.method(function, args)?
            .call()
            .await?;
        
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_blockchainparams_serialize() {
        let params = BlockchainParams {
            chain_id: 1,
            rpc_url: "https://mainnet.infura.io/v3/YOUR-API-KEY".to_string(),
            explorer_url: "https://etherscan.io".to_string(),
        };
        
        let json = serde_json::to_string(&params).unwrap();
        assert!(!json.is_empty());
        
        let deserialized: BlockchainParams = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chain_id, 1);
        assert_eq!(deserialized.rpc_url, "https://mainnet.infura.io/v3/YOUR-API-KEY");
        assert_eq!(deserialized.explorer_url, "https://etherscan.io");
    }
}

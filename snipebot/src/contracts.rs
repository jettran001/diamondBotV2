use ethers::prelude::*;
use ethers::types::{Address, U256, Bytes};
use ethers::providers::Middleware;
use std::sync::Arc;
use std::str::FromStr;
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use std::marker::Send;
use ethers::abi::Token;

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

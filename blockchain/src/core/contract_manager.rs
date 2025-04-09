// External imports
use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    types::{Address, U256, H256, Bytes},
    utils::keccak256,
};
use anyhow::{Result, Context, anyhow};
use thiserror::Error;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Instant, Duration};
use std::str::FromStr;
use tracing::{debug, error, info, warn};

// Internal imports
use crate::abi::abis::{PAIR_ABI, FACTORY_ABI, ERC20_ABI, ROUTER_ABI};

// Error type for contract operations
#[derive(Error, Debug)]
pub enum ContractError {
    #[error("Contract initialization error: {0}")]
    InitializationError(String),

    #[error("Contract call error: {0}")]
    CallError(String),

    #[error("ABI error: {0}")]
    AbiError(String),

    #[error("Contract address error: {0}")]
    AddressError(String),

    #[error("Provider error: {0}")]
    ProviderError(String),
}

// Cache entry for contract data
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    pub value: T,
    pub expires_at: Instant,
}

impl<T> CacheEntry<T> {
    pub fn new(value: T, ttl_seconds: u64) -> Self {
        Self {
            value,
            expires_at: Instant::now() + Duration::from_secs(ttl_seconds),
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

// Contract Manager handles contract interactions and caching
#[derive(Debug)]
pub struct ContractManager<M: Middleware + 'static> {
    provider: Arc<M>,
    contract_cache: Arc<RwLock<HashMap<Address, CacheEntry<Contract<Arc<M>>>>>>,
    abi_cache: Arc<RwLock<HashMap<String, CacheEntry<ethers::abi::Abi>>>>,
    contracts_info: Arc<RwLock<HashMap<Address, ContractInfo>>>,
}

// Contract information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    pub address: Address,
    pub name: String,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub contract_type: ContractType,
    pub verified: bool,
    pub chain_id: u64,
    pub created_at: u64,
    pub bytecode_hash: Option<H256>,
}

// Contract types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractType {
    ERC20,
    ERC721,
    DEXPair,
    DEXFactory,
    DEXRouter,
    MultiSig,
    Unknown,
}

impl<M: Middleware + 'static> ContractManager<M> {
    // Create a new contract manager
    pub fn new(provider: Arc<M>) -> Self {
        Self {
            provider,
            contract_cache: Arc::new(RwLock::new(HashMap::new())),
            abi_cache: Arc::new(RwLock::new(HashMap::new())),
            contracts_info: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // Get a contract instance by address with default ERC20 ABI
    pub async fn get_erc20(&self, address: Address) -> Result<Contract<Arc<M>>> {
        self.get_contract_with_abi(address, "erc20", ERC20_ABI).await
    }

    // Get a DEX pair contract
    pub async fn get_pair(&self, address: Address) -> Result<Contract<Arc<M>>> {
        self.get_contract_with_abi(address, "pair", PAIR_ABI).await
    }

    // Get a DEX factory contract
    pub async fn get_factory(&self, address: Address) -> Result<Contract<Arc<M>>> {
        self.get_contract_with_abi(address, "factory", FACTORY_ABI).await
    }

    // Get a DEX router contract
    pub async fn get_router(&self, address: Address) -> Result<Contract<Arc<M>>> {
        self.get_contract_with_abi(address, "router", ROUTER_ABI).await
    }

    // Get contract with custom ABI
    pub async fn get_contract_with_abi(&self, address: Address, abi_key: &str, abi_json: &str) -> Result<Contract<Arc<M>>> {
        // Check cache first
        if let Ok(cache) = self.contract_cache.read() {
            if let Some(entry) = cache.get(&address) {
                if !entry.is_expired() {
                    return Ok(entry.value.clone());
                }
            }
        }

        // Get ABI from cache or parse
        let abi = if let Ok(abi_cache) = self.abi_cache.read() {
            if let Some(entry) = abi_cache.get(abi_key) {
                if !entry.is_expired() {
                    entry.value.clone()
                } else {
                    self.parse_and_cache_abi(abi_key, abi_json).await?
                }
            } else {
                self.parse_and_cache_abi(abi_key, abi_json).await?
            }
        } else {
            self.parse_and_cache_abi(abi_key, abi_json).await?
        };

        // Create contract
        let contract = Contract::new(address, abi, self.provider.clone());

        // Cache contract
        if let Ok(mut cache) = self.contract_cache.write() {
            cache.insert(address, CacheEntry::new(contract.clone(), 3600)); // 1 hour
        }

        Ok(contract)
    }

    // Parse ABI and cache it
    async fn parse_and_cache_abi(&self, abi_key: &str, abi_json: &str) -> Result<ethers::abi::Abi> {
        let abi: ethers::abi::Abi = serde_json::from_str(abi_json)
            .map_err(|e| anyhow!(ContractError::AbiError(e.to_string())))?;

        if let Ok(mut cache) = self.abi_cache.write() {
            cache.insert(abi_key.to_string(), CacheEntry::new(abi.clone(), 86400)); // 24 hours
        }

        Ok(abi)
    }

    // Get token info
    pub async fn get_token_info(&self, address: Address) -> Result<ContractInfo> {
        // Check cache
        if let Ok(info_cache) = self.contracts_info.read() {
            if let Some(info) = info_cache.get(&address) {
                return Ok(info.clone());
            }
        }

        // Get the ERC20 contract
        let contract = self.get_erc20(address).await?;

        // Build token info
        let mut info = ContractInfo {
            address,
            name: "Unknown".to_string(),
            symbol: None,
            decimals: None,
            contract_type: ContractType::Unknown,
            verified: false,
            chain_id: 0,
            created_at: 0,
            bytecode_hash: None,
        };

        // Try to get basic token info
        if let Ok(name) = contract.method::<_, String>("name", ()).call().await {
            info.name = name;
        }

        if let Ok(symbol) = contract.method::<_, String>("symbol", ()).call().await {
            info.symbol = Some(symbol);
        }

        if let Ok(decimals) = contract.method::<_, u8>("decimals", ()).call().await {
            info.decimals = Some(decimals);
        }

        // Try to determine contract type
        if info.symbol.is_some() && info.decimals.is_some() {
            info.contract_type = ContractType::ERC20;
        }

        // Get chain ID
        if let Ok(chain_id) = self.provider.get_chainid().await {
            info.chain_id = chain_id.as_u64();
        }

        // Cache the info
        if let Ok(mut cache) = self.contracts_info.write() {
            cache.insert(address, info.clone());
        }

        Ok(info)
    }

    // Clean expired cache entries
    pub fn cleanup_cache(&self) {
        // Clean contract cache
        if let Ok(mut cache) = self.contract_cache.write() {
            cache.retain(|_, entry| !entry.is_expired());
        }

        // Clean ABI cache
        if let Ok(mut cache) = self.abi_cache.write() {
            cache.retain(|_, entry| !entry.is_expired());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_cache_entry() {
        let value = "test";
        let entry = CacheEntry::new(value, 1);
        assert!(!entry.is_expired());

        // Wait for expiration
        std::thread::sleep(Duration::from_secs(2));
        assert!(entry.is_expired());
    }

    #[test]
    fn test_contract_type() {
        assert_ne!(ContractType::ERC20, ContractType::Unknown);
        assert_eq!(ContractType::ERC20, ContractType::ERC20);
    }
}
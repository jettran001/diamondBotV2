use std::collections::HashMap;
use std::sync::Arc;
use ethers::prelude::*;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub name: String,
    pub rpc_url: String,
    pub router_address: String,
    pub weth_address: String,
    pub block_time: u64, // milliseconds
    pub explorer_url: String,
}

pub struct MultiChainManager {
    chains: HashMap<u64, Arc<ChainConfig>>,
    providers: HashMap<u64, Arc<Provider<Http>>>,
    current_chain_id: u64,
}

impl MultiChainManager {
    pub fn new() -> Self {
        Self {
            chains: HashMap::new(),
            providers: HashMap::new(),
            current_chain_id: 1, // Default to Ethereum mainnet
        }
    }
    
    pub fn add_chain(&mut self, config: ChainConfig) -> Result<(), Box<dyn std::error::Error>> {
        let chain_id = config.chain_id;
        
        // Create provider for this chain
        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        
        // Store the config and provider
        self.chains.insert(chain_id, Arc::new(config));
        self.providers.insert(chain_id, Arc::new(provider));
        
        Ok(())
    }
    
    pub fn set_current_chain(&mut self, chain_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        if !self.chains.contains_key(&chain_id) {
            return Err(format!("Chain with ID {} not found", chain_id).into());
        }
        
        self.current_chain_id = chain_id;
        Ok(())
    }
    
    pub fn get_current_chain(&self) -> Result<Arc<ChainConfig>, Box<dyn std::error::Error>> {
        self.chains.get(&self.current_chain_id)
            .ok_or_else(|| format!("Current chain with ID {} not found", self.current_chain_id).into())
            .map(Arc::clone)
    }
    
    pub fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>, Box<dyn std::error::Error>> {
        self.providers.get(&chain_id)
            .ok_or_else(|| format!("Provider for chain ID {} not found", chain_id).into())
            .map(Arc::clone)
    }
    
    pub fn get_current_provider(&self) -> Result<Arc<Provider<Http>>, Box<dyn std::error::Error>> {
        self.get_provider(self.current_chain_id)
    }
    
    pub fn get_all_chains(&self) -> Vec<Arc<ChainConfig>> {
        self.chains.values().cloned().collect()
    }
}

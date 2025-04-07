use std::collections::HashMap;
use crate::chain_adapters::ChainConfig;

pub fn get_predefined_chains() -> HashMap<String, ChainConfig> {
    let mut chains = HashMap::new();
    
    // Ethereum Mainnet
    chains.insert("ethereum".to_string(), ChainConfig {
        name: "Ethereum".to_string(),
        chain_id: 1,
        rpc_url: "https://eth.llamarpc.com".to_string(),
        native_symbol: "ETH".to_string(),
        wrapped_native_token: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH
        router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2 Router
        factory_address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".to_string(), // Uniswap V2 Factory
        explorer_url: "https://etherscan.io".to_string(),
        block_time: 12000, // ~12 seconds
        default_gas_limit: 500000,
        default_gas_price: 20000000000, // 20 Gwei
        eip1559_supported: true,
        max_priority_fee: Some(2000000000), // 2 Gwei
    });
    
    // Binance Smart Chain
    chains.insert("bsc".to_string(), ChainConfig {
        name: "Binance Smart Chain".to_string(),
        chain_id: 56,
        rpc_url: "https://bsc-dataseed.binance.org".to_string(),
        native_symbol: "BNB".to_string(),
        wrapped_native_token: "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(),
        router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2 Router
        factory_address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".to_string(), // Uniswap V2 Factory
        explorer_url: "https://bscscan.com".to_string(),
        block_time: 12000, // ~12 seconds
        default_gas_limit: 500000,
        default_gas_price: 20000000000, // 20 Gwei
        eip1559_supported: true,
        max_priority_fee: Some(2000000000), // 2 Gwei
    });
    
    chains
}

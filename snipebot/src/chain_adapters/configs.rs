use super::base::ChainConfig;
use lazy_static::lazy_static;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Định nghĩa các cấu hình sẵn có cho từng blockchain
lazy_static! {
    pub static ref CHAIN_CONFIGS: HashMap<&'static str, ChainConfig> = {
        let mut m = HashMap::new();
        
        // Ethereum Mainnet
        m.insert("ethereum", ChainConfig {
            name: "Ethereum".to_string(),
            chain_id: 1,
            rpc_url: "https://mainnet.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(), // WETH
            router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2 Router
            factory_address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".to_string(), // Uniswap V2 Factory
            explorer_url: "https://etherscan.io".to_string(),
            block_time: 12000, // 12 giây
            default_gas_limit: 250000,
            default_gas_price: 20.0, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(1.5), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });

        // Binance Smart Chain
        m.insert("bsc", ChainConfig {
            name: "Binance Smart Chain".to_string(),
            chain_id: 56,
            rpc_url: "https://bsc-dataseed.binance.org".to_string(),
            native_symbol: "BNB".to_string(),
            wrapped_native_token: "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(), // WBNB
            router_address: "0x10ED43C718714eb63d5aA57B78B54704E256024E".to_string(), // PancakeSwap Router
            factory_address: "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73".to_string(), // PancakeSwap Factory
            explorer_url: "https://bscscan.com".to_string(),
            block_time: 3000, // 3 giây
            default_gas_limit: 300000,
            default_gas_price: 5.0, // gwei
            eip1559_supported: false,
            max_priority_fee: None,
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });

        // Avalanche C-Chain
        m.insert("avalanche", ChainConfig {
            name: "Avalanche".to_string(),
            chain_id: 43114,
            rpc_url: "https://api.avax.network/ext/bc/C/rpc".to_string(),
            native_symbol: "AVAX".to_string(),
            wrapped_native_token: "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7".to_string(), // WAVAX
            router_address: "0x60aE616a2155Ee3d9A68541Ba4544862310933d4".to_string(), // TraderJoe Router
            factory_address: "0x9Ad6C38BE94206cA50bb0d90783181662f0Cfa10".to_string(), // TraderJoe Factory
            explorer_url: "https://snowtrace.io".to_string(),
            block_time: 2000, // 2 giây
            default_gas_limit: 300000,
            default_gas_price: 25.0, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(2.0), // gwei
            eth_to_token_swap_fn: "swapExactAVAXForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForAVAX".to_string(),
        });

        // Base (Coinbase L2)
        m.insert("base", ChainConfig {
            name: "Base".to_string(),
            chain_id: 8453,
            rpc_url: "https://mainnet.base.org".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0x4200000000000000000000000000000000000006".to_string(), // WETH on Base
            router_address: "0xfCA736a42EE6f1BF35afDeFa3B262a4B0C4D3E6e".to_string(), // BaseSwap Router
            factory_address: "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".to_string(), // BaseSwap Factory
            explorer_url: "https://basescan.org".to_string(),
            block_time: 2000, // 2 giây
            default_gas_limit: 300000,
            default_gas_price: 1.0, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(0.5), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });

        // Arbitrum
        m.insert("arbitrum", ChainConfig {
            name: "Arbitrum".to_string(),
            chain_id: 42161,
            rpc_url: "https://arb1.arbitrum.io/rpc".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(), // WETH on Arbitrum
            router_address: "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".to_string(), // SushiSwap Router
            factory_address: "0xc35DADB65012eC5796536bD9864eD8773aBc74C4".to_string(), // SushiSwap Factory
            explorer_url: "https://arbiscan.io".to_string(),
            block_time: 500, // 0.5 giây
            default_gas_limit: 1000000,
            default_gas_price: 0.1, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(0.05), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });

        // Optimism
        m.insert("optimism", ChainConfig {
            name: "Optimism".to_string(),
            chain_id: 10,
            rpc_url: "https://mainnet.optimism.io".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0x4200000000000000000000000000000000000006".to_string(), // WETH on Optimism
            router_address: "0x9c12939390052919aF3155f41Bf4160Fd3666A6f".to_string(), // Velodrome Router
            factory_address: "0x25CbdDb98b35ab1FF77413456B31EC81A6B6B746".to_string(), // Velodrome Factory
            explorer_url: "https://optimistic.etherscan.io".to_string(),
            block_time: 2000, // 2 giây
            default_gas_limit: 1000000,
            default_gas_price: 0.001, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(0.0005), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });

        // Polygon (Matic)
        m.insert("polygon", ChainConfig {
            name: "Polygon".to_string(),
            chain_id: 137,
            rpc_url: "https://polygon-rpc.com".to_string(),
            native_symbol: "MATIC".to_string(),
            wrapped_native_token: "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270".to_string(), // WMATIC
            router_address: "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff".to_string(), // QuickSwap Router
            factory_address: "0x5757371414417b8C6CAad45bAeF941aBc7d3Ab32".to_string(), // QuickSwap Factory
            explorer_url: "https://polygonscan.com".to_string(),
            block_time: 2000, // 2 giây
            default_gas_limit: 500000,
            default_gas_price: 50.0, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(30.0), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });
        
        // Monad
        m.insert("monad", ChainConfig {
            name: "Monad".to_string(),
            chain_id: 1284, // Chain ID tạm thời của Monad
            rpc_url: "https://rpc.monad.xyz".to_string(), // RPC URL tạm thời
            native_symbol: "MONAD".to_string(),
            wrapped_native_token: "0x2C1b868d6596a18e32E61B901E4060C872647b6C".to_string(), // WMONAD address (tạm thời)
            router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(), // Uniswap V2 like router on Monad
            factory_address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".to_string(), // Uniswap V2 like factory on Monad
            explorer_url: "https://explorer.monad.xyz".to_string(),
            block_time: 1000, // 1 giây
            default_gas_limit: 300000,
            default_gas_price: 0.1, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(0.05), // gwei
            eth_to_token_swap_fn: "swapExactETHForTokens".to_string(),
            token_to_eth_swap_fn: "swapExactTokensForETH".to_string(),
        });
        
        m
    };
}

/// Hàm lấy cấu hình cho một chain cụ thể
pub fn get_chain_config(chain_name: &str) -> Option<ChainConfig> {
    CHAIN_CONFIGS.get(chain_name.to_lowercase().as_str()).cloned()
}

/// Hàm lấy danh sách tên các chain được hỗ trợ
pub fn get_supported_chains() -> Vec<&'static str> {
    CHAIN_CONFIGS.keys().cloned().collect()
}

/// Hàm tạo cấu hình custom chain
pub fn create_custom_chain_config(
    name: &str,
    chain_id: u64,
    rpc_url: &str,
    native_symbol: &str,
    wrapped_native_token: &str,
    router_address: &str,
    factory_address: &str,
    explorer_url: &str,
    eip1559_supported: bool,
    eth_to_token_swap_fn: &str,
    token_to_eth_swap_fn: &str,
) -> ChainConfig {
    ChainConfig {
        name: name.to_string(),
        chain_id,
        rpc_url: rpc_url.to_string(),
        native_symbol: native_symbol.to_string(),
        wrapped_native_token: wrapped_native_token.to_string(),
        router_address: router_address.to_string(),
        factory_address: factory_address.to_string(),
        explorer_url: explorer_url.to_string(),
        block_time: 12000, // Giá trị mặc định, có thể điều chỉnh sau
        default_gas_limit: 300000,
        default_gas_price: 10.0, // gwei
        eip1559_supported,
        max_priority_fee: if eip1559_supported { Some(1.0) } else { None },
        eth_to_token_swap_fn: eth_to_token_swap_fn.to_string(),
        token_to_eth_swap_fn: token_to_eth_swap_fn.to_string(),
    }
}

#[macro_export]
macro_rules! define_chain_configs {
    // ... existing code ...
} 
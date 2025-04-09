// Diamondchain - Copyright (c) 2023

// External imports
use ethers::{
    providers::Provider,
    types::{Address, U256, H256, TransactionReceipt},
    contract::Contract,
};

// Standard library imports
use std::{
    sync::{Arc, Mutex, RwLock},
    collections::HashMap,
    time::{Duration, Instant},
};

// Internal imports
use crate::{
    chain_adapters::{ChainAdapter, AsyncChainAdapter, ChainConfig},
    types::{
        WalletBalance, TokenBalance, NetworkStats, SystemStats,
        TradeConfig, TradeResult, TradeType
    },
    error_handling::{TransactionError, SnipeBotError},
};

// Common crate imports - Sử dụng hệ thống cache thống nhất
use common::cache;
// Thêm common middleware
use common::middleware;

// Blockchain crate imports
use diamond_blockchain::core;

// Third party imports
use anyhow::Result;
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use prometheus::{Registry, Counter, Gauge};

// Public modules
pub mod types;
pub mod abi_utils;
pub mod utils;
pub mod chain_adapters;
pub mod wallet;
pub mod blockchain; // Giữ lại facade wrapper để tương thích
pub mod network;
pub mod snipebot;
pub mod middleware; // Giữ lại để tương thích, nhưng deprecated
pub mod error_handling;
pub mod api;
pub mod risk_analyzer;
pub mod wallet;
pub mod mempool;
pub mod ai;
pub mod module_manager;
pub mod health_monitor;
pub mod trade_executor;
pub mod subscription_manager;
// Re-exports
pub use {
    types::*,
    abi_utils::*,
    utils::*,
    chain_adapters::*,
    wallet::*,
    // Re-export từ facade wrapper
    blockchain::*,
    network::*,
    snipebot::*,
    // Middleware được re-export từ common
    common::middleware::*,
    error_handling::*,
    api::*,
    risk_analyzer::*
};

// Re-export từ diamond_blockchain
pub use diamond_blockchain::core::{
    BlockchainParams,
    TokenInfo,
    ContractInfo,
    ContractManager,
    BlockchainService,
    create_blockchain_service,
};

// Internal modules
pub mod auto_tuning;
pub mod config;
pub mod contracts;
pub mod gas_optimizer;
pub mod mempool;
pub mod rate_limit;
pub mod storage;
pub mod token_status;

// Re-export core types
pub use config::{Config, NetworkConfig};
pub use snipebot::SnipeBot;
pub use storage::Storage;
pub use error_handling::TransactionError;
pub use trade::trade_logic::{TradeResult, TradeType, TradeManager};
pub use token_status::TokenStatus;
pub use utils::{try_with_timeout, try_lock_with_timeout, RetryConfig};
pub use chain_adapters::nonce_manager;

// Re-export chain adapter types
pub use chain_adapters::{
    ChainAdapter,
    AsyncChainAdapter,
    ChainConfig,
    GasInfo,
    BlockInfo,
    NodeInfo,
};

// Re-export service types
pub use blockchain::{
    BlockchainService,
    TransactionService,
};

pub use network::{
    NetworkService,
    ConnectionPool,
};

pub use storage::{
    StorageService,
    CacheService,
};

pub use risk_analyzer::{
    RiskAnalyzer,
    TokenRiskAnalysis,
};

// Re-export core functionality
pub use retry_policy::RetryPolicy;

// Testing utilities
#[cfg(test)]
pub mod tests {
    use super::*;
    use env_logger;
    
    pub fn init_test_logger() {
        let _ = env_logger::builder().is_test(true).try_init();
    }
}

// Đổi từ log sang tracing
#[macro_use]
extern crate tracing;

// Re-exports from the new modules
pub use chain_adapters::retry;
pub use chain_adapters::configs;
pub use chain_adapters::base::ChainAdapterEnum;

// Export các module cần thiết
pub use chain_adapters::trait_adapter::ChainAdapter;
pub use crate::error_handling::SnipeBotError as Error;
pub use crate::service::SnipeService;
pub use crate::utils::safe_now;

// Re-export các module phụ thuộc
pub mod cache;
pub use common::cache;

// Re-exports from the new modules đã được gom nhóm ở trên
// Loại bỏ các re-export trùng lặp 

// Re-export các module phụ thuộc
pub mod cache;
pub use common::cache; 
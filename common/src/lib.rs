// External imports
use ethers::core::types::{Address, H256, U256};

// Re-exports for common crate
pub mod prelude {
    // External types
    pub use ethers::core::types::{Address, H256, U256};
    
    // Common modules
    pub use crate::types;
    pub use crate::error;
    pub use crate::utils;
    pub use crate::chain_adapter;
    
    // Re-export cache từ thư mục gốc của crate
    pub use crate::cache;
    
    // Re-export middleware mới
    pub use crate::middleware;
    
    // Re-export error_handling
    pub use crate::error::*;
    
    // Re-export error_handling từ thư mục gốc của crate
    pub use crate::error_handling;
}

// Import các module từ thư mục gốc của crate
// Thêm đường dẫn tương đối cho các module này
#[path = "../cache.rs"]
pub mod cache;

#[path = "../error_handling.rs"]
pub mod error_handling;

// Modules paths - tất cả các module trong src
pub mod types;
pub mod utils;
pub mod chain_adapter;
pub mod error;
pub mod worker;
pub mod validator;
pub mod executor;
pub mod scheduler;
pub mod state_manager;
pub mod task_manager;
pub mod models;
pub mod network;
pub mod middleware;
pub mod retry_policy;
pub mod security;
pub mod logger;
pub mod metrics;
pub mod config;
pub mod event_handler;
pub mod diamond_manager;
pub mod ai;
pub mod user;
pub mod monte_equilibrium;
pub mod equilibrium;

// Re-exports
pub use chain_adapter::{
    ChainAdapter,
    TransactionReceipt,
    Log,
    Block,
    Transaction,
};
pub use error::{CommonError, CommonResult};
pub use types::{
    TradingParams,
    TradingStrategy,
    AIModelConfig,
};
pub use models::{
    TokenInfo,
    PairInfo,
    TradeInfo,
    OrderInfo,
};
pub use utils::*;

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modules() {
        assert!(true);
    }
} 
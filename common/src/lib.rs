// Internal modules
mod chain_adapter;
mod error;
mod middleware;
mod models;
mod network;
mod types;
mod utils;

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
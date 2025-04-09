// External imports
use ethers::{
    abi::{Abi, Function, Event},
    contract::Contract,
    middleware::Middleware,
    types::{Address, H256, U256, Bytes, Filter, Log, AccessList},
    utils::hex,
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
pub mod abi;
pub mod marketplace;
pub mod nft;
pub mod smartcontract;
pub mod mining;
pub mod grpc;
pub mod farming;
pub mod diamond_token;
pub mod depin;
pub mod crosschain;
pub mod core;

// Re-export các module chính
pub use abi::*;
pub use marketplace::*;
pub use nft::*;
pub use smartcontract::*;
pub use mining::*;
pub use grpc::*;
pub use farming::*;
pub use diamond_token::*;
pub use depin::*;
pub use crosschain::*;
pub use core::*;

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test khởi tạo các module
    #[test]
    fn test_modules() {
        assert!(true); // Placeholder test
    }
} 
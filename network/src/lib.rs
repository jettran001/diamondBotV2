
// External imports
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use thiserror::Error;

// Module exports
pub mod core;
pub use core::*;

// Re-export sub-modules
pub use self::core::{
    NetworkConfig, 
    NetworkError, 
    NetworkResult,
    models,
    error,
    types,
    utils,
    config,
    message
};

#[cfg(test)]
mod tests {
    #[test]
    fn test_network_module() {
        assert!(true);
    }
}

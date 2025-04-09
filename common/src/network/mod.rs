
// External imports
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};
use thiserror::Error;

// Module exports
pub mod core;
pub mod models;
pub mod error;
pub mod server;
pub mod types;
pub mod utils;
pub mod config;
pub mod message;

// Re-export sub-modules
pub use self::core::*;
pub use self::models::*;
pub use self::error::*;
pub use self::server::*;
pub use self::types::*;
pub use self::utils::*;
pub use self::config::*;
pub use self::message::*;

#[cfg(test)]
mod tests {
    #[test]
    fn test_network_module() {
        assert!(true);
    }
}

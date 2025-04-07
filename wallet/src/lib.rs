// Module exports
mod wallet;
mod secure_storage;
pub mod config;
pub mod defi;
pub mod mission;
pub mod stake;
pub mod farm;

// Re-export các component chính
pub use wallet::{
    WalletManager, 
    WalletManagerConfig, 
    WalletClientExt,
};

pub use secure_storage::{
    StorageConfig, 
    SecureWalletStorage, 
    WalletInfo, 
    EncryptedData,
    SafeWalletView
};

// Re-export ABI từ blockchain
pub use diamond_blockchain::abi; 
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
    fs,
    path::Path,
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::{timeout, sleep};
use serde_json;

// Internal imports
pub mod abis;
pub use abis::*;

/// Đọc ABI từ file JSON
/// 
/// # Arguments
/// 
/// * `file_path` - Đường dẫn đến file JSON chứa ABI
/// 
/// # Returns
/// 
/// * `Result<Abi>` - ABI đã được parse từ file JSON
/// 
/// # Errors
/// 
/// * `anyhow::Error` - Lỗi khi đọc file hoặc parse JSON
pub fn read_abi_from_file(file_path: &str) -> Result<Abi> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read ABI file: {}", file_path))?;
    let abi = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse ABI from file: {}", file_path))?;
    Ok(abi)
}

/// Lưu ABI vào file JSON
/// 
/// # Arguments
/// 
/// * `file_path` - Đường dẫn đến file JSON để lưu ABI
/// * `abi` - ABI cần lưu
/// 
/// # Returns
/// 
/// * `Result<()>` - Kết quả thực hiện
/// 
/// # Errors
/// 
/// * `anyhow::Error` - Lỗi khi ghi file hoặc serialize JSON
pub fn save_abi_to_file(file_path: &str, abi: &Abi) -> Result<()> {
    let content = serde_json::to_string_pretty(abi)
        .with_context(|| format!("Failed to serialize ABI to JSON"))?;
    fs::write(file_path, content)
        .with_context(|| format!("Failed to write ABI to file: {}", file_path))?;
    Ok(())
}

/// Kiểm tra xem file có phải là file ABI hợp lệ không
/// 
/// # Arguments
/// 
/// * `file_path` - Đường dẫn đến file cần kiểm tra
/// 
/// # Returns
/// 
/// * `Result<bool>` - `true` nếu là file ABI hợp lệ, `false` nếu không
/// 
/// # Errors
/// 
/// * `anyhow::Error` - Lỗi khi đọc file hoặc parse JSON
pub fn is_valid_abi_file(file_path: &str) -> Result<bool> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read ABI file: {}", file_path))?;
    match serde_json::from_str::<Abi>(&content) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Test đọc ABI từ file
    #[test]
    fn test_read_abi_from_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path().to_str().unwrap();
        
        let abi = Abi::default();
        save_abi_to_file(file_path, &abi).unwrap();
        
        let read_abi = read_abi_from_file(file_path).unwrap();
        assert_eq!(read_abi, abi);
    }

    /// Test lưu ABI vào file
    #[test]
    fn test_save_abi_to_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path().to_str().unwrap();
        
        let abi = Abi::default();
        assert!(save_abi_to_file(file_path, &abi).is_ok());
        
        let content = std::fs::read_to_string(file_path).unwrap();
        assert!(!content.is_empty());
    }

    /// Test kiểm tra file ABI hợp lệ
    #[test]
    fn test_is_valid_abi_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path().to_str().unwrap();
        
        let abi = Abi::default();
        save_abi_to_file(file_path, &abi).unwrap();
        
        assert!(is_valid_abi_file(file_path).unwrap());
    }
} 
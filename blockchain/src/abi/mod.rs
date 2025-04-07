// Xóa các import không sử dụng
// use std::path::Path;
// use ethers::prelude::*;

use std::fs;
use anyhow::Result;

// Re-export module abis từ file abis.rs
pub mod abis;
pub use abis::*;

// Hàm tiện ích để đọc ABI từ file
pub fn read_abi_from_file(file_path: &str) -> Result<ethers::abi::Abi> {
    let content = fs::read_to_string(file_path)?;
    let abi = serde_json::from_str(&content)?;
    Ok(abi)
}

// Thêm các hàm tiện ích khác cần thiết cho ABI trong tương lai 
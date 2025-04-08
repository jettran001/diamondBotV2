// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
    fmt::{Display, Formatter},
};

// Third party imports
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::future::Future;
use tokio;

/// Lấy thời gian hiện tại (milliseconds)
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Tạo ID ngẫu nhiên
pub fn random_id() -> String {
    Uuid::new_v4().to_string()
}

/// Kiểm tra node có online không
pub fn is_node_online(last_seen: DateTime<Utc>, timeout: Duration) -> bool {
    let now = Utc::now();
    let diff = now - last_seen;
    diff.num_seconds() < timeout.as_secs() as i64
}

/// Mã hóa/giải mã XOR
pub fn xor_encrypt_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    for (i, &b) in data.iter().enumerate() {
        result.push(b ^ key[i % key.len()]);
    }
    result
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test current_timestamp
    #[test]
    fn test_current_timestamp() {
        let timestamp = current_timestamp();
        assert!(timestamp > 0);
    }

    /// Test random_id
    #[test]
    fn test_random_id() {
        let id = random_id();
        assert!(!id.is_empty());
    }

    /// Test is_node_online
    #[test]
    fn test_is_node_online() {
        let last_seen = Utc::now();
        let timeout = Duration::from_secs(60);
        assert!(is_node_online(last_seen, timeout));
    }

    /// Test xor_encrypt_decrypt
    #[test]
    fn test_xor_encrypt_decrypt() {
        let data = b"test";
        let key = b"key";
        let encrypted = xor_encrypt_decrypt(data, key);
        let decrypted = xor_encrypt_decrypt(&encrypted, key);
        assert_eq!(data, decrypted.as_slice());
    }
}

pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Kiểm tra timeout
pub fn is_timeout(diff: chrono::Duration, timeout: std::time::Duration) -> bool {
    diff.num_seconds() < timeout.as_secs() as i64
}

/// Đợi điều kiện
pub async fn wait_for_condition<F, R>(
    condition: F,
    timeout: Duration,
    interval: Duration,
) -> Result<R>
where
    F: Fn() -> Box<dyn Future<Output = Result<Option<R>>> + Send + Unpin>,
    R: Send + 'static,
{
    let start = Utc::now();
    let _timeout_seconds = timeout.as_secs() as i64;
    
    loop {
        let future = condition();
        if let Some(result) = future.await? {
            return Ok(result);
        }
        
        if is_timeout(start.signed_duration_since(start), timeout) {
            return Err(anyhow::anyhow!("Timeout waiting for condition"));
        }
        
        tokio::time::sleep(interval).await;
    }
}

pub async fn retry_with_timeout<F, R, E>(
    f: F,
    max_retries: u32,
    timeout: Duration,
) -> Result<R, E>
where
    F: Fn() -> Box<dyn Future<Output = Result<R, E>> + Send + Unpin>,
    E: std::fmt::Debug + std::convert::From<std::string::String>,
{
    let mut retries = 0;
    let start = Utc::now();

    while retries < max_retries {
        let diff = Utc::now() - start;
        if diff.num_seconds() < timeout.as_secs() as i64 {
            return Err(format!("Operation timed out after {} retries", retries).into());
        }

        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                retries += 1;
                if retries >= max_retries {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(100 * retries as u64)).await;
            }
        }
    }

    Err(format!("Operation failed after {} retries", max_retries).into())
} 
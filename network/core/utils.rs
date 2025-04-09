// External imports
use anyhow::{Result, Context};

// Standard library imports
use std::{
    time::{SystemTime, UNIX_EPOCH},
    net::{IpAddr, SocketAddr},
    str::FromStr,
};

// Third party imports
use rand::{thread_rng, Rng};
use hex;
use tokio::time::sleep;

/// Lấy thời gian hiện tại dưới dạng timestamp (số giây từ epoch)
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Kiểm tra một địa chỉ có hợp lệ không
pub fn is_valid_address(address: &str) -> bool {
    if !address.contains(':') || address.split(':').count() != 2 {
        return false;
    }
    
    let parts: Vec<&str> = address.split(':').collect();
    let ip = parts[0];
    let port = parts[1];
    
    // Kiểm tra IP
    if IpAddr::from_str(ip).is_err() {
        return false;
    }
    
    // Kiểm tra port
    if let Ok(port) = port.parse::<u16>() {
        port > 0 && port <= 65535
    } else {
        false
    }
}

/// Tạo một định danh ngẫu nhiên
pub fn random_id() -> String {
    let random_bytes: [u8; 16] = thread_rng().gen();
    hex::encode(random_bytes)
}

/// Kiểm tra xem một node có đang online không
pub fn is_node_online(last_heartbeat: u64, timeout_seconds: u64) -> bool {
    let now = current_timestamp();
    now - last_heartbeat < timeout_seconds
}

/// Mã hóa/giải mã đơn giản cho dữ liệu (XOR với key)
pub fn xor_encrypt_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(key.iter().cycle())
        .map(|(d, k)| d ^ k)
        .collect()
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

    /// Test is_valid_address
    #[test]
    fn test_is_valid_address() {
        assert!(is_valid_address("127.0.0.1:8080"));
        assert!(!is_valid_address("invalid"));
        assert!(!is_valid_address("127.0.0.1:invalid"));
        assert!(!is_valid_address("invalid:8080"));
    }

    /// Test random_id
    #[test]
    fn test_random_id() {
        let id1 = random_id();
        let id2 = random_id();
        assert_eq!(id1.len(), 32);
        assert_eq!(id2.len(), 32);
        assert_ne!(id1, id2);
    }

    /// Test is_node_online
    #[test]
    fn test_is_node_online() {
        let now = current_timestamp();
        assert!(is_node_online(now, 60));
        assert!(!is_node_online(now - 61, 60));
    }

    /// Test xor_encrypt_decrypt
    #[test]
    fn test_xor_encrypt_decrypt() {
        let data = b"hello";
        let key = b"key";
        let encrypted = xor_encrypt_decrypt(data, key);
        let decrypted = xor_encrypt_decrypt(&encrypted, key);
        assert_eq!(data, decrypted.as_slice());
    }
} 
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use sha2::{Sha256, Digest};
use zeroize::{Zeroize, ZeroizeOnDrop};
use rand::{rngs::OsRng, RngCore};
use anyhow::{Result, Context, anyhow};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc, Duration};
use aes_gcm::{
    aead::{Aead, KeyInit, generic_array::GenericArray},
    Aes256Gcm,
};
use anyhow::{anyhow, Result, Context};
use argon2::password_hash::SaltString;
use ethers::{
    prelude::LocalWallet,
    signers::Signer
};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;
use zeroize::{Zeroize, ZeroizeOnDrop};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::PasswordHasher,
};


// Cache entry standard structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub value: T,
    pub expires_at: DateTime<Utc>,
}

impl<T> CacheEntry<T> {
    pub fn new(value: T, ttl_seconds: i64) -> Self {
        Self {
            value,
            expires_at: Utc::now() + Duration::seconds(ttl_seconds),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

// Sensitive data container with automatic zeroing
#[derive(Zeroize, ZeroizeOnDrop, Serialize, Deserialize)]
pub struct SensitiveData {
    #[zeroize(skip)]
    pub identifier: String,
    pub data: Vec<u8>,
}

impl SensitiveData {
    pub fn new(identifier: &str, data: Vec<u8>) -> Self {
        Self {
            identifier: identifier.to_string(),
            data,
        }
    }
}

// Security error types
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Password error: {0}")]
    PasswordError(String),

    #[error("Format error: {0}")]
    FormatError(String),

    #[error("Key derivation error: {0}")]
    KeyDerivationError(String),
}

// Cấu trúc dữ liệu ví bảo mật
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_private_key: Option<EncryptedData>, 
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_mnemonic: Option<EncryptedData>, 
    pub chain_id: u64,
    pub created_at: u64,
    pub last_used: u64,
    pub balance: Option<String>,
    pub name: Option<String>,
    pub tags: Vec<String>,
    pub is_hardware: bool,
}

// Cấu trúc dữ liệu mã hóa
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub salt: Vec<u8>,
    pub version: u8,  // Phiên bản của thuật toán mã hóa
}

// View an toàn của wallet để hiển thị
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeWalletView {
    pub address: String,
    pub chain_id: u64,
    pub created_at: u64,
    pub last_used: u64,
    pub balance: Option<String>,
    pub has_private_key: bool,
    pub has_mnemonic: bool,
    pub name: Option<String>,
    pub tags: Vec<String>,
    pub is_hardware: bool,
}


// Secure storage manager
pub struct SecureWalletStorage {
    storage_path: PathBuf,
    // Cache for loaded wallets
    wallet_cache: Arc<RwLock<HashMap<String, CacheEntry<SensitiveData>>>>,
    salt: [u8; 16],
    wallets: HashMap<String, WalletInfo>,
    current_key: EncryptionKey,

}

impl SecureWalletStorage {
    // Create a new secure storage
    pub fn new(storage_path: PathBuf, config: &StorageConfig) -> Result<Self> {
        let salt_vec = config.encryption_salt.as_bytes().to_vec();
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&salt_vec[0..16]);

        fs::create_dir_all(&storage_path)
            .with_context(|| format!("Failed to create storage directory at {:?}", storage_path))?;


        let key_id = format!("key-{}", Self::rand_hex_string(8));
        let key = Self::generate_encryption_key("default", &salt)?;
        let mut wallets = HashMap::new();

        // Tải ví từ đĩa nếu tệp tồn tại
        let wallet_path = storage_path.clone().join(&config.wallet_filename);
        if wallet_path.exists() {
            if let Ok(loaded_wallets) = load_wallets(&wallet_path) {
                for wallet in loaded_wallets {
                    wallets.insert(wallet.address.clone(), wallet);
                }
            }
        }

        Ok(Self {
            storage_path,
            wallet_cache: Arc::new(RwLock::new(HashMap::new())),
            salt,
            wallets,
            current_key: EncryptionKey {
                key_id,
                key,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        })
    }

    // Store sensitive data securely
    pub fn store_wallet(&mut self, wallet_info: &WalletInfo, password: &str) -> Result<()> {
        let key = self.derive_key(password)?;
        let identifier = wallet_info.address.clone();
        let data = serde_json::to_vec(wallet_info)?;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| SecurityError::EncryptionError(e.to_string()))?;

        let encrypted_data = cipher.encrypt(nonce, Payload {
            msg: &data,
            aad: identifier.as_bytes(),
        })
        .map_err(|e| SecurityError::EncryptionError(e.to_string()))?;

        let mut file_content = Vec::with_capacity(nonce_bytes.len() + encrypted_data.len());
        file_content.extend_from_slice(&nonce_bytes);
        file_content.extend_from_slice(&encrypted_data);

        let file_path = self.get_file_path(&identifier);
        fs::write(&file_path, &file_content)
            .with_context(|| format!("Failed to write encrypted data to {:?}", file_path))?;

        Ok(())
    }

    // Load sensitive data
    pub fn load_wallet(&self, address: &str, password: &str) -> Result<WalletInfo> {
        let file_path = self.get_file_path(address);
        let file_content = fs::read(&file_path)
            .with_context(|| format!("Failed to read encrypted data from {:?}", file_path))?;

        if file_content.len() < 12 {
            return Err(anyhow!(SecurityError::FormatError(
                "Encrypted file is too short".to_string()
            )));
        }

        let (nonce_bytes, encrypted_data) = file_content.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let key = self.derive_key(password)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| SecurityError::DecryptionError(e.to_string()))?;

        let decrypted_data = cipher.decrypt(nonce, Payload {
            msg: encrypted_data,
            aad: address.as_bytes(),
        })
        .map_err(|e| SecurityError::DecryptionError(e.to_string()))?;

        let wallet_info: WalletInfo = serde_json::from_slice(&decrypted_data)?;
        Ok(wallet_info)
    }

    // Delete sensitive data
    pub fn delete_wallet(&self, address: &str) -> Result<()> {
        let file_path = self.get_file_path(address);

        if file_path.exists() {
            fs::remove_file(&file_path)
                .with_context(|| format!("Failed to delete file at {:?}", file_path))?;
        }

        Ok(())
    }

    // Helper: Get file path for an identifier
    fn get_file_path(&self, identifier: &str) -> PathBuf {
        self.storage_path.join(format!("{}.bin", identifier))
    }

    // Helper: Derive encryption key from password
    fn derive_key(&self, password: &str) -> Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.update(&self.salt);

        let result = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&result);

        Ok(key)
    }
    /// Tạo hàm trợ giúp tạo chuỗi hex ngẫu nhiên
    fn rand_hex_string(len: usize) -> String {
        let mut bytes = vec![0u8; len];
        OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }
    
     /// Tạo khóa từ mật khẩu sử dụng Argon2
    fn generate_encryption_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
        // Tạo một Argon2 context
        let argon2 = Argon2::new(
            Algorithm::Argon2id, 
            Version::V0x13, 
            Params::new(4096, 3, 4, Some(32)).unwrap()
        );
        
        // Tạo salt string từ slice
        let mut random_salt = [0u8; 16];
        OsRng.fill_bytes(&mut random_salt);
        
        // Đảm bảo có một số byte từ salt ban đầu trong random_salt
        for i in 0..std::cmp::min(salt.len(), 8) {
            random_salt[i] ^= salt[i];  // XOR để kết hợp giá trị
        }
        
        // Sử dụng từ khóa từ tài liệu của password-hash
        let salt_str = SaltString::encode_b64(&random_salt)
            .map_err(|e| anyhow!("Failed to encode salt: {}", e))?;
        
        // Hash mật khẩu
        let hash = argon2
            .hash_password(password.as_bytes(), &salt_str)
            .map_err(|e| anyhow!("Failed to hash password: {}", e))?;
            
        // Lấy bytes từ hash
        let hash_wrapped = hash.hash.unwrap();
        let hash_bytes = hash_wrapped.as_bytes();
        
        // Sao chép 32 bytes đầu tiên vào key
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash_bytes[0..32]);
        
        Ok(key)
    }
}


// Cấu hình cho lưu trữ ví
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub wallet_dir: String,
    pub wallet_filename: String,
    pub encryption_salt: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            wallet_dir: ".wallets".to_string(),
            wallet_filename: "wallets.json".to_string(),
            encryption_salt: "diamond".to_string(),
        }
    }
}

fn load_wallets(path: &Path) -> Result<Vec<WalletInfo>> {
    let file_content = fs::read(path)
        .context("Failed to read wallet file")?;

    let wallet_data: Vec<WalletInfo> = serde_json::from_slice(&file_content)
        .context("Failed to parse wallet data")?;

    Ok(wallet_data)
}

// Khóa mã hóa
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct EncryptionKey {
    key_id: String,
    key: [u8; 32],
    created_at: u64,
}

impl Zeroize for EncryptionKey {
    fn zeroize(&mut self) {
        self.key.zeroize();
    }
}

impl Drop for EncryptionKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for EncryptionKey {}

#[allow(dead_code)]
impl EncryptionKey {
    /// Tạo khóa mã hóa mới
    fn new() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        
        Self {
            key_id: uuid::Uuid::new_v4().to_string(),
            key,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
    
}


impl WalletInfo {
    /// Tạo view an toàn không chứa dữ liệu nhạy cảm
    pub fn to_safe_view(&self) -> SafeWalletView {
        SafeWalletView {
            address: self.address.clone(),
            chain_id: self.chain_id,
            created_at: self.created_at,
            last_used: self.last_used,
            balance: self.balance.clone(),
            has_private_key: self.encrypted_private_key.is_some(),
            has_mnemonic: self.encrypted_mnemonic.is_some(),
            name: self.name.clone(),
            tags: self.tags.clone(),
            is_hardware: self.is_hardware,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_secure_storage_store_load() {
        let temp_dir = tempdir().unwrap();
        let config = StorageConfig::default();
        let storage = SecureWalletStorage::new(temp_dir.path().to_path_buf(), &config).unwrap();

        let identifier = "test_wallet";
        let data = b"sensitive wallet data";
        let password = "secure_password";

        // Store data
        storage.store_wallet( &WalletInfo{ address: identifier.to_string(), encrypted_private_key: None, encrypted_mnemonic: None, chain_id: 1, created_at: 1, last_used: 1, balance: None, name: None, tags: vec![], is_hardware: false}, password).unwrap();

        // Verify it exists
        assert!(storage.exists(identifier));

        // Load data
        let loaded_data = storage.load_wallet(identifier, password).unwrap();
        assert_eq!(loaded_data.address, identifier.to_string());


        // Delete data
        storage.delete_wallet(identifier).unwrap();
        assert!(!storage.exists(identifier));
    }

    #[test]
    fn test_cache_entry() {
        let value = "test_value";
        // Create with negative TTL to ensure it's already expired
        let entry = CacheEntry::new(value, -1);
        assert!(entry.is_expired());

        // Create with future TTL
        let entry = CacheEntry::new(value, 3600);
        assert!(!entry.is_expired());
    }
}
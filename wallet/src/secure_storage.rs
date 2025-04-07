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

/// Cấu trúc dữ liệu ví bảo mật
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

/// Cấu trúc dữ liệu mã hóa
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub salt: Vec<u8>,
    pub version: u8,  // Phiên bản của thuật toán mã hóa
}

/// View an toàn của wallet để hiển thị
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

/// Cấu hình cho lưu trữ ví
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

/// Khóa mã hóa
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
    
    /// Tạo khóa từ mật khẩu sử dụng Argon2
    fn from_password(password: &str, salt: &[u8]) -> Result<Self> {
        use argon2::{
            Argon2, 
            Algorithm, 
            Params, 
            Version
        };
        
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
        
        Ok(Self {
            key_id: hex::encode(&salt[0..16]),
            key,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }
}

/// SecureWalletStorage - Quản lý lưu trữ ví an toàn
pub struct SecureWalletStorage {
    wallets: HashMap<String, WalletInfo>,
    wallet_path: PathBuf,
    current_key: EncryptionKey,
    salt: Vec<u8>,
}

impl SecureWalletStorage {
    /// Tạo hàm trợ giúp tạo chuỗi hex ngẫu nhiên
    fn rand_hex_string(len: usize) -> String {
        let mut bytes = vec![0u8; len];
        OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    /// Tạo kho lưu trữ mới
    pub fn new(config: &StorageConfig) -> Result<Self> {
        let salt = config.encryption_salt.as_bytes().to_vec();
        let wallet_path = Path::new(&config.wallet_dir).join(&config.wallet_filename);
        
        // Tạo khóa mặc định
        let key_id = format!("key-{}", Self::rand_hex_string(8));
        let key = Self::generate_encryption_key("default", &salt)?;
        
        let mut storage = SecureWalletStorage {
            wallets: HashMap::new(),
            wallet_path,
            current_key: EncryptionKey {
                key_id,
                key,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
            salt,
        };
        
        // Tải ví từ đĩa nếu tệp tồn tại
        if storage.wallet_path.exists() {
            if let Ok(wallets) = storage.load_wallets() {
                for wallet in wallets {
                    storage.wallets.insert(wallet.address.clone(), wallet);
                }
            }
        }
        
        Ok(storage)
    }

    /// Tải ví từ đĩa
    pub fn load_wallets(&self) -> Result<Vec<WalletInfo>> {
        if !self.wallet_path.exists() {
            debug!("Wallet file does not exist, returning empty list");
            return Ok(Vec::new());
        }

        debug!("Loading wallets from: {:?}", self.wallet_path);
        
        // Sử dụng std::fs thay vì tokio::fs
        let file_content = fs::read(&self.wallet_path)
            .context("Failed to read wallet file")?;

        let wallet_data: Vec<WalletInfo> = serde_json::from_slice(&file_content)
            .context("Failed to parse wallet data")?;

        Ok(wallet_data)
    }

    /// Lưu ví vào tệp
    pub fn save_to_file(&self) -> Result<()> {
        let wallets: Vec<WalletInfo> = self.wallets.values().cloned().collect();
        
        debug!("Saving {} wallets to storage", wallets.len());
        
        let wallets_json = serde_json::to_string_pretty(&wallets)
            .context("Failed to serialize wallets")?;
        
        let temp_path = format!("{}.tmp", self.wallet_path.display());
        fs::write(&temp_path, wallets_json)
            .with_context(|| format!("Failed to write wallet file: {}", temp_path))?;
        
        fs::rename(&temp_path, &self.wallet_path)
            .with_context(|| format!("Failed to rename temp file to wallet file: {}", self.wallet_path.display()))?;
        
        debug!("Saved {} wallets to storage", self.wallets.len());
        Ok(())
    }

    /// Hỗ trợ clone từ RwLockReadGuard để tránh vấn đề borrow checker
    pub fn clone_from_lock(storage: &Self) -> Self {
        Self {
            wallets: storage.wallets.clone(),
            wallet_path: storage.wallet_path.clone(),
            current_key: storage.current_key.clone(),
            salt: storage.salt.clone(),
        }
    }

    /// Lưu tất cả ví vào file
    pub fn save(&self) -> Result<()> {
        let wallets: Vec<WalletInfo> = self.wallets.values().cloned().collect();
        let wallets_json = serde_json::to_string_pretty(&wallets)
            .context("Failed to serialize wallets")?;
        
        let temp_path = format!("{}.tmp", self.wallet_path.display());
        fs::write(&temp_path, wallets_json)
            .with_context(|| format!("Failed to write wallet file: {}", temp_path))?;
        
        fs::rename(&temp_path, &self.wallet_path)
            .with_context(|| format!("Failed to rename temp file to wallet file: {}", self.wallet_path.display()))?;
        
        debug!("Saved {} wallets to storage", self.wallets.len());
        Ok(())
    }
    
    /// Thêm ví mới vào lưu trữ
    pub fn add_wallet(&mut self, wallet: WalletInfo) -> Result<()> {
        // Kiểm tra ví đã tồn tại chưa
        if self.wallets.contains_key(&wallet.address.to_lowercase()) {
            return Err(anyhow!("Wallet with address {} already exists", wallet.address));
        }
        
        // Thêm vào danh sách
        self.wallets.insert(wallet.address.to_lowercase(), wallet);
        
        Ok(())
    }
    
    /// Lấy thông tin ví theo địa chỉ
    pub fn get_wallet(&self, address: &str) -> Option<&WalletInfo> {
        self.wallets.get(&address.to_lowercase())
    }
    
    /// Lấy thông tin ví có thể thay đổi
    pub fn get_wallet_mut(&mut self, address: &str) -> Option<&mut WalletInfo> {
        self.wallets.get_mut(&address.to_lowercase())
    }
    
    /// Xóa ví khỏi lưu trữ
    pub fn remove_wallet(&mut self, address: &str) -> Result<()> {
        if !self.wallets.contains_key(&address.to_lowercase()) {
            return Err(anyhow!("Wallet with address {} not found", address));
        }
        
        self.wallets.remove(&address.to_lowercase());
        
        Ok(())
    }
    
    /// Lấy danh sách tất cả ví
    pub fn get_all_wallets(&self) -> Vec<SafeWalletView> {
        self.wallets.values()
            .map(|w| w.to_safe_view())
            .collect()
    }
    
    /// Mã hóa private key cho ví
    pub fn encrypt_private_key(&self, private_key: &str) -> Result<EncryptedData> {
        let salt = {
            let mut s = [0u8; 16];
            OsRng.fill_bytes(&mut s);
            s.to_vec()
        };
        
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.current_key.key));
        
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = GenericArray::from_slice(&nonce_bytes);
        
        let ciphertext = cipher.encrypt(nonce, private_key.as_bytes())
            .map_err(|e| anyhow!("Failed to encrypt private key: {}", e))?;
            
        Ok(EncryptedData {
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            salt,
            version: 1,
        })
    }
    
    /// Giải mã private key
    pub fn decrypt_private_key(&self, encrypted: &EncryptedData) -> Result<String> {
        let cipher = Aes256Gcm::new(GenericArray::from_slice(&self.current_key.key));
        
        let nonce = GenericArray::from_slice(&encrypted.nonce);
        let plaintext = cipher.decrypt(nonce, encrypted.ciphertext.as_ref())
            .map_err(|e| anyhow!("Failed to decrypt private key: {}", e))?;
        
        let private_key = String::from_utf8(plaintext)
            .map_err(|e| anyhow!("Failed to convert data: {}", e))?;
        
        Ok(private_key)
    }
    
    /// Tạo ví từ private key
    pub fn create_with_private_key(&mut self, 
                                  private_key: &str, 
                                  chain_id: u64,
                                  name: Option<String>) -> Result<WalletInfo> {
        // Xác thực private key
        Self::validate_private_key(private_key)?;
        
        // Tạo ví từ private key
        let wallet = LocalWallet::from_str(private_key)
            .context("Failed to create wallet from private key")?
            .with_chain_id(chain_id);
            
        // Lấy địa chỉ
        let address = format!("{:?}", wallet.address());
        
        // Mã hóa private key
        let encrypted_private_key = Some(self.encrypt_private_key(private_key)?);
        
        // Tạo thông tin ví
        let wallet_info = WalletInfo {
            address: address.clone(),
            encrypted_private_key,
            encrypted_mnemonic: None,
            chain_id,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            last_used: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            balance: None,
            name,
            tags: Vec::new(),
            is_hardware: false,
        };
        
        // Thêm vào danh sách
        self.add_wallet(wallet_info.clone())?;
        
        Ok(wallet_info)
    }
    
    /// Chuyển đổi từ WalletInfo sang LocalWallet
    pub fn to_local_wallet(&self, wallet_info: &WalletInfo) -> Result<LocalWallet> {
        let encrypted_key = wallet_info.encrypted_private_key.as_ref()
            .ok_or_else(|| anyhow!("Wallet does not have a private key"))?;
        
        let private_key = self.decrypt_private_key(encrypted_key)?;
        
        let wallet = LocalWallet::from_str(&private_key)
            .context("Failed to create wallet from private key")?
            .with_chain_id(wallet_info.chain_id);
            
        Ok(wallet)
    }
    
    /// Thay đổi khóa mã hóa và mã hóa lại tất cả ví
    pub fn rotate_encryption_key(&mut self, new_password: Option<&str>) -> Result<()> {
        let new_key = if let Some(password) = new_password {
            let mut salt = [0u8; 16];
            OsRng.fill_bytes(&mut salt);
            Self::generate_encryption_key(password, &salt)?
        } else {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            key
        };
        
        // Tạo bản sao tạm thời của self.current_key để tránh borrow conflict
        let current_key = self.current_key.clone();
        
        // Tạo encryption key mới
        let key_id = format!("key-{}", Self::rand_hex_string(8));
        let new_key_obj = EncryptionKey {
            key_id,
            key: new_key,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        // Mã hóa lại tất cả ví
        let mut re_encrypted_wallets = HashMap::new();
        for (address, mut wallet) in self.wallets.iter().map(|(k, v)| (k.clone(), v.clone())) {
            if let Some(encrypted_key) = &wallet.encrypted_private_key {
                // Sử dụng bản copy của current_key
                let cipher = Aes256Gcm::new(GenericArray::from_slice(&current_key.key));
                let nonce = GenericArray::from_slice(&encrypted_key.nonce);
                let plaintext = cipher.decrypt(nonce, encrypted_key.ciphertext.as_ref())
                    .map_err(|e| anyhow!("Failed to decrypt private key: {}", e))?;
                
                let private_key = String::from_utf8(plaintext)
                    .map_err(|e| anyhow!("Failed to convert data: {}", e))?;
                
                // Mã hóa lại với khóa mới
                let salt = {
                    let mut s = [0u8; 16];
                    OsRng.fill_bytes(&mut s);
                    s.to_vec()
                };
                
                let new_cipher = Aes256Gcm::new(GenericArray::from_slice(&new_key_obj.key));
                
                let mut nonce_bytes = [0u8; 12];
                OsRng.fill_bytes(&mut nonce_bytes);
                let new_nonce = GenericArray::from_slice(&nonce_bytes);
                
                let ciphertext = new_cipher.encrypt(new_nonce, private_key.as_bytes())
                    .map_err(|e| anyhow!("Failed to encrypt private key: {}", e))?;
                
                wallet.encrypted_private_key = Some(EncryptedData {
                    ciphertext,
                    nonce: nonce_bytes.to_vec(),
                    salt,
                    version: 1,
                });
            }
            
            if let Some(_encrypted_mnemonic) = &wallet.encrypted_mnemonic {
                // existing code 
            }
            
            re_encrypted_wallets.insert(address, wallet);
        }
        
        // Cập nhật wallets và key
        self.wallets = re_encrypted_wallets;
        self.current_key = new_key_obj;
        
        // Lưu thay đổi
        self.save()?;
        
        Ok(())
    }
    
    /// Xác thực private key
    pub fn validate_private_key(private_key: &str) -> Result<()> {
        let key = private_key.trim_start_matches("0x");
        
        if key.len() != 64 {
            return Err(anyhow!("Invalid private key length. Expected 64 hex characters."));
        }
        
        // Kiểm tra private key có phải dạng hex không
        if !key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(anyhow!("Private key must contain only hex characters"));
        }
        
        // Kiểm tra xem có thể parse thành LocalWallet không
        let _ = LocalWallet::from_str(private_key)?;
        
        Ok(())
    }
    
    /// Xác thực địa chỉ ví
    pub fn validate_wallet_address(address: &str) -> Result<()> {
        if !address.starts_with("0x") {
            return Err(anyhow!("Address must start with 0x"));
        }
        
        if address.len() != 42 {
            return Err(anyhow!("Address must be 42 characters long (including 0x)"));
        }
        
        // Kiểm tra địa chỉ có phải dạng hex không
        if !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(anyhow!("Address must contain only hex characters"));
        }
        
        // Kiểm tra checksum
        let address_no_prefix = &address[2..];
        if address_no_prefix.chars().any(|c| c.is_ascii_alphabetic()) {
            // Nếu có ký tự chữ, kiểm tra checksum
            match ethers::types::Address::from_str(address) {
                Ok(_) => Ok(()),
                Err(_) => Err(anyhow!("Invalid address checksum")),
            }
        } else {
            // Nếu toàn số, chuyển đổi sang Address
            match ethers::types::Address::from_str(address) {
                Ok(_) => Ok(()),
                Err(_) => Err(anyhow!("Invalid address format")),
            }
        }
    }

    /// Tạo khóa mã hóa từ mật khẩu
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
    
    #[tokio::test]
    async fn test_wallet_encryption() -> Result<()> {
        let config = StorageConfig {
            wallet_dir: ".wallets".to_string(),
            wallet_filename: "test_wallets.json".to_string(),
            encryption_salt: "diamond".to_string(),
        };
        
        let storage = SecureWalletStorage::new(&config)?;
        
        // Private key từ test
        let private_key = "0000000000000000000000000000000000000000000000000000000000000001";
        
        // Mã hóa và giải mã
        let encrypted = storage.encrypt_private_key(private_key)?;
        let decrypted = storage.decrypt_private_key(&encrypted)?;
        
        assert_eq!(private_key, decrypted);
        
        // Xóa file test nếu tồn tại
        let path = Path::new("test_wallets.json");
        if path.exists() {
            fs::remove_file(path)?;
        }
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_create_wallet() -> Result<()> {
        let config = StorageConfig {
            wallet_dir: ".wallets".to_string(),
            wallet_filename: "test_wallets2.json".to_string(),
            encryption_salt: "diamond".to_string(),
        };
        
        let mut storage = SecureWalletStorage::new(&config)?;
        
        // Private key từ test (không dùng trong thực tế)
        let private_key = "0000000000000000000000000000000000000000000000000000000000000001";
        
        // Tạo ví mới
        let wallet_info = storage.create_with_private_key(private_key, 1, Some("Test Wallet".to_string()))?;
        
        // Kiểm tra ví đã được thêm
        assert!(storage.get_wallet(&wallet_info.address).is_some());
        
        // Chuyển đổi sang LocalWallet
        let local_wallet = storage.to_local_wallet(&wallet_info)?;
        
        // Kiểm tra địa chỉ
        assert_eq!(format!("{:?}", local_wallet.address()), wallet_info.address);
        
        // Xóa file test nếu tồn tại
        let path = Path::new("test_wallets2.json");
        if path.exists() {
            fs::remove_file(path)?;
        }
        
        Ok(())
    }
} 
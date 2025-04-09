use ethers::{
    prelude::{LocalWallet, SignerMiddleware, MnemonicBuilder, Provider},
    signers::{coins_bip39::English, Signer},
    types::{transaction::eip2718::TypedTransaction, Address, Bytes, H256, U256},
    providers::{Http, Middleware}
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc as StdArc;
use std::sync::{Arc, RwLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Result, anyhow, Context};
use tracing::{debug, info, warn, error};
use zeroize::Zeroize;
use aes_gcm::{
    aead::{Aead, generic_array::GenericArray, KeyInit},
    Aes256Gcm
};
use crate::secure_storage::{SecureWalletStorage, StorageConfig, SafeWalletView, WalletInfo};

/// Cấu hình cho WalletManager
#[derive(Debug, Clone)]
pub struct WalletManagerConfig {
    pub default_chain_id: u64,
    pub storage_config: StorageConfig,
    pub wallet_encryption_seed: String,
}

impl Default for WalletManagerConfig {
    fn default() -> Self {
        WalletManagerConfig {
            default_chain_id: 1, // Ethereum mainnet
            storage_config: StorageConfig::default(),
            wallet_encryption_seed: "diamond_wallet".to_string(),
        }
    }
}

/// Dữ liệu nhạy cảm của ví
#[derive(Clone)]
#[allow(dead_code)]
struct WalletSecrets {
    private_key: String,
}

impl Drop for WalletSecrets {
    fn drop(&mut self) {
        // Xóa dữ liệu nhạy cảm khi đối tượng bị hủy
        self.private_key.zeroize();
    }
}

impl Zeroize for WalletSecrets {
    fn zeroize(&mut self) {
        self.private_key.zeroize();
    }
}

/// WalletManager - Quản lý ví và khóa bảo mật
pub struct WalletManager {
    storage: RwLock<SecureWalletStorage>,
    wallets: RwLock<HashMap<Address, LocalWallet>>,
    config: WalletManagerConfig,
    encryption_key: String,
}

impl WalletManager {
    /// Tạo một WalletManager mới
    pub fn new(config: WalletManagerConfig) -> Result<Self> {
        let storage = SecureWalletStorage::new(&config.storage_config)?;
        
        Ok(WalletManager {
            storage: RwLock::new(storage),
            wallets: RwLock::new(HashMap::new()),
            encryption_key: config.wallet_encryption_seed.clone(),
            config,
        })
    }
    
    /// Tạo WalletManager từ Config cũ
    pub async fn from_config(config: Arc<crate::config::Config>) -> Result<Self> {
        // Chuyển đổi từ Config cũ sang WalletManagerConfig
        let wallet_config = WalletManagerConfig {
            default_chain_id: config.chain_id,
            storage_config: StorageConfig {
                wallet_dir: config.wallet_dir.clone(),
                wallet_filename: config.wallet_filename.clone(),
                encryption_salt: config.wallet_encryption_seed.clone(),
            },
            wallet_encryption_seed: config.wallet_encryption_seed.clone(),
        };
        
        Self::new(wallet_config)
    }
    
    /// Tạo một ví mới với seed phrase ngẫu nhiên
    pub fn create_wallet(&self, passphrase: Option<&str>) -> Result<(String, Address)> {
        // Tạo mnemonic mới
        let mut rand = rand::thread_rng();
        let mnemonic = ethers::signers::coins_bip39::Mnemonic::<English>::new_with_count(&mut rand, 12)?;
        
        // Lấy phrase 
        let phrase = mnemonic.to_phrase();
        
        // Tạo ví từ mnemonic
        let wallet = MnemonicBuilder::<English>::default()
            .phrase(phrase.as_str())
            .password(passphrase.unwrap_or(""))
            .derivation_path("m/44'/60'/0'/0/0")?
            .build()?;

        let address = wallet.address();
        let private_key = hex::encode(wallet.signer().to_bytes());
        
        // Tạo wallet info và lưu vào storage
        let mut storage = self.storage.write().unwrap();
        storage.create_with_private_key(&private_key, 
                                       self.config.default_chain_id, 
                                       None)?;
        storage.save_to_file()?;
        drop(storage);
        
        // Thêm ví vào cache
        let mut wallets = self.wallets.write().unwrap();
        wallets.insert(address, wallet);
        
        Ok((phrase, address))
    }
    
    /// Tạo ví mới và trả về SafeWalletView - tương thích với API cũ
    pub fn create_new_wallet(&mut self, passphrase: Option<&str>) -> Result<SafeWalletView> {
        let (_, address) = self.create_wallet(passphrase)?;
        
        // Trả về thông tin ví an toàn
        let storage = self.storage.read().unwrap();
        let wallet_info = storage.get_wallet(&address.to_string())
            .ok_or_else(|| anyhow!("Wallet not found"))?;
        
        Ok(wallet_info.to_safe_view())
    }
    
    /// Import ví từ private key
    pub fn import_private_key(&self, private_key: &str, name: Option<String>) -> Result<Address> {
        // Xác nhận private key hợp lệ
        let wallet = LocalWallet::from_str(private_key)?;
        let address = wallet.address();
        
        // Lưu vào storage
        let mut storage = self.storage.write().unwrap();
        storage.create_with_private_key(private_key, 
                                       self.config.default_chain_id, 
                                       name)?;
        storage.save_to_file()?;
        drop(storage);
        
        // Thêm vào cache
        let mut wallets = self.wallets.write().unwrap();
        wallets.insert(address, wallet);
        
        Ok(address)
    }
    
    /// Import ví từ private key - tương thích với API cũ
    pub fn import_from_private_key(&self, private_key: &str) -> Result<WalletInfo> {
        // Xác thực private key
        Self::validate_private_key(private_key)?;
        
        // Import ví
        let address = self.import_private_key(private_key, None)?;
        
        // Lấy thông tin ví
        let storage = self.storage.read().unwrap();
        let wallet_info = storage.get_wallet(&address.to_string())
            .ok_or_else(|| anyhow!("Wallet not found"))?
            .clone();
            
        Ok(wallet_info)
    }
    
    /// Import ví từ mnemonic
    pub fn import_from_mnemonic(&self, mnemonic: &str, passphrase: Option<&str>) -> Result<WalletInfo> {
        // Xác thực mnemonic
        Self::validate_mnemonic(mnemonic)?;
        
        // Tạo ví từ mnemonic
        let wallet = MnemonicBuilder::<English>::default()
            .phrase(mnemonic)
            .password(passphrase.unwrap_or(""))
            .derivation_path("m/44'/60'/0'/0/0")?
            .build()?;
            
        let address = wallet.address();
        let private_key = hex::encode(wallet.signer().to_bytes());
        
        // Lưu private key vào storage
        let mut storage = self.storage.write().unwrap();
        let wallet_info = storage.create_with_private_key(&private_key, 
                                       self.config.default_chain_id, 
                                       None)?;
        storage.save_to_file()?;
        drop(storage);
        
        // Thêm vào cache
        let mut wallets = self.wallets.write().unwrap();
        wallets.insert(address, wallet);
        
        Ok(wallet_info)
    }
    
    /// Tạo nhiều ví HD từ mnemonic
    pub fn create_hd_wallets(&mut self, mnemonic: &str, count: usize, passphrase: Option<&str>) -> Result<Vec<WalletInfo>> {
        // Xác thực mnemonic
        Self::validate_mnemonic(mnemonic)?;
        
        let mut wallet_infos = Vec::with_capacity(count);
        
        // Tạo nhiều ví với các path khác nhau
        for i in 0..count {
            let path = format!("m/44'/60'/0'/0/{}", i);
            
            let wallet = MnemonicBuilder::<English>::default()
                .phrase(mnemonic)
                .password(passphrase.unwrap_or(""))
                .derivation_path(&path)?
                .build()?;
                
            let address = wallet.address();
            let private_key = hex::encode(wallet.signer().to_bytes());
            
            // Lưu ví vào storage
            let mut storage = self.storage.write().unwrap();
            let wallet_info = storage.create_with_private_key(&private_key, 
                                           self.config.default_chain_id, 
                                           Some(format!("HD Wallet {}", i)))?;
            storage.save_to_file()?;
            drop(storage);
            
            // Thêm vào cache
            let mut wallets = self.wallets.write().unwrap();
            wallets.insert(address, wallet);
            
            wallet_infos.push(wallet_info);
        }
        
        Ok(wallet_infos)
    }
    
    /// Lấy ví theo địa chỉ
    pub fn get_wallet(&self, address: Address) -> Result<LocalWallet> {
        // Kiểm tra cache trước
        let wallets = self.wallets.read().unwrap();
        if let Some(wallet) = wallets.get(&address) {
            return Ok(wallet.clone());
        }
        drop(wallets);
        
        // Nếu không có trong cache, tìm trong storage
        let storage = self.storage.read().unwrap();
        let wallet_info = storage.get_wallet(&address.to_string())
            .ok_or_else(|| anyhow!("Wallet not found"))?;
        
        // Tạo LocalWallet từ wallet_info
        let wallet = storage.to_local_wallet(wallet_info)?;
        
        // Thêm vào cache
        let mut wallets = self.wallets.write().unwrap();
        wallets.insert(address, wallet.clone());
        
        Ok(wallet)
    }
    
    /// Ký một giao dịch
    pub async fn sign_transaction(
        &self,
        address: Address,
        tx: &TypedTransaction,
        chain_id: u64,
    ) -> Result<Bytes> {
        let wallet = self.get_wallet(address)?;
        let wallet = wallet.with_chain_id(chain_id);
        
        // Ký giao dịch
        let signature = wallet.sign_transaction(tx).await?;
        
        // Đóng gói giao dịch với chữ ký
        let signed_tx = tx.rlp_signed(&signature);
        
        Ok(signed_tx)
    }
    
    /// Gửi một giao dịch đã ký qua một provider
    pub async fn send_transaction(
        &self,
        provider: StdArc<Provider<Http>>,
        address: Address,
        tx: &TypedTransaction,
    ) -> Result<H256> {
        let wallet = self.get_wallet(address)?;
        
        // Thiết lập chain ID từ provider
        let chain_id = provider.get_chainid().await?;
        let signing_wallet = wallet.with_chain_id(chain_id.as_u64());
        
        // Tạo client với ví đã ký
        let client = SignerMiddleware::new(provider, signing_wallet);
        
        // Ký và gửi giao dịch 
        let pending_tx = client.send_transaction(tx.clone(), None).await?;
        
        // Lấy hash của giao dịch
        Ok(pending_tx.tx_hash())
    }
    
    /// Lấy danh sách tất cả các ví
    pub fn list_wallets(&self) -> Result<Vec<SafeWalletView>> {
        let storage = self.storage.read().unwrap();
        let wallets = storage.get_all_wallets();
        
        Ok(wallets)
    }
    
    /// Lấy danh sách ví (trả về danh sách an toàn) - tương thích với API cũ
    pub fn get_wallet_list(&self) -> Vec<SafeWalletView> {
        match self.list_wallets() {
            Ok(wallets) => wallets,
            Err(e) => {
                error!("Lỗi khi lấy danh sách ví: {}", e);
                Vec::new()
            }
        }
    }
    
    /// Xóa ví từ storage
    pub fn remove_wallet(&self, address: &str) -> bool {
        // Xóa khỏi storage
        let result = {
            let mut storage = self.storage.write().unwrap();
            let result = storage.remove_wallet(address);
            if result.is_ok() {
                let _ = storage.save_to_file();
                true
            } else {
                false
            }
        };
        
        if result {
            // Xóa khỏi cache nếu xóa từ storage thành công
            if let Ok(address_obj) = Address::from_str(address) {
                let mut wallets = self.wallets.write().unwrap();
                wallets.remove(&address_obj);
            }
        }
        
        result
    }
    
    /// Lưu danh sách ví
    pub async fn save_wallets(&self) -> Result<()> {
        let storage = self.storage.read().unwrap();
        storage.save_to_file()?;
        
        Ok(())
    }
    
    /// Các phương thức xác thực
    pub fn validate_private_key(private_key: &str) -> Result<()> {
        // Xác thực private key hợp lệ
        match LocalWallet::from_str(private_key) {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Private key không hợp lệ: {}", e)),
        }
    }
    
    pub fn validate_mnemonic(mnemonic: &str) -> Result<()> {
        // Xác thực mnemonic hợp lệ
        match ethers::signers::coins_bip39::Mnemonic::<English>::new_from_phrase(mnemonic) {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Mnemonic không hợp lệ: {}", e)),
        }
    }
    
    pub fn validate_wallet_address(address: &str) -> Result<()> {
        // Xác thực địa chỉ ví hợp lệ
        match Address::from_str(address) {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Địa chỉ ví không hợp lệ: {}", e)),
        }
    }
}

/// Extension để tạo client có khả năng ký giao dịch từ Provider
pub trait WalletClientExt {
    fn with_wallet(self, wallet_info: &WalletInfo, encryption_key: &[u8; 32]) -> Result<SignerMiddleware<Provider<Http>, LocalWallet>>;
}

impl WalletClientExt for Provider<Http> {
    fn with_wallet(self, wallet_info: &WalletInfo, encryption_key: &[u8; 32]) -> Result<SignerMiddleware<Provider<Http>, LocalWallet>> {
        let cipher = Aes256Gcm::new(GenericArray::from_slice(encryption_key));
        
        // Giải mã private key
        let encrypted_key = wallet_info.encrypted_private_key.as_ref()
            .ok_or_else(|| anyhow!("Wallet does not have a private key"))?;
            
        let nonce = GenericArray::from_slice(&encrypted_key.nonce);
        let private_key = cipher
            .decrypt(nonce, encrypted_key.ciphertext.as_slice())
            .map_err(|e| anyhow!("Failed to decrypt private key: {}", e))?;
            
        let private_key = String::from_utf8(private_key)
            .map_err(|e| anyhow!("Failed to convert data: {}", e))?;
        
        // Tạo ví từ private key
        let wallet = LocalWallet::from_str(&private_key)?
            .with_chain_id(wallet_info.chain_id);
        
        // Trả về middleware có khả năng ký
        Ok(SignerMiddleware::new(self, wallet))
    }
}

/// Module unit tests
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_wallet_management() -> Result<()> {
        // Test code
        Ok(())
    }
    
    #[tokio::test]
    async fn test_import_from_private_key() -> Result<()> {
        // Test code
        Ok(())
    }
    
    #[tokio::test]
    async fn test_create_hd_wallets() -> Result<()> {
        // Test code
        Ok(())
    }
}

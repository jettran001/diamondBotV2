use ethers::{
    prelude::{LocalWallet, SignerMiddleware, MnemonicBuilder, Provider},
    signers::{coins_bip39::English, Signer},
    types::{transaction::eip2718::TypedTransaction, Address, Bytes, H256},
    providers::{Http, Middleware}
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc as StdArc;
use std::sync::RwLock;
use anyhow::{Result, anyhow};
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
}

impl Default for WalletManagerConfig {
    fn default() -> Self {
        WalletManagerConfig {
            default_chain_id: 1, // Ethereum mainnet
            storage_config: StorageConfig::default(),
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
}

impl WalletManager {
    /// Tạo một WalletManager mới
    pub fn new(config: WalletManagerConfig) -> Result<Self> {
        let storage = SecureWalletStorage::new(&config.storage_config)?;
        
        Ok(WalletManager {
            storage: RwLock::new(storage),
            wallets: RwLock::new(HashMap::new()),
            config,
        })
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
    
    /// Xóa ví từ storage
    pub fn remove_wallet(&self, address: &str) -> Result<()> {
        // Xóa khỏi storage
        let mut storage = self.storage.write().unwrap();
        storage.remove_wallet(address)?;
        storage.save_to_file()?;
        
        // Xóa khỏi cache
        let address_obj = Address::from_str(address)?;
        let mut wallets = self.wallets.write().unwrap();
        wallets.remove(&address_obj);
        
        Ok(())
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

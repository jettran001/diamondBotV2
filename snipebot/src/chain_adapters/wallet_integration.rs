// External imports
use ethers::{
    providers::Middleware,
    signers::LocalWallet,
    types::{Address, U256, H256},
    contract::ERC20,
};

// Standard library imports
use std::{
    sync::Arc,
    collections::HashMap,
    time::Duration,
    str::FromStr,
};

// Internal imports
use crate::{
    chain_adapters::{
        interfaces::{ChainAdapter, ChainError},
        chain_registry::ChainRegistry,
        chain_adapter_impl::EVMChainAdapter,
    },
    types::{WalletBalance, TokenBalance},
    abi_utils,
};

// Third party imports
use anyhow::{Result, anyhow};
use tracing::{info, warn, error};
use tokio::sync::RwLock;
use wallet::{
    secure_storage::{SecureWalletStorage, WalletInfo},
    WalletManager,
};

/// Cấu trúc dữ liệu để quản lý giao dịch
#[derive(Debug, Clone)]
pub struct TransactionManager {
    /// Registry chứa các chain adapter
    registry: Arc<RwLock<ChainRegistry>>,
    /// Secure storage cho ví
    wallet_storage: Arc<RwLock<SecureWalletStorage>>,
}

impl TransactionManager {
    /// Tạo mới một TransactionManager
    pub fn new(
        registry: Arc<RwLock<ChainRegistry>>,
        wallet_storage: Arc<RwLock<SecureWalletStorage>>,
    ) -> Self {
        Self {
            registry,
            wallet_storage,
        }
    }
    
    /// Lấy thông tin ví từ secure storage
    pub async fn get_wallet_info(&self, address: &str) -> Result<WalletInfo> {
        // Kiểm tra định dạng địa chỉ
        let validated_address = SecureWalletStorage::validate_wallet_address(address)
            .map(|_| address.to_string())?;
            
        // Lấy thông tin ví từ storage
        let storage = self.wallet_storage.read().await;
        let wallet_info = storage.get_wallet(&validated_address)
            .ok_or_else(|| anyhow!("Wallet not found: {}", validated_address))?
            .clone();
            
        Ok(wallet_info)
    }
    
    /// Tạo và gửi giao dịch từ ví
    pub async fn send_transaction(
        &self,
        chain_id: u64,
        wallet_address: &str,
        to: &str,
        value: Option<U256>,
        data: Option<Vec<u8>>,
        gas_limit: Option<U256>,
        gas_price: Option<U256>,
        nonce: Option<U256>,
    ) -> Result<H256, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Kiểm tra định dạng địa chỉ ví và địa chỉ nhận
        let wallet_address = SecureWalletStorage::validate_wallet_address(wallet_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        let to_address = SecureWalletStorage::validate_wallet_address(to)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Chuyển đổi địa chỉ sang ethers Address
        let to_address = Address::from_str(to)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Lấy nonce nếu không có
        let nonce = match nonce {
            Some(n) => n,
            None => {
                let address = Address::from_str(wallet_address)
                    .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
                adapter.get_transaction_count(address, None).await?
            }
        };
        
        // Lấy gas price nếu không có
        let gas_price = match gas_price {
            Some(gp) => gp,
            None => adapter.get_gas_price().await?,
        };
        
        // Tạo transaction request
        let tx = TransactionRequest {
            to,
            value,
            data,
            gas_limit,
            gas_price,
            nonce,
            chain_id: Some(chain_id),
        };
        
        // Ước lượng gas nếu không có
        let gas_limit = match gas_limit {
            Some(gl) => gl,
            None => adapter.estimate_gas(&tx).await?,
        };
        
        // Thêm gas limit vào transaction
        let tx = tx.gas(gas_limit);
        
        // Gửi transaction
        debug!("Sending transaction on chain {}: {:?}", chain_id, tx);
        let pending_tx = adapter.send_transaction(&tx).await?;
        
        // Lấy tx hash
        let tx_hash = pending_tx.tx_hash();
        info!("Transaction sent: {:?}", tx_hash);
        
        Ok(tx_hash)
    }
    
    /// Phê duyệt token cho một spender
    pub async fn approve_token(
        &self,
        chain_id: u64,
        wallet_address: &str,
        token_address: &str,
        spender_address: &str,
        amount: U256,
    ) -> Result<H256, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Kiểm tra địa chỉ hợp lệ
        let token = Address::from_str(token_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        let spender = Address::from_str(spender_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Lấy ABI cho ERC20
        let abi: ethers::abi::Abi = serde_json::from_str(abi_utils::get_erc20_abi())
            .context("Failed to parse ERC20 ABI")?;
            
        // Tạo hàm approve
        let function = abi.function("approve")
            .map_err(|e| ChainError::ContractError(e.to_string()))?;
            
        // Mã hóa tham số
        let encoded = function.encode_input(&[
            ethers::abi::Token::Address(spender),
            ethers::abi::Token::Uint(amount),
        ]).map_err(|e| ChainError::ContractError(e.to_string()))?;
        
        // Gửi transaction approve
        self.send_transaction(
            chain_id,
            wallet_address,
            token_address,
            None,                  // Không gửi ETH
            Some(encoded),         // Dữ liệu approve
            None,                  // Tự động ước tính gas limit
            None,                  // Tự động lấy gas price
            None,                  // Tự động lấy nonce
        ).await
    }
    
    /// Gửi token đến một địa chỉ
    pub async fn transfer_token(
        &self,
        chain_id: u64,
        wallet_address: &str,
        token_address: &str,
        to_address: &str,
        amount: U256,
    ) -> Result<H256, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Kiểm tra địa chỉ hợp lệ
        let token = Address::from_str(token_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        let to = Address::from_str(to_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Lấy ABI cho ERC20
        let abi: ethers::abi::Abi = serde_json::from_str(abi_utils::get_erc20_abi())
            .context("Failed to parse ERC20 ABI")?;
            
        // Tạo hàm transfer
        let function = abi.function("transfer")
            .map_err(|e| ChainError::ContractError(e.to_string()))?;
            
        // Mã hóa tham số
        let encoded = function.encode_input(&[
            ethers::abi::Token::Address(to),
            ethers::abi::Token::Uint(amount),
        ]).map_err(|e| ChainError::ContractError(e.to_string()))?;
        
        // Gửi transaction transfer
        self.send_transaction(
            chain_id,
            wallet_address,
            token_address,
            None,                  // Không gửi ETH
            Some(encoded),         // Dữ liệu transfer
            None,                  // Tự động ước tính gas limit
            None,                  // Tự động lấy gas price
            None,                  // Tự động lấy nonce
        ).await
    }
    
    /// Lấy danh sách giao dịch gần đây của ví
    pub async fn get_recent_transactions(
        &self,
        chain_id: u64,
        wallet_address: &str,
        limit: usize,
    ) -> Result<Vec<TransactionReceipt>, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Chuyển đổi địa chỉ
        let address = Address::from_str(wallet_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // TODO: Triển khai lấy giao dịch gần đây (cần thực hiện tổng hợp các giao dịch từ block gần đây)
        // Đây chỉ là stub, cần triển khai đầy đủ
        
        Err(ChainError::from_anyhow(anyhow!("get_recent_transactions not implemented yet".to_string())))
    }
    
    /// Kiểm tra xem giao dịch đã hoàn thành chưa
    pub async fn wait_for_transaction(
        &self,
        chain_id: u64,
        tx_hash: H256,
        confirmations: usize,
        timeout_secs: u64,
    ) -> Result<TransactionReceipt, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Đợi giao dịch hoàn thành
        let timeout = std::time::Duration::from_secs(timeout_secs);
        adapter.wait_for_transaction_receipt(tx_hash, confirmations, timeout).await
    }
    
    /// Thực hiện swap token thông qua router
    pub async fn swap_tokens(
        &self,
        chain_id: u64,
        wallet_address: &str,
        token_in: &str,
        token_out: &str,
        amount_in: U256,
        min_amount_out: U256,
        router_address: &str,
        deadline: u64,
    ) -> Result<H256, ChainError> {
        // Lấy adapter cho chain
        let registry = self.registry.read().await;
        let adapter = registry.get_adapter(chain_id)
            .ok_or(ChainError::UnsupportedChain(chain_id))?;
            
        // Kiểm tra địa chỉ hợp lệ
        let token_in_addr = Address::from_str(token_in)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        let token_out_addr = Address::from_str(token_out)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        let router = Address::from_str(router_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Lấy địa chỉ ví
        let wallet = Address::from_str(wallet_address)
            .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
            
        // Lấy ABI cho router
        let abi: ethers::abi::Abi = serde_json::from_str(abi_utils::get_router_abi())
            .context("Failed to parse Router ABI")?;
            
        // Chuẩn bị tham số cho swapExactTokensForTokens
        let path = vec![token_in_addr, token_out_addr];
        let deadline_timestamp = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + deadline
        );
        
        // Tạo hàm swapExactTokensForTokens
        let function = abi.function("swapExactTokensForTokens")
            .map_err(|e| ChainError::ContractError(e.to_string()))?;
            
        // Mã hóa tham số
        let encoded = function.encode_input(&[
            ethers::abi::Token::Uint(amount_in),
            ethers::abi::Token::Uint(min_amount_out),
            ethers::abi::Token::Array(
                path.into_iter()
                    .map(ethers::abi::Token::Address)
                    .collect()
            ),
            ethers::abi::Token::Address(wallet),
            ethers::abi::Token::Uint(deadline_timestamp),
        ]).map_err(|e| ChainError::ContractError(e.to_string()))?;
        
        // Gửi transaction swap
        self.send_transaction(
            chain_id,
            wallet_address,
            router_address,
            None,                  // Không gửi ETH
            Some(encoded),         // Dữ liệu swap
            None,                  // Tự động ước tính gas limit
            None,                  // Tự động lấy gas price
            None,                  // Tự động lấy nonce
        ).await
    }
}

/// Hàm trợ giúp để tạo TransactionManager từ registry và secure storage
pub async fn create_transaction_manager(
    registry: Arc<RwLock<ChainRegistry>>,
    wallet_storage: Arc<RwLock<SecureWalletStorage>>,
) -> TransactionManager {
    TransactionManager::new(registry, wallet_storage)
}

/// Hàm trợ giúp để lấy số dư ví trên nhiều chain
pub async fn get_wallet_balances(
    registry: Arc<RwLock<ChainRegistry>>, 
    wallet_address: &str, 
    chain_ids: &[u64]
) -> Result<Vec<(u64, U256)>, ChainError> {
    // Kiểm tra địa chỉ ví
    let address = Address::from_str(wallet_address)
        .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
        
    let registry_guard = registry.read().await;
    
    let mut results = Vec::new();
    
    // Lấy số dư trên từng chain
    for &chain_id in chain_ids {
        let adapter = match registry_guard.get_adapter(chain_id) {
            Ok(a) => a,
            Err(_) => {
                warn!("Chain adapter not found for chain ID: {}", chain_id);
                continue;
            }
        };
        
        // Lấy số dư ETH
        match adapter.get_eth_balance(address, None).await {
            Ok(balance) => {
                results.push((chain_id, balance));
            }
            Err(e) => {
                error!("Failed to get balance for chain {}: {:?}", chain_id, e);
            }
        }
    }
    
    Ok(results)
}

/// Hàm trợ giúp để lấy số dư token trên nhiều chain
pub async fn get_token_balances(
    registry: Arc<RwLock<ChainRegistry>>, 
    wallet_address: &str, 
    token_addresses: &[(u64, &str)]
) -> Result<Vec<(u64, Address, U256)>, ChainError> {
    // Kiểm tra địa chỉ ví
    let address = Address::from_str(wallet_address)
        .map_err(|e| ChainError::InvalidAddress(e.to_string()))?;
        
    let registry_guard = registry.read().await;
    
    let mut results = Vec::new();
    
    // Lấy số dư trên từng chain và token
    for &(chain_id, token_addr) in token_addresses {
        let adapter = match registry_guard.get_adapter(chain_id) {
            Ok(a) => a,
            Err(_) => {
                warn!("Chain adapter not found for chain ID: {}", chain_id);
                continue;
            }
        };
        
        // Chuyển địa chỉ token sang Address
        let token = match Address::from_str(token_addr) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Invalid token address {}: {:?}", token_addr, e);
                continue;
            }
        };
        
        // Lấy số dư token
        match adapter.get_token_balance(token, address, None).await {
            Ok(balance) => {
                results.push((chain_id, token, balance));
            }
            Err(e) => {
                error!("Failed to get token balance for chain {} token {}: {:?}", 
                       chain_id, token_addr, e);
            }
        }
    }
    
    Ok(results)
}

/// Kết quả của việc gửi giao dịch
#[derive(Debug, Clone)]
pub struct TransactionResult {
    /// Hash giao dịch
    pub tx_hash: H256,
    /// Địa chỉ người gửi
    pub from: Address,
    /// Địa chỉ người nhận (nếu có)
    pub to: Option<Address>,
    /// Giá trị được gửi
    pub value: U256,
    /// Gas limit
    pub gas: U256,
    /// Gas price
    pub gas_price: U256,
    /// Chain ID
    pub chain_id: u64,
}

/// Quản lý tương tác giữa wallet và chain adapters
pub struct WalletIntegration {
    /// Quản lý wallet
    wallet_manager: Arc<WalletManager>,
    /// Cache cho các chain adapter (chain_id -> adapter)
    chain_adapters: Arc<RwLock<HashMap<u64, Arc<EVMChainAdapter>>>>,
}

impl WalletIntegration {
    /// Tạo đối tượng WalletIntegration mới
    pub fn new(wallet_manager: Arc<WalletManager>) -> Self {
        Self {
            wallet_manager,
            chain_adapters: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Thêm adapter cho một chain
    pub async fn add_adapter(&self, chain_id: u64, adapter: Arc<EVMChainAdapter>) {
        let mut adapters = self.chain_adapters.write().await;
        adapters.insert(chain_id, adapter);
    }
    
    /// Lấy adapter cho một chain
    pub async fn get_adapter(&self, chain_id: u64) -> Result<Arc<EVMChainAdapter>, ChainError> {
        let adapters = self.chain_adapters.read().await;
        adapters.get(&chain_id)
            .cloned()
            .ok_or(ChainError::UnsupportedChain(chain_id))
    }
    
    /// Gửi giao dịch trên một chain
    pub async fn send_transaction(
        &self,
        wallet_address: Address,
        chain_id: u64,
        to: Option<Address>,
        value: U256,
        data: Option<Bytes>,
        gas_price: Option<U256>,
        gas_limit: Option<U256>,
    ) -> Result<TransactionResult, ChainError> {
        // Lấy adapter cho chain
        let adapter = self.get_adapter(chain_id).await?;
        
        // Lấy wallet
        let wallet = match self.wallet_manager.get_wallet(wallet_address) {
            Ok(wallet) => wallet.with_chain_id(chain_id),
            Err(_) => return Err(ChainError::WalletNotConfigured),
        };
        
        // Tạo request giao dịch
        let mut tx = TransactionRequest::new();
        
        if let Some(to_addr) = to {
            tx = tx.to(to_addr);
        }
        
        tx = tx.from(wallet_address)
            .value(value);
        
        if let Some(data_bytes) = data {
            tx = tx.data(data_bytes);
        }
        
        // Thiết lập gas price nếu được cung cấp, nếu không thì lấy từ mạng
        let actual_gas_price = if let Some(gp) = gas_price {
            gp
        } else {
            adapter.get_gas_price().await?
        };
        tx = tx.gas_price(actual_gas_price);
        
        // Thiết lập gas limit nếu được cung cấp, nếu không thì ước tính
        let actual_gas_limit = if let Some(gl) = gas_limit {
            gl
        } else {
            // Nếu không có gas_limit, ước tính với một giá trị mặc định
            let mut typed_tx = TypedTransaction::Eip1559(Eip1559TransactionRequest::new());
            typed_tx.set_from(wallet_address);
            typed_tx.set_to(Address::from_str(to.unwrap().to_string().as_str()).unwrap());
            
            if let Some(val) = value {
                typed_tx.set_value(val);
            }
            
            if let Some(data_bytes) = &data {
                typed_tx.set_data(data_bytes.clone());
            }
            
            let gas = match adapter.estimate_gas(&typed_tx).await {
                Ok(gas) => {
                    // Kiểm tra gas limit
                    if gas > config.gas_limit.into() {
                        warn!("Estimated gas {} exceeds max gas limit {}", gas, config.gas_limit);
                        return Err(ChainError::GasLimitExceeded(gas.as_u64()));
                    }
                    
                    // Tăng thêm 20% để đảm bảo đủ gas
                    let adjusted = (gas.as_u64() * 12) / 10;
                    U256::from(adjusted)
                },
                Err(e) => {
                    warn!("Failed to estimate gas: {}", e);
                    U256::from(config.gas_limit)
                }
            };
            
            gas
        };
        tx = tx.gas(actual_gas_limit);
        
        // Lấy nonce hiện tại
        let nonce = adapter.get_transaction_count(wallet_address, None).await?;
        tx = tx.nonce(nonce);
        
        // Chuyển đổi thành TypedTransaction
        let tx: TypedTransaction = tx.into();
        
        // Ký giao dịch
        let signature = wallet.sign_transaction(&tx).await
            .map_err(|e| ChainError::TransactionError(format!("Failed to sign: {}", e)))?;
        
        // Tạo giao dịch đã ký
        let signed_tx = tx.rlp_signed(&signature);
        
        // Gửi giao dịch
        let pending_tx = adapter.send_raw_transaction(signed_tx).await
            .map_err(|e| ChainError::TransactionError(format!("Failed to send: {}", e)))?;
        
        // Lấy hash giao dịch
        let tx_hash = pending_tx.tx_hash();
        
        Ok(TransactionResult {
            tx_hash,
            from: wallet_address,
            to: to,
            value,
            gas: actual_gas_limit,
            gas_price: actual_gas_price,
            chain_id,
        })
    }
    
    /// Phê duyệt token ERC20
    pub async fn approve_token(
        &self,
        wallet_address: Address,
        chain_id: u64,
        token_address: Address,
        spender_address: Address,
        amount: U256,
    ) -> Result<TransactionResult, ChainError> {
        // Tạo calldata cho approve(address,uint256)
        let token = ERC20::new(token_address, self.get_adapter(chain_id).await?.get_provider());
        
        // Kiểm tra allowance hiện tại
        let current_allowance = token.allowance(wallet_address, spender_address).call().await
            .map_err(|e| ChainError::ContractError(format!("Failed to check allowance: {}", e)))?;
        
        // Nếu allowance đã đủ, không cần phê duyệt nữa
        if current_allowance >= amount {
            return Err(ChainError::ApprovalError("Already approved sufficient amount".to_string()));
        }
        
        // Tạo calldata cho approve
        let approve_data = token.approve(spender_address, amount).tx.data()
            .ok_or_else(|| ChainError::ContractError("Failed to generate approval data".to_string()))?
            .clone();
        
        // Gửi giao dịch phê duyệt
        self.send_transaction(
            wallet_address,
            chain_id,
            Some(token_address),
            U256::zero(),
            Some(approve_data),
            None,
            None,
        ).await
    }
    
    /// Chờ giao dịch được xác nhận
    pub async fn wait_for_transaction(
        &self,
        chain_id: u64,
        tx_hash: H256,
        confirmations: usize,
        timeout: Duration,
    ) -> Result<TransactionReceipt, ChainError> {
        let adapter = self.get_adapter(chain_id).await?;
        
        // Tạo future với timeout
        let receipt_future = adapter.wait_for_transaction_receipt(tx_hash, confirmations, timeout);
        
        // Đặt timeout cho việc chờ
        match tokio::time::timeout(timeout, receipt_future).await {
            Ok(result) => result.map_err(|e| ChainError::from_anyhow(e.into())),
            Err(_) => Err(ChainError::from_anyhow(anyhow::anyhow!(
                "Timed out waiting for tx {}", tx_hash
            ))),
        }
    }
    
    /// Lấy số dư ETH và tokens của ví
    pub async fn get_wallet_balances(
        &self,
        wallet_address: Address,
        chain_id: u64,
        token_addresses: Vec<Address>,
    ) -> Result<(U256, HashMap<Address, U256>), ChainError> {
        let adapter = self.get_adapter(chain_id).await?;
        
        // Lấy số dư ETH
        let eth_balance = adapter.get_eth_balance(wallet_address, None).await?;
        
        // Lấy số dư từng token
        let mut token_balances = HashMap::new();
        for token_address in token_addresses {
            match adapter.get_token_balance(token_address, wallet_address, None).await {
                Ok(balance) => {
                    token_balances.insert(token_address, balance);
                },
                Err(e) => {
                    warn!(
                        "Không thể lấy số dư token {}: {}",
                        token_address, e
                    );
                }
            }
        }
        
        Ok((eth_balance, token_balances))
    }
    
    /// Lấy chi tiết về một token
    pub async fn get_token_details(
        &self,
        chain_id: u64,
        token_address: Address,
    ) -> Result<crate::chain_adapters::interfaces::TokenDetails, ChainError> {
        let adapter = self.get_adapter(chain_id).await?;
        adapter.get_token_details(token_address).await
    }
    
    /// Lấy số dư của token
    pub async fn get_token_balance(
        &self,
        chain_id: u64,
        token_address: Address,
        wallet_address: Address,
    ) -> Result<U256, ChainError> {
        let adapter = self.get_adapter(chain_id).await?;
        adapter.get_token_balance(token_address, wallet_address, None).await
    }
    
    /// Làm mới cache adapter
    pub async fn refresh_adapters(&self) {
        let mut adapters = self.chain_adapters.write().await;
        adapters.clear();
    }
    
    /// Nhập ví từ private key và thêm vào WalletManager
    pub fn import_wallet_from_private_key(&self, private_key: &str) -> Result<Address, ChainError> {
        self.wallet_manager.import_from_private_key(private_key)
            .map_err(|e| ChainError::from_anyhow(e))
    }
    
    /// Nhập ví từ seed phrase và thêm vào WalletManager
    pub fn import_wallet_from_seed(&self, seed_phrase: &str, password: Option<&str>)
        -> Result<Address, ChainError> 
    {
        self.wallet_manager.import_from_seed_phrase(seed_phrase, password)
            .map_err(|e| ChainError::from_anyhow(e))
    }
    
    /// Tạo ví mới và thêm vào WalletManager
    pub fn create_new_wallet(&self, password: Option<&str>) -> Result<(String, Address), ChainError> {
        self.wallet_manager.create_wallet(password)
            .map_err(|e| ChainError::from_anyhow(e))
    }
    
    /// Xóa ví khỏi WalletManager
    pub fn remove_wallet(&self, address: Address) -> Result<(), ChainError> {
        self.wallet_manager.remove_wallet(address)
            .map_err(|e| ChainError::from_anyhow(e))
    }
    
    /// Liệt kê tất cả các ví trong WalletManager
    pub fn list_wallets(&self) -> Result<Vec<Address>, ChainError> {
        self.wallet_manager.list_wallets()
            .map_err(|e| ChainError::from_anyhow(e))
    }
    
    /// Kiểm tra nếu có ví nào trong hệ thống
    pub fn has_wallets(&self) -> bool {
        self.wallet_manager.has_wallets()
    }
}

// Giao diện ERC20 để tương tác với token
abigen!(
    ERC20,
    r#"[
        function name() view returns (string)
        function symbol() view returns (string)
        function decimals() view returns (uint8)
        function totalSupply() view returns (uint256)
        function balanceOf(address) view returns (uint256)
        function allowance(address owner, address spender) view returns (uint256)
        function approve(address spender, uint256 amount) returns (bool)
        function transfer(address to, uint256 amount) returns (bool)
        function transferFrom(address from, address to, uint256 amount) returns (bool)
    ]"#,
); 
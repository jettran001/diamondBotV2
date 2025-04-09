// External imports
use async_trait::async_trait;
use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    signers::{LocalWallet, Signer},
    types::{Address, U256, TransactionRequest, Transaction, TransactionReceipt, BlockId},
    core::types::BlockNumber,
};
use log::{info, error, warn};
use serde::{Serialize, Deserialize};
use anyhow::Result;
use once_cell::sync::Lazy;

// Standard library imports
use std::{
    sync::{Arc, RwLock},
    str::FromStr,
    collections::HashMap,
    time::{Instant, SystemTime},
};

// Internal imports 
use crate::{
    error::{TransactionError, classify_blockchain_error, get_recovery_info},
    utils::{self, retry_blockchain_tx, RetryConfig, transaction_retry_config},
    abi_utils,
    chain_adapters::{
        nonce_manager::NonceManager,
        retry::retry_blockchain_operation,
        interfaces::{ChainAdapter, ChainError, GasInfo, TokenDetails, BlockInfo, NodeInfo},
        retry_policy::{RetryContext, create_default_retry_policy},
        connection_pool::{get_or_create_pool, ProviderGuard},
    },
};

use common::cache::{Cache, JSONCache, CacheEntry, CacheConfig};

/// Cấu hình cơ bản cho một mạng EVM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Tên chain (VD: "Ethereum", "BSC")
    pub name: String,
    /// Chain ID
    pub chain_id: u64,
    /// URL của RPC endpoint
    pub rpc_url: String,
    /// Ký hiệu của token gốc (VD: "ETH", "BNB", "AVAX")
    pub native_symbol: String,
    /// Địa chỉ của wrapped native token
    pub wrapped_native_token: String,
    /// Địa chỉ của router DEX (Uniswap v2 compatible) 
    pub router_address: String,
    /// Địa chỉ của factory DEX
    pub factory_address: String,
    /// URL của blockchain explorer
    pub explorer_url: String,
    /// Thời gian ra block (ms)
    pub block_time: u64,
    /// Gas limit mặc định
    pub default_gas_limit: u64,
    /// Gas price mặc định (gwei)
    pub default_gas_price: f64,
    /// Hỗ trợ EIP-1559 (fee market)
    pub eip1559_supported: bool,
    /// Mức priority fee đề xuất (nếu hỗ trợ EIP-1559)
    pub max_priority_fee: Option<f64>,
    /// Tên hàm swap ETH -> Token (VD: swapExactETHForTokens hoặc swapExactAVAXForTokens)
    pub eth_to_token_swap_fn: String,
    /// Tên hàm swap Token -> ETH (VD: swapExactTokensForETH hoặc swapExactTokensForAVAX)
    pub token_to_eth_swap_fn: String,
}

/// Adapter chung cho tất cả các mạng EVM 
#[derive(Debug)]
pub struct EVMAdapter {
    /// Provider cho mạng
    provider: Provider<Http>,
    /// Ví người dùng (nếu được thiết lập)
    wallet: Option<LocalWallet>,
    /// Cấu hình của chain
    config: ChainConfig,
    /// Cache cho ABI contracts
    contract_abis: HashMap<String, ethers::abi::Abi>,
    rpc_pool: Option<Arc<crate::chain_adapters::retry::RPCPool>>,
    /// Quản lý nonce để tránh duplicate transaction
    nonce_manager: Arc<NonceManager>,
    /// Cache cho dữ liệu JSON
    cache: JSONCache,
}

#[async_trait]
impl Cache for EVMAdapter {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.cache.remove(key).await
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear().await
    }

    async fn cleanup_cache(&self) -> Result<()> {
        self.cache.cleanup_cache().await
    }
}

impl EVMAdapter {
    /// Tạo adapter mới với cấu hình định sẵn
    pub async fn new(config: ChainConfig) -> Result<Self> {
        // Tạo provider từ RPC URL
        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .context(format!("Không thể kết nối đến RPC: {}", config.rpc_url))?;
        
        // Tạo map chứa ABI
        let mut contract_abis = HashMap::new();
        
        // Load ABIs
        let router_abi = abi_utils::get_router_abi();
        let router_abi: ethers::abi::Abi = serde_json::from_str(router_abi)
            .context("Không thể parse Router ABI")?;
        contract_abis.insert("router".to_string(), router_abi);
        
        let factory_abi = abi_utils::get_factory_abi();
        let factory_abi: ethers::abi::Abi = serde_json::from_str(factory_abi)
            .context("Không thể parse Factory ABI")?;
        contract_abis.insert("factory".to_string(), factory_abi);
        
        let erc20_abi = abi_utils::get_erc20_abi();
        let erc20_abi: ethers::abi::Abi = serde_json::from_str(erc20_abi)
            .context("Không thể parse ERC20 ABI")?;
        contract_abis.insert("erc20".to_string(), erc20_abi);
        
        let pair_abi = abi_utils::get_pair_abi();
        let pair_abi: ethers::abi::Abi = serde_json::from_str(pair_abi)
            .context("Không thể parse Pair ABI")?;
        contract_abis.insert("pair".to_string(), pair_abi);
        
        // Tạo nonce manager với provider
        let provider_arc = Arc::new(provider.clone());
        let nonce_manager = Arc::new(NonceManager::new(provider_arc, 60)); // Cache nonce trong 60 giây
        
        Ok(Self {
            provider,
            wallet: None,
            config,
            contract_abis,
            rpc_pool: None,
            nonce_manager,
            cache: JSONCache::new(),
        })
    }
    
    /// Lấy thông tin cấu hình
    pub fn get_config(&self) -> &ChainConfig {
        &self.config
    }
    
    /// Lấy provider
    pub fn get_provider(&self) -> &Provider<Http> {
        &self.provider
    }
    
    /// Lấy ví (nếu có)
    pub fn get_wallet(&self) -> Option<&LocalWallet> {
        self.wallet.as_ref()
    }
    
    /// Đặt ví
    pub fn set_wallet(&mut self, wallet: LocalWallet) {
        self.wallet = Some(wallet);
    }
    
    /// Lấy ví với chain id
    fn get_wallet_with_chain_id(&self) -> Result<LocalWallet> {
        let wallet = self.wallet.as_ref()
            .ok_or_else(|| anyhow!("Không có ví để thực hiện giao dịch"))?;
        Ok(wallet.clone().with_chain_id(self.config.chain_id))
    }
    
    /// Tạo client với ví
    fn get_client(&self) -> Result<SignerMiddleware<Provider<Http>, LocalWallet>> {
        let wallet = self.get_wallet_with_chain_id()?;
        Ok(SignerMiddleware::new(self.provider.clone(), wallet))
    }
    
    /// Lấy số dư native token
    pub async fn get_native_balance(&self, address: &str) -> Result<U256> {
        use crate::chain_adapters::retry::retry_async;
        
        // Thử lấy từ cache trước
        let cache_key = format!("balance_{}_{}", self.config.name, address);
        if let Some(balance) = self.get_from_cache::<U256>(&cache_key) {
            return Ok(balance);
        }
        
        // Tạo address từ chuỗi
        let wallet_addr = Address::from_str(address)
            .context(format!("Địa chỉ ví không hợp lệ: {}", address))?;
            
        // Thực hiện request với retry
        let provider = self.provider.clone();
        let balance = retry_async(move || async move {
            provider.get_balance(wallet_addr, None).await
                .map_err(|e| anyhow!("Lỗi khi lấy số dư: {}", e))
        }).await?;
        
        // Cache kết quả
        let _ = self.store_in_cache(&cache_key, &balance, 10); // TTL 10 giây
        
        Ok(balance)
    }
    
    /// Lấy số dư token
    pub async fn get_token_balance(&self, token_address: &str, wallet_address: &str) -> Result<U256> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        let wallet_addr = Address::from_str(wallet_address)
            .context(format!("Địa chỉ ví không hợp lệ: {}", wallet_address))?;
        
        // Tạo contract ERC20
        let token_abi = self.contract_abis.get("erc20")
            .ok_or_else(|| anyhow!("Không tìm thấy ERC20 ABI"))?;
        let token_contract = Contract::new(token_addr, token_abi.clone(), self.provider.clone());
        
        // Gọi hàm balanceOf
        let balance: U256 = token_contract.method("balanceOf", wallet_addr)?
            .call().await
            .context("Lỗi khi gọi balanceOf")?;
        Ok(balance)
    }
    
    /// Phê duyệt token
    pub async fn approve_token(&self, token_address: &str, spender_address: &str, amount: U256) -> Result<Option<TransactionReceipt>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        let spender_addr = Address::from_str(spender_address)
            .context(format!("Địa chỉ spender không hợp lệ: {}", spender_address))?;
        
        // Tạo client với ví
        let client = Arc::new(self.get_client()?);
        
        // Tạo contract ERC20 với quyền ghi
        let token_abi = self.contract_abis.get("erc20")
            .ok_or_else(|| anyhow!("Không tìm thấy ERC20 ABI"))?;
        let token_contract = Contract::new(token_addr, token_abi.clone(), client);
        
        // Gọi hàm approve
        let tx = token_contract.method("approve", (spender_addr, amount))?
            .send()
            .await
            .context("Lỗi khi gửi giao dịch approve")?;
            
        // Đợi biên lai
        let receipt = tx.await
            .context("Lỗi khi lấy biên lai giao dịch")?;
        Ok(Some(receipt))
    }
    
    /// Swap chính xác ETH sang token
    pub async fn swap_exact_eth_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        let recipient_addr = Address::from_str(recipient)
            .context(format!("Địa chỉ người nhận không hợp lệ: {}", recipient))?;
        
        // Tạo client với ví
        let client = Arc::new(self.get_client()?);
        
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?;
        let router_contract = Contract::new(router_addr, router_abi.clone(), client);
        
        // Tạo path cho swap
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)
            .context(format!("Địa chỉ wrapped token không hợp lệ: {}", self.config.wrapped_native_token))?;
        let path = vec![weth_addr, token_addr];
        
        // Chuẩn bị tx
        let mut tx = router_contract.method(
            &self.config.eth_to_token_swap_fn, 
            (min_amount_out, path, recipient_addr, U256::from(deadline))
        ).context(format!("Không thể tạo giao dịch swap với method: {}", self.config.eth_to_token_swap_fn))?;
        
        // Thêm value và gas
        tx.tx.set_value(amount_in);
        
        if let Some(limit) = gas_limit {
            tx.tx.set_gas(limit);
        }
        
        if let Some(price) = gas_price {
            tx.tx.set_gas_price(U256::from(price));
        }
        
        // Gửi giao dịch
        let pending_tx = tx.send().await
            .context("Lỗi khi gửi giao dịch swap")?;
        
        // Đợi biên lai
        let receipt = pending_tx.await
            .context("Lỗi khi lấy biên lai giao dịch")?;
        Ok(Some(receipt))
    }
    
    /// Swap chính xác token sang ETH
    pub async fn swap_exact_tokens_for_eth(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        let recipient_addr = Address::from_str(recipient)
            .context(format!("Địa chỉ người nhận không hợp lệ: {}", recipient))?;
        
        // Tạo client với ví
        let client = Arc::new(self.get_client()?);
        
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?;
        let router_contract = Contract::new(router_addr, router_abi.clone(), client);
        
        // Tạo path cho swap
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)
            .context(format!("Địa chỉ wrapped token không hợp lệ: {}", self.config.wrapped_native_token))?;
        let path = vec![token_addr, weth_addr];
        
        // Chuẩn bị tx
        let mut tx = router_contract.method(
            &self.config.token_to_eth_swap_fn, 
            (amount_in, min_amount_out, path, recipient_addr, U256::from(deadline))
        ).context(format!("Không thể tạo giao dịch swap với method: {}", self.config.token_to_eth_swap_fn))?;
        
        // Thêm gas
        if let Some(limit) = gas_limit {
            tx.tx.set_gas(limit);
        }
        
        if let Some(price) = gas_price {
            tx.tx.set_gas_price(U256::from(price));
        }
        
        // Gửi giao dịch
        let pending_tx = tx.send().await
            .context("Lỗi khi gửi giao dịch swap")?;
        
        // Đợi biên lai
        let receipt = pending_tx.await
            .context("Lỗi khi lấy biên lai giao dịch")?;
        Ok(Some(receipt))
    }
    
    /// Lấy giá swap dự kiến
    pub async fn get_amounts_out(&self, amount_in: U256, path: Vec<Address>) -> Result<Vec<U256>> {
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?;
        let router_contract = Contract::new(router_addr, router_abi.clone(), self.provider.clone());
        
        // Gọi hàm getAmountsOut
        let amounts: Vec<U256> = router_contract.method("getAmountsOut", (amount_in, path))?
            .call().await
            .context("Lỗi khi gọi getAmountsOut")?;
        Ok(amounts)
    }
    
    /// Kiểm tra sự tồn tại của cặp token
    pub async fn get_pair(&self, token_a: &str, token_b: &str) -> Result<Option<String>> {
        let token_a_addr = Address::from_str(token_a)
            .context(format!("Địa chỉ token A không hợp lệ: {}", token_a))?;
        let token_b_addr = Address::from_str(token_b)
            .context(format!("Địa chỉ token B không hợp lệ: {}", token_b))?;
        
        // Tạo contract Factory
        let factory_addr = Address::from_str(&self.config.factory_address)
            .context(format!("Địa chỉ factory không hợp lệ: {}", self.config.factory_address))?;
        let factory_abi = self.contract_abis.get("factory")
            .ok_or_else(|| anyhow!("Không tìm thấy Factory ABI"))?;
        let factory_contract = Contract::new(factory_addr, factory_abi.clone(), self.provider.clone());
        
        // Gọi hàm getPair
        let pair_addr: Address = factory_contract.method("getPair", (token_a_addr, token_b_addr))?
            .call().await
            .context("Lỗi khi gọi getPair")?;
        
        // Kiểm tra nếu địa chỉ là zero address
        if pair_addr == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(format!("{:?}", pair_addr)))
        }
    }
    
    /// Tạo path từ native token đến token
    pub fn get_native_to_token_path(&self, token_address: &str) -> Result<Vec<Address>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)
            .context(format!("Địa chỉ wrapped token không hợp lệ: {}", self.config.wrapped_native_token))?;
        
        Ok(vec![weth_addr, token_addr])
    }
    
    /// Tạo path từ token đến native token
    pub fn get_token_to_native_path(&self, token_address: &str) -> Result<Vec<Address>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)
            .context(format!("Địa chỉ wrapped token không hợp lệ: {}", self.config.wrapped_native_token))?;
        
        Ok(vec![token_addr, weth_addr])
    }
    
    /// Tạo FlashBots bundle để gửi giao dịch qua mạng FlashBots
    pub async fn create_flashbots_bundle(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        if self.config.name == "Ethereum" {
            // Triển khai FlashBots cho Ethereum
            info!("Tạo FlashBots bundle cho {}", self.config.name);
            
            // Kiểm tra ví người dùng
            let wallet = self.get_wallet_with_chain_id()?;
            
            // Lấy URL FlashBots endpoint
            let flashbots_url = "https://relay.flashbots.net";
            
            // Tạo FlashBots provider
            let provider = self.provider.clone();
            let flashbots_signer = wallet.clone();
            
            // Tạo Flashbots provider
            let flashbots = match ethers::providers::FlashbotsMiddleware::new(
                provider,
                Url::parse(flashbots_url)?,
                flashbots_signer,
            ) {
                Ok(fb) => fb,
                Err(e) => return Err(anyhow!("Không thể tạo Flashbots provider: {}", e)),
            };
            
            // Tham số cho bundle
            let block_number = self.provider.get_block_number().await?;
            let target_block = block_number + 1;
            
            // Tạo bundle từ các giao dịch
            let mut bundle_txs = Vec::new();
            for tx in txs {
                // Clone giao dịch và cập nhật các trường cần thiết
                let mut bundle_tx = tx.clone();
                
                // Bỏ qua gas price và max fee per gas để FlashBots tối ưu
                bundle_tx.gas_price = None;
                bundle_tx.max_fee_per_gas = None;
                
                // Ký giao dịch
                let signed_tx = wallet.sign_transaction(&bundle_tx).await?;
                bundle_txs.push(signed_tx.into());
            }
            
            // Tạo bundle request
            let bundle = FlashbotsBundle {
                transactions: bundle_txs,
                target_block,
                min_block: target_block,
                max_block: target_block + 3, // Thử trong 3 block
                revert_on_fail: true,
            };
            
            // Gửi bundle
            match flashbots.send_bundle(&bundle).await {
                Ok(bundle_hash) => {
                    info!("Đã gửi Flashbots bundle thành công: {:?}", bundle_hash);
                    
                    // Bắt đầu theo dõi bundle
                    let bundle_stats = flashbots.get_bundle_stats(bundle_hash).await?;
                    debug!("Bundle stats: {:?}", bundle_stats);
                    
                    // Lưu thông tin bundle vào cache để theo dõi sau này
                    self.cache_bundle_info(bundle_hash, bundle.target_block, bundle_txs.clone())?;
                    
                    Ok(())
                },
                Err(e) => Err(anyhow!("Lỗi khi gửi Flashbots bundle: {}", e)),
            }
        } else {
            // Đối với các chain khác, kiểm tra nếu có FlashBots tương đương
            match self.config.name.as_str() {
                "BSC" => {
                    info!("Sử dụng MEV Protection trên BSC...");
                    self.send_bundle_to_bsc_mev_relay(txs).await
                },
                "Avalanche" => {
                    info!("Sử dụng Subnet protection trên Avalanche...");
                    self.send_bundle_to_avalanche_subnet(txs).await
                },
                _ => {
                    info!("FlashBots không được hỗ trợ trên {}, sử dụng giao dịch thông thường", self.config.name);
                    // Gửi giao dịch thông thường
                    self.send_transactions_sequentially(txs).await
                }
            }
        }
    }
    
    /// Lưu thông tin bundle vào cache
    fn cache_bundle_info(&self, bundle_hash: H256, target_block: u64, txs: Vec<TypedTransaction>) -> Result<()> {
        // Lấy danh sách transaction hash
        let tx_hashes: Vec<H256> = txs.iter()
            .filter_map(|tx| tx.hash())
            .collect();
            
        // Tạo thông tin bundle
        let bundle_info = BundleInfo {
            bundle_hash,
            transaction_hashes: tx_hashes,
            target_block: target_block.into(),
            created_at: utils::safe_now(), // Sử dụng hàm an toàn từ utils
        };
        
        // Lưu vào cache
        match self.cache.try_write() {
            Ok(mut cache) => {
                let key = format!("bundle_{:?}", bundle_hash);
                let json_value = serde_json::to_value(&bundle_info)?;
                cache.insert(key, (Instant::now(), json_value));
                Ok(())
            },
            Err(_) => {
                warn!("Không thể lấy write lock cho cache khi lưu bundle info");
                Err(anyhow!("Không thể lấy write lock cho cache"))
            }
        }
    }
    
    /// Gửi bundle đến BSC MEV Relay
    async fn send_bundle_to_bsc_mev_relay(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        // TODO: Triển khai gửi bundle đến BSC MEV Relay khi có
        Err(anyhow!("BSC MEV Relay chưa được hỗ trợ đầy đủ"))
    }
    
    /// Gửi bundle đến Avalanche Subnet
    async fn send_bundle_to_avalanche_subnet(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        // TODO: Triển khai gửi bundle đến Avalanche Subnet khi có
        Err(anyhow!("Avalanche Subnet protection chưa được hỗ trợ đầy đủ"))
    }
    
    /// Gửi các giao dịch theo thứ tự
    async fn send_transactions_sequentially(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        info!("Gửi {} giao dịch theo thứ tự", txs.len());
        let client = self.get_client()?;
        
        for (i, tx) in txs.iter().enumerate() {
            info!("Gửi giao dịch {}/{}", i+1, txs.len());
            
            let tx_hash = client.send_transaction(tx.clone(), None).await?;
            info!("Đã gửi giao dịch {}: {:?}", i+1, tx_hash);
            
            // Đợi giao dịch được xác nhận trước khi gửi giao dịch tiếp theo
            let receipt = client.pending_transaction(tx_hash)
                .confirmations(1)
                .await?;
                
            match receipt {
                Some(r) => {
                    if r.status == Some(1.into()) {
                        info!("Giao dịch {} thành công", i+1);
                    } else {
                        warn!("Giao dịch {} thất bại", i+1);
                        return Err(anyhow!("Giao dịch trong chuỗi thất bại"));
                    }
                },
                None => {
                    warn!("Không nhận được biên lai cho giao dịch {}", i+1);
                }
            }
        }
        
        Ok(())
    }
    
    /// Theo dõi giao dịch chờ xử lý (chưa triển khai chi tiết)
    pub async fn watch_pending_transactions(&self, callback: Box<dyn Fn(Transaction) + Send + Sync>) -> Result<()> {
        info!("Chức năng theo dõi mempool trên {} sẽ được triển khai sau", self.config.name);
        Ok(())
    }
    
    /// Theo dõi mempool để phát hiện các giao dịch liên quan đến một token
    pub async fn watch_token_transactions(&self, 
        token_address: &str, 
        callback: Box<dyn Fn(Transaction) + Send + Sync + 'static>
    ) -> Result<tokio::task::JoinHandle<()>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        
        // Tạo WebSocket provider để nhận các transaction mới từ mempool
        let ws_rpc_url = self.config.rpc_url.replace("http", "ws");
        let ws_provider = match Provider::<ethers::providers::Ws>::connect(&ws_rpc_url).await {
            Ok(provider) => provider,
            Err(e) => {
                // Nếu WebSocket không khả dụng, thử polling
                info!("WebSocket không khả dụng cho {}, sử dụng HTTP polling: {}", self.config.name, e);
                return self.watch_token_transactions_with_polling(token_address, callback).await;
            }
        };
        
        // Subscribe đến các pending transaction
        let stream = match ws_provider.subscribe_pending_txs().await {
            Ok(stream) => stream,
            Err(e) => {
                info!("Không thể subscribe pending txs cho {}, sử dụng HTTP polling: {}", self.config.name, e);
                return self.watch_token_transactions_with_polling(token_address, callback).await;
            }
        };
        
        // Provider để lấy chi tiết transaction
        let provider = self.provider.clone();
        let chain_name = self.config.name.clone();
        let token_addr_str = token_address.to_string();
        
        // Lấy router address
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
        
        // Tạo task để xử lý stream
        let handle = tokio::spawn(async move {
            let mut stream = stream;
            
            info!("Đã bắt đầu theo dõi mempool cho token {} trên {}", token_addr_str, chain_name);
            
            while let Some(tx_hash) = stream.next().await {
                // Lấy chi tiết transaction
                if let Ok(tx) = provider.get_transaction(tx_hash).await {
                    if let Some(tx) = tx {
                        // Kiểm tra nếu giao dịch đến router
                        if tx.to == Some(router_addr) {
                            // Kiểm tra xem có liên quan đến token không
                            if let Some(input) = &tx.input.0 {
                                let input_str = hex::encode(input);
                                // Kiểm tra xem input có chứa địa chỉ token không
                                if input_str.contains(&token_addr.to_string()[2..].to_lowercase()) {
                                    callback(tx);
                                }
                            }
                        }
                        // Kiểm tra nếu giao dịch đến token
                        else if tx.to == Some(token_addr) {
                            callback(tx);
                        }
                    }
                }
            }
            
            info!("Đã dừng theo dõi mempool cho token {} trên {}", token_addr_str, chain_name);
        });
        
        Ok(handle)
    }
    
    /// Theo dõi mempool bằng HTTP polling khi WebSocket không khả dụng
    pub async fn watch_token_transactions_with_polling(&self, 
        token_address: &str, 
        callback: Box<dyn Fn(Transaction) + Send + Sync + 'static>
    ) -> Result<tokio::task::JoinHandle<()>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
        
        let provider = self.provider.clone();
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
        
        let chain_name = self.config.name.clone();
        let token_addr_str = token_address.to_string();
        let block_time = self.config.block_time;
        
        // Tạo task để polling các block mới
        let handle = tokio::spawn(async move {
            let mut last_block = 0u64;
            
            info!("Đã bắt đầu theo dõi mempool bằng polling cho token {} trên {}", token_addr_str, chain_name);
            
            loop {
                // Đợi khoảng thời gian bằng block time
                let wait_time = if block_time < 1000 { 1 } else { block_time / 1000 };
                tokio::time::sleep(tokio::time::Duration::from_secs(wait_time)).await;
                
                // Lấy block hiện tại
                if let Ok(block) = provider.get_block_number().await {
                    let current_block = block.as_u64();
                    
                    // Nếu có block mới
                    if current_block > last_block {
                        for block_num in last_block+1..=current_block {
                            if let Ok(Some(block)) = provider.get_block_with_txs(block_num.into()).await {
                                // Xử lý từng transaction trong block
                                for tx in block.transactions {
                                    // Kiểm tra nếu giao dịch đến router
                                    if tx.to == Some(router_addr) {
                                        // Kiểm tra xem có liên quan đến token không
                                        if let Some(input) = &tx.input.0 {
                                            let input_str = hex::encode(input);
                                            // Kiểm tra xem input có chứa địa chỉ token không
                                            if input_str.contains(&token_addr.to_string()[2..].to_lowercase()) {
                                                callback(tx);
                                            }
                                        }
                                    }
                                    // Kiểm tra nếu giao dịch đến token
                                    else if tx.to == Some(token_addr) {
                                        callback(tx);
                                    }
                                }
                            }
                        }
                        
                        last_block = current_block;
                    }
                }
            }
        });
        
        Ok(handle)
    }
    
    /// Theo dõi mempool để phát hiện các giao dịch sandwich có thể
    pub async fn watch_for_sandwich_opportunities(&self, 
        token_address: &str,
        min_amount: U256,
        callback: Box<dyn Fn(Transaction, U256) + Send + Sync + 'static>
    ) -> Result<tokio::task::JoinHandle<()>> {
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
            
        // Tạo WebSocket provider để nhận các transaction mới từ mempool
        let ws_rpc_url = self.config.rpc_url.replace("http", "ws");
        let ws_provider = match Provider::<ethers::providers::Ws>::connect(&ws_rpc_url).await {
            Ok(provider) => provider,
            Err(e) => {
                // Nếu WebSocket không khả dụng, trả về lỗi
                return Err(anyhow!("WebSocket không khả dụng cho sandwich: {}", e));
            }
        };
        
        // Subscribe đến các pending transaction
        let stream = match ws_provider.subscribe_pending_txs().await {
            Ok(stream) => stream,
            Err(e) => {
                return Err(anyhow!("Không thể subscribe pending txs: {}", e));
            }
        };
        
        // Provider để lấy chi tiết transaction
        let provider = self.provider.clone();
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
            
        // Lấy ABI của router
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?
            .clone();
            
        // Tạo task để xử lý stream
        let handle = tokio::spawn(async move {
            let mut stream = stream;
            
            while let Some(tx_hash) = stream.next().await {
                // Lấy chi tiết transaction
                if let Ok(tx) = provider.get_transaction(tx_hash).await {
                    if let Some(tx) = tx {
                        // Kiểm tra nếu giao dịch đến router và có giá trị tối thiểu
                        if tx.to == Some(router_addr) && (tx.value >= min_amount) {
                            // Phân tích function call
                            if let Some(input) = &tx.input.0 {
                                // Mặc định tất cả các hàm swap đều có 4 byte đầu là function selector
                                if input.len() > 4 {
                                    let selector = &input[0..4];
                                    
                                    // Phân tích transaction dựa trên selector
                                    if let Ok(function) = router_abi.functions().find(|f| {
                                        let computed_selector = f.short_signature();
                                        selector == computed_selector.as_slice()
                                    }) {
                                        // Nếu là hàm swap
                                        if function.name.contains("swap") {
                                            // Thử decode tham số
                                            if let Ok(decoded) = function.decode_input(&input[4..]) {
                                                // Tìm tham số path
                                                for param in decoded {
                                                    if let Some(path) = param.into_array() {
                                                        // Kiểm tra nếu path chứa token
                                                        for addr in path {
                                                            if let Some(address) = addr.into_address() {
                                                                if address == token_addr {
                                                                    // Gọi callback với số lượng token
                                                                    callback(tx.clone(), tx.value);
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        
        Ok(handle)
    }
    
    /// Giải mã input của router transaction
    pub fn decode_router_input(&self, input: &[u8]) -> Result<Vec<ethers::abi::Token>> {
        // Lấy ABI của router
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?;
            
        // Mặc định tất cả các hàm swap đều có 4 byte đầu là function selector
        if input.len() > 4 {
            let selector = &input[0..4];
            
            // Tìm function dựa trên selector
            if let Some(function) = router_abi.functions().find(|f| {
                let computed_selector = f.short_signature();
                selector == computed_selector.as_slice()
            }) {
                // Decode tham số
                let decoded = function.decode_input(&input[4..])?;
                return Ok(decoded);
            }
        }
        
        Err(anyhow!("Không thể giải mã input của giao dịch"))
    }

    /// Gửi giao dịch với cơ chế thử lại
    pub async fn send_transaction_with_retry(
        &self,
        tx: TypedTransaction,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
        operation_name: &str,
    ) -> Result<TransactionReceipt, TransactionError> {
        use crate::chain_adapters::retry::{retry_blockchain_operation, BlockchainError};
        
        // Tạo client với ví
        let client = self.get_client()
            .map_err(|e| TransactionError::Other(e.to_string()))?;
        
        // Sử dụng NonceManager để lấy nonce tiếp theo
        let wallet_address = self.wallet.as_ref().map(|w| w.address()).unwrap_or_default();
        
        // Chuẩn bị transaction
        let mut modified_tx = tx.clone();
        
        if let Some(limit) = gas_limit {
            modified_tx.set_gas(limit);
        }
        
        // Sử dụng retry với gas price tự động điều chỉnh
        retry_blockchain_operation(
            operation_name,
            |gas_override| async {
                // Lấy nonce từ NonceManager
                let nonce = self.nonce_manager.get_next_nonce(wallet_address).await?;
                
                // Set nonce vào transaction
                let mut tx_to_send = modified_tx.clone();
                tx_to_send.set_nonce(nonce);
                
                // Set gas price nếu có
                let gas_to_use = gas_override.unwrap_or_else(|| {
                    if let Some(gp) = gas_price {
                        U256::from(gp)
                    } else {
                        U256::from(self.config.default_gas_price)
                    }
                });
                
                tx_to_send.set_gas_price(gas_to_use);
                
                // Khi gọi send_transaction, middlewares trong ethers sẽ tự ký transaction
                debug!("Gửi giao dịch với nonce {}, gas price {}", nonce, gas_to_use);
                
                // Gửi transaction
                let pending_tx_result = client.send_transaction(tx_to_send, None).await;
                let pending_tx = match pending_tx_result {
                    Ok(tx) => tx,
                    Err(e) => {
                        // Xử lý lỗi nonce
                        if e.to_string().contains("nonce too low") ||
                           e.to_string().contains("already known") ||
                           e.to_string().contains("replacement transaction underpriced") {
                            // Reset nonce từ blockchain
                            if let Err(reset_err) = self.nonce_manager.reset_nonce(wallet_address).await {
                                error!("Không thể reset nonce: {}", reset_err);
                            }
                        }
                        return Err(anyhow::anyhow!(e));
                    }
                };
                
                // Đợi transaction được xác nhận
                let receipt_result = pending_tx.await;
                let receipt = match receipt_result {
                    Ok(receipt_opt) => receipt_opt.ok_or_else(|| anyhow::anyhow!("Không có transaction receipt"))?,
                    Err(e) => return Err(anyhow::anyhow!(e)),
                };
                
                // Cập nhật nonce mới (nonce tiếp theo là nonce hiện tại + 1)
                if let Some(tx_nonce) = receipt.transaction_nonce {
                    let _ = self.nonce_manager.update_nonce(
                        wallet_address, 
                        tx_nonce + U256::from(1)
                    ).await;
                }
                
                Ok(receipt)
            },
            // Sử dụng gas price từ input hoặc default
            gas_price.map(U256::from),
            3, // 3 lần thử lại
        ).await.map_err(|e| {
            error!("Lỗi khi gửi giao dịch: {}", e);
            TransactionError::from_anyhow(e)
        })
    }
    
    /// Lấy client với RPC rotation
    pub async fn get_provider_with_rotation(&self) -> Result<Provider<Http>> {
        use crate::chain_adapters::retry::with_rpc_rotation;
        
        // Nếu có RPC pool, sử dụng nó
        if let Some(pool) = &self.rpc_pool {
            let best_url = pool.get_best_url().await?;
            let provider = Provider::<Http>::try_from(&best_url)
                .context(format!("Không thể kết nối đến RPC: {}", best_url))?;
            return Ok(provider);
        }
        
        // Fallback nếu không có pool
        Ok(self.provider.clone())
    }
    
    /// Thực hiện swap ETH sang tokens
    pub async fn swap_eth_for_tokens(&self, amount: U256, token_address: &str, slippage: f64) -> Result<String> {
        use crate::chain_adapters::retry::retry_blockchain_operation;
        
        let token_addr = Address::from_str(token_address)
            .context(format!("Địa chỉ token không hợp lệ: {}", token_address))?;
            
        // Lấy đường dẫn từ ETH đến token
        let path = self.get_native_to_token_path(token_address)?;
        
        // Tạo client với ví
        let client = self.get_client()
            .context("Không có ví cho adapter")?;
            
        // Lấy address của router
        let router_addr = Address::from_str(&self.config.router_address)
            .context(format!("Địa chỉ router không hợp lệ: {}", self.config.router_address))?;
            
        // Lấy ABI router
        let router_abi = self.contract_abis.get("router")
            .ok_or_else(|| anyhow!("Không tìm thấy Router ABI"))?
            .clone();
            
        // Tạo contract router
        let router_contract = Contract::new(router_addr, router_abi, Arc::new(client.clone()));
        
        // Lấy số lượng token tối thiểu (với slippage)
        let amounts = self.get_amounts_out(amount, path.clone()).await
            .context("Không thể lấy amounts out cho swap")?;
        
        let min_amount_out = if amounts.len() > 1 {
            let amount_out = amounts[amounts.len() - 1];
            // Áp dụng slippage
            amount_out - (amount_out * U256::from((slippage * 100.0) as u64) / U256::from(10000))
        } else {
            return Err(anyhow!("Swap path không hợp lệ"));
        };
        
        // Lấy địa chỉ ví
        let address = if let Some(wallet) = &self.wallet {
            wallet.address()
        } else {
            return Err(anyhow!("Không có ví cho adapter"));
        };
        
        // Lấy deadline (thời gian hiện tại + 20 phút)
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() + 1200;
        
        // Thực hiện swap với retry
        let tx_hash = retry_blockchain_operation(
            "swap_eth_for_tokens",
            |gas_price| async {
                // Tên hàm swap từ config
                let swap_fn = &self.config.eth_to_token_swap_fn;
                
                // Tạo tham số swap
                let swap_params = match swap_fn.as_str() {
                    "swapExactETHForTokens" => (min_amount_out, path.clone(), address, U256::from(deadline)),
                    "swapExactAVAXForTokens" => (min_amount_out, path.clone(), address, U256::from(deadline)),
                    _ => return Err(anyhow!("Hàm swap không được hỗ trợ: {}", swap_fn)),
                };
                
                // Thực hiện gọi hàm swap
                let mut call = router_contract.method::<_, H256>(swap_fn, swap_params)?;
                
                // Thêm giá trị ETH và gas
                call = call.value(amount);
                
                // Nếu có gas price override
                if let Some(gas) = gas_price {
                    if self.config.eip1559_supported {
                        // Sử dụng EIP-1559 nếu được hỗ trợ
                        let priority_fee = self.config.max_priority_fee
                            .map(|fee| ethers::utils::parse_units(fee.to_string(), "gwei").unwrap_or(U256::from(1_500_000_000)))
                            .unwrap_or(U256::from(1_500_000_000)); // 1.5 gwei
                        
                        call = call.gas_price(gas)
                            .max_priority_fee_per_gas(priority_fee);
                    } else {
                        call = call.gas_price(gas);
                    }
                }
                
                // Chạy transaction
                let tx = call.send().await?;
                let tx_hash = tx.tx_hash();
                
                Ok(format!("{:?}", tx_hash))
            },
            Some(U256::from(self.config.default_gas_price)),
            3
        ).await?;
        
        Ok(tx_hash)
    }
}

/// Enum quản lý các chain sử dụng EVMAdapter
#[derive(Debug, Clone)]
pub enum ChainAdapterEnum {
    Ethereum(Arc<EVMAdapter>),
    BSC(Arc<EVMAdapter>),
    Avalanche(Arc<EVMAdapter>),
    Base(Arc<EVMAdapter>),
    Monad(Arc<EVMAdapter>),
    Arbitrum(Arc<EVMAdapter>),
    Optimism(Arc<EVMAdapter>),
    Polygon(Arc<EVMAdapter>),
    Custom(String, Arc<EVMAdapter>),
}

/// Registry cho các chain adapters
pub struct ChainRegistry {
    adapters: HashMap<String, ChainAdapterEnum>,
}

impl ChainRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }
    
    pub fn register(&mut self, name: &str, adapter: ChainAdapterEnum) {
        self.adapters.insert(name.to_lowercase(), adapter);
    }
    
    pub fn get(&self, name: &str) -> Option<&ChainAdapterEnum> {
        self.adapters.get(&name.to_lowercase())
    }
}

/// Singleton registry
pub static CHAIN_REGISTRY: Lazy<RwLock<ChainRegistry>> = Lazy::new(|| {
    RwLock::new(ChainRegistry::new())
});

/// Macro tự động tạo match statements cho tất cả variants của ChainAdapterEnum
#[macro_export]
macro_rules! chain_variant_match {
    ($enum_var:expr, $adapter_var:ident, $body:expr) => {
        match $enum_var {
            ChainAdapterEnum::Ethereum($adapter_var) => $body,
            ChainAdapterEnum::BSC($adapter_var) => $body,
            ChainAdapterEnum::Avalanche($adapter_var) => $body,
            ChainAdapterEnum::Base($adapter_var) => $body,
            ChainAdapterEnum::Monad($adapter_var) => $body,
            ChainAdapterEnum::Arbitrum($adapter_var) => $body,
            ChainAdapterEnum::Optimism($adapter_var) => $body,
            ChainAdapterEnum::Polygon($adapter_var) => $body,
            ChainAdapterEnum::Custom(_, $adapter_var) => $body,
        }
    };
}

/// Macro tự động tạo variant mới cho ChainAdapterEnum khi thêm chain mới
#[macro_export]
macro_rules! define_chain_enum_variant {
    ($name:ident, $adapter_type:ty) => {
        pub enum ChainAdapterEnum {
            Ethereum(Arc<$adapter_type>),
            BSC(Arc<$adapter_type>),
            Avalanche(Arc<$adapter_type>),
            Base(Arc<$adapter_type>),
            Monad(Arc<$adapter_type>),
            Arbitrum(Arc<$adapter_type>),
            Optimism(Arc<$adapter_type>),
            Polygon(Arc<$adapter_type>),
            Custom(String, Arc<$adapter_type>),
            $name(Arc<$adapter_type>),
        }
    };
}

/// Macro tạo các phương thức giống nhau cho toàn bộ enum variants
#[macro_export]
macro_rules! impl_chain_adapter_method {
    ($method:ident, $return_type:ty, $($param_name:ident: $param_type:ty),*) => {
        pub async fn $method(&self, $($param_name: $param_type),*) -> $return_type {
            chain_variant_match!(self, adapter, adapter.$method($($param_name),*).await)
        }
    };
    ($method:ident, $return_type:ty) => {
        pub async fn $method(&self) -> $return_type {
            chain_variant_match!(self, adapter, adapter.$method().await)
        }
    };
}

/// Macro tạo các phương thức đồng bộ cho toàn bộ enum variants
#[macro_export]
macro_rules! impl_chain_adapter_sync_method {
    ($method:ident, $return_type:ty, $($param_name:ident: $param_type:ty),*) => {
        pub fn $method(&self, $($param_name: $param_type),*) -> $return_type {
            chain_variant_match!(self, adapter, adapter.$method($($param_name),*))
        }
    };
    ($method:ident, $return_type:ty) => {
        pub fn $method(&self) -> $return_type {
            chain_variant_match!(self, adapter, adapter.$method())
        }
    };
}

/// Tạo getter cho config để đảm bảo luôn trả về đúng config
impl ChainAdapterEnum {
    pub fn get_config(&self) -> &ChainConfig {
        chain_variant_match!(self, adapter, &adapter.config)
    }
    
    pub fn get_chain_name(&self) -> String {
        self.get_config().name.clone()
    }
    
    // Sử dụng macro để implement các phương thức async
    impl_chain_adapter_method!(get_native_balance, Result<U256>, address: &str);
    impl_chain_adapter_method!(get_token_balance, Result<U256>, token_address: &str, wallet_address: &str);
    impl_chain_adapter_method!(approve_token, Result<Option<TransactionReceipt>>, token_address: &str, spender_address: &str, amount: U256);
    impl_chain_adapter_method!(swap_exact_eth_for_tokens, Result<Option<TransactionReceipt>>, token_address: &str, amount_in: U256, min_amount_out: U256, recipient: &str, deadline: u64, gas_limit: Option<u64>, gas_price: Option<u64>);
    impl_chain_adapter_method!(swap_exact_tokens_for_eth, Result<Option<TransactionReceipt>>, token_address: &str, amount_in: U256, min_amount_out: U256, recipient: &str, deadline: u64, gas_limit: Option<u64>, gas_price: Option<u64>);
    impl_chain_adapter_method!(get_amounts_out, Result<Vec<U256>>, amount_in: U256, path: Vec<Address>);
    impl_chain_adapter_method!(get_pair, Result<Option<String>>, token_a: &str, token_b: &str);
    impl_chain_adapter_method!(create_flashbots_bundle, Result<()>, txs: Vec<TransactionRequest>);
    impl_chain_adapter_method!(watch_pending_transactions, Result<()>, callback: Box<dyn Fn(Transaction) + Send + Sync>);
    impl_chain_adapter_method!(watch_token_transactions, Result<tokio::task::JoinHandle<()>>, token_address: &str, callback: Box<dyn Fn(Transaction) + Send + Sync + 'static>);
    impl_chain_adapter_method!(watch_for_sandwich_opportunities, Result<tokio::task::JoinHandle<()>>, token_address: &str, min_amount: U256, callback: Box<dyn Fn(Transaction, U256) + Send + Sync + 'static>);
    
    // Phương thức đồng bộ
    impl_chain_adapter_sync_method!(decode_router_input, Result<Vec<ethers::abi::Token>>, input: &[u8]);
    impl_chain_adapter_sync_method!(get_native_to_token_path, Result<Vec<Address>>, token_address: &str);
    impl_chain_adapter_sync_method!(get_token_to_native_path, Result<Vec<Address>>, token_address: &str);
}

impl PartialEq for ChainAdapterEnum {
    fn eq(&self, other: &Self) -> bool {
        // So sánh theo chain name từ config
        self.get_chain_name() == other.get_chain_name()
    }
}

impl Eq for ChainAdapterEnum {}

impl std::hash::Hash for ChainAdapterEnum {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash theo chain name
        self.get_chain_name().hash(state);
    }
}

impl std::fmt::Display for ChainAdapterEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChainAdapter({})", self.get_chain_name())
    }
}

pub async fn create_chain_adapter(chain_name: &str) -> Result<ChainAdapterEnum> {
    match chain_name.to_lowercase().as_str() {
        "ethereum" => {
            let adapter = crate::chain_adapters::ethereum::EthereumAdapter::new().await?;
            Ok(ChainAdapterEnum::Ethereum(adapter))
        },
        "bsc" => {
            let adapter = crate::chain_adapters::bsc::BSCAdapter::new().await?;
            Ok(ChainAdapterEnum::BSC(adapter))
        },
        "base" => {
            let adapter = crate::chain_adapters::base_adapter::BaseAdapter::new().await?;
            Ok(ChainAdapterEnum::Base(adapter))
        },
        "avalanche" => {
            let adapter = crate::chain_adapters::avalanche::AvalancheAdapter::new().await?;
            Ok(ChainAdapterEnum::Avalanche(adapter))
        },
        "monad" => {
            let adapter = crate::chain_adapters::monad::MonadAdapter::new().await?;
            Ok(ChainAdapterEnum::Monad(adapter))
        },
        _ => Err(anyhow::anyhow!("Unsupported chain: {}", chain_name))
    }
}

pub async fn handle_transaction_error(
    e: anyhow::Error, 
    retry_count: u8,
    max_retries: u8
) -> Result<Option<TransactionReceipt>> {
    match classify_blockchain_error(&e) {
        TransactionError::Timeout => {
            if retry_count < max_retries {
                info!("Giao dịch timeout, thử lại lần {}/{}", retry_count + 1, max_retries);
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Đã hết số lần thử lại sau timeout: {}", e))
            }
        },
        TransactionError::Underpriced => {
            if retry_count < max_retries {
                info!("Giao dịch underpriced, thử lại với gas price cao hơn {}/{}", 
                    retry_count + 1, max_retries);
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Đã hết số lần thử lại với underpriced: {}", e))
            }
        },
        TransactionError::InsufficientFunds => {
            error!("Không đủ native token để thực hiện giao dịch: {}", e);
            Err(anyhow::anyhow!("Không đủ native token để thực hiện giao dịch: {}", e))
        },
        TransactionError::NonceTooLow => {
            if retry_count < max_retries {
                info!("Nonce too low, thử lại với nonce mới {}/{}", 
                    retry_count + 1, max_retries);
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Đã hết số lần thử lại với nonce too low: {}", e))
            }
        },
        TransactionError::ExecutionReverted(reason) => {
            error!("Giao dịch bị revert: {}", reason);
            Err(anyhow::anyhow!("Giao dịch bị revert: {}", reason))
        },
        TransactionError::Other(message) => {
            if retry_count < max_retries {
                info!("Lỗi giao dịch: {}, thử lại {}/{}", 
                    message, retry_count + 1, max_retries);
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Đã hết số lần thử lại với lỗi: {}", message))
            }
        },
        TransactionError::Unknown(message) => {
            if retry_count < max_retries {
                info!("Lỗi không xác định: {}, thử lại {}/{}", 
                    message, retry_count + 1, max_retries);
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Đã hết số lần thử lại với lỗi không xác định: {}", message))
            }
        },
    }
}

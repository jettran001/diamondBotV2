// External imports
use ethers::{
    types::{Address, U256, TransactionRequest, Transaction, TransactionReceipt, BlockId, Bytes, Filter, Log},
    providers::Middleware,
    contract::Contract,
    prelude::*,
};

// Standard library imports
use std::{
    time::Instant,
    sync::Arc,
};

// Internal imports
use crate::{
    chain_adapters::{
        interfaces::{ChainAdapter, ChainError, GasInfo, TokenDetails, BlockInfo, NodeInfo},
        retry_policy::{RetryPolicy, RetryContext, create_default_retry_policy},
        chain_registry::{ChainConfig, get_chain_config},
        connection_pool::{get_or_create_pool, ProviderGuard},
    },
    abi_utils,
};

// Third party imports
use anyhow::{Result, anyhow};
use tracing::{info, warn, error};
use async_trait::async_trait;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use wallet::secure_storage::SecureWalletStorage;

/// Triển khai ChainAdapter cho các blockchain tương thích EVM
pub struct EVMChainAdapter {
    /// Chain ID
    chain_id: u64,
    /// Cấu hình chain
    config: ChainConfig,
    /// Retry policy
    retry_policy: RetryPolicyEnum,
    /// Cache cho các kết quả truy vấn
    cache: RwLock<HashMap<String, (Instant, serde_json::Value)>>,
    /// ABIs của các contract
    abis: HashMap<String, ethers::abi::Abi>,
    /// Wallet info
    wallet_address: Option<Address>,
    /// Ví cục bộ
    wallet: Option<LocalWallet>,
}

impl EVMChainAdapter {
    /// Tạo mới một EVMChainAdapter
    pub async fn new(
        chain_id: u64,
        retry_policy: Option<RetryPolicyEnum>,
    ) -> Result<Arc<Self>> {
        // Lấy cấu hình chain
        let config = get_chain_config(chain_id)?;
        
        // Tạo ABIs
        let mut abis = HashMap::new();
        
        // Thêm ABI cơ bản
        abis.insert(
            "erc20".to_string(),
            serde_json::from_str(abi_utils::get_erc20_abi())
                .context("Failed to parse ERC20 ABI")?,
        );
        
        // Thêm router ABI nếu có
        if config.router_contracts.contains_key("default") {
            abis.insert(
                "router".to_string(),
                serde_json::from_str(abi_utils::get_router_abi())
                    .context("Failed to parse Router ABI")?,
            );
        }
        
        // Tạo adapter
        let adapter = Self {
            chain_id,
            config,
            retry_policy: retry_policy.unwrap_or_else(create_default_retry_policy),
            cache: RwLock::new(HashMap::new()),
            abis,
            wallet_address: None,
            wallet: None,
        };
        
        Ok(Arc::new(adapter))
    }
    
    /// Khởi tạo từ một cấu hình
    pub async fn from_config(config: ChainConfig) -> Result<Arc<Self>> {
        Self::new(config.chain_id, None).await
    }
    
    /// Lấy provider từ pool
    async fn get_provider(&self) -> Result<ProviderGuard> {
        let pool = get_or_create_pool(
            self.chain_id,
            self.config.primary_rpc_urls.clone(),
            self.config.backup_rpc_urls.clone(),
            self.config.connection_pool_config.clone(),
        ).await?;
        
        let provider = pool.write().await.get_provider().await?;
        
        Ok(provider)
    }
    
    /// Thêm ví vào adapter
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = Some(wallet.with_chain_id(self.chain_id));
        self.wallet_address = Some(self.wallet.as_ref().unwrap().address());
        self
    }
    
    /// Thêm ví từ secure storage
    pub async fn with_wallet_from_storage(
        mut self,
        storage: Arc<RwLock<SecureWalletStorage>>,
        address: &str,
    ) -> Result<Self> {
        // Kiểm tra địa chỉ hợp lệ
        let address = SecureWalletStorage::validate_wallet_address(address)
            .map(|_| address.to_string())?;
        
        // Lấy thông tin ví từ storage
        let storage = storage.read().await;
        let wallet_info = storage.get_wallet(&address)
            .ok_or_else(|| anyhow!("Wallet not found: {}", address))?;
        
        // Chuyển đổi sang LocalWallet
        let wallet = storage.to_local_wallet(wallet_info)?
            .with_chain_id(self.chain_id);
        
        // Cập nhật wallet
        self.wallet = Some(wallet);
        self.wallet_address = Some(
            Address::from_str(&wallet_info.address)
                .context("Invalid wallet address format")?
        );
        
        Ok(self)
    }
    
    /// Lấy ABI cho một contract
    fn get_abi(&self, name: &str) -> Result<&ethers::abi::Abi> {
        self.abis.get(name)
            .ok_or_else(|| anyhow!("ABI not found: {}", name))
    }
    
    /// Tạo contract instance với provider
    async fn create_contract<T: Middleware + 'static>(
        &self,
        address: Address,
        abi_name: &str,
        provider: T,
    ) -> Result<Contract<T>> {
        let abi = self.get_abi(abi_name)?;
        Ok(Contract::new(address, abi.clone(), provider))
    }
    
    /// Mã hóa tham số giao dịch cho router
    fn encode_router_transaction(
        &self,
        function_name: &str,
        params: Vec<ethers::abi::Token>,
    ) -> Result<Bytes> {
        let router_abi = self.get_abi("router")?;
        
        // Lấy hàm từ ABI
        let function = router_abi.functions()
            .into_iter()
            .find(|f| f.name == function_name)
            .ok_or_else(|| anyhow!("Function not found: {}", function_name))?;
        
        // Mã hóa tham số
        let encoded = function.encode_input(&params)?;
        
        Ok(encoded.into())
    }
    
    /// Ước tính gas price dựa vào cấu hình
    fn estimate_gas_price(&self) -> Result<U256> {
        let gas_config = &self.config.gas_config;
        let base_fee_gwei = gas_config.base_fee;
        let priority_fee_gwei = gas_config.priority_fee;
        
        // Chuyển từ gwei sang wei
        let gwei_to_wei = U256::from(10).pow(U256::from(9));
        
        if gas_config.supports_eip1559 {
            // EIP-1559: Trả về max_fee_per_gas và max_priority_fee_per_gas
            let max_fee_per_gas = U256::from_f64_lossy(base_fee_gwei) * gwei_to_wei;
            let max_priority_fee_per_gas = U256::from_f64_lossy(priority_fee_gwei) * gwei_to_wei;
            
            Ok(max_fee_per_gas)
        } else {
            // Legacy: Trả về gas_price = base_fee + priority_fee
            let gas_price = U256::from_f64_lossy(base_fee_gwei + priority_fee_gwei) * gwei_to_wei;
            
            Ok(gas_price)
        }
    }

    pub async fn send_transaction_with_retry(
        &self,
        tx: TypedTransaction,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
        operation_name: &str,
    ) -> Result<TransactionReceipt, TransactionError> {
        // Clone dữ liệu cần thiết trước khi await
        let tx_clone = tx.clone();
        let provider = self.get_provider().await?;
        let retry_policy = self.retry_policy.clone();

        let context = RetryContext::new(
            operation_name,
            &provider.endpoint_info.url,
            self.chain_id,
            gas_price.map(U256::from),
        );

        retry_policy.retry(
            Box::new(move || {
                let tx = tx_clone.clone();
                let provider = provider.provider.clone();
                
                Box::pin(async move {
                    let tx_hash = provider.send_transaction(tx, None).await?;
                    let receipt = provider.wait_for_transaction_receipt(tx_hash, 2).await?;
                    Ok(receipt)
                })
            }),
            &context,
        ).await.map_err(|e| TransactionError::Unknown(e.to_string()))
    }

    pub async fn get_provider_with_rotation(&self) -> Result<Provider<Http>> {
        // Clone dữ liệu cần thiết
        let rpc_pool = self.get_provider().await?;
        
        Ok(rpc_pool.provider.clone())
    }

    pub async fn swap_eth_for_tokens(
        &self,
        amount: U256,
        token_address: &str,
        slippage: f64,
    ) -> Result<String> {
        // Clone dữ liệu cần thiết
        let provider = self.get_provider().await?;
        let token_addr = token_address.to_string();
        let wallet = self.wallet.clone().ok_or_else(|| anyhow!("No wallet configured"))?;

        // Tạo scope riêng cho mutable borrow
        let path = {
            let path = self.get_native_to_token_path(&token_addr)?;
            path
        };

        // Tính toán amounts
        let amounts = self.get_amounts_out(amount, path.clone()).await?;
        let min_amount_out = calculate_min_amount_out(amounts[1], slippage);

        // Tạo và gửi transaction
        let tx = self.create_swap_tx(
            amount,
            min_amount_out,
            path,
            wallet.address(),
        ).await?;

        let receipt = self.send_transaction_with_retry(
            tx,
            None,
            None,
            "swap_eth_for_tokens",
        ).await?;

        Ok(receipt.transaction_hash.to_string())
    }
}

#[async_trait]
impl ChainAdapter for EVMChainAdapter {
    async fn get_block_number(&self) -> Result<u64, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_block_number",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy block number với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_block_number()
                    .await
                    .map(|bn| bn.as_u64())
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_gas_price(&self) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_gas_price",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy gas price với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_gas_price()
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    fn get_chain_id(&self) -> u64 {
        self.chain_id
    }
    
    fn get_type(&self) -> String {
        "EVM".to_string()
    }
    
    async fn get_block(&self, block_id: BlockId) -> Result<Option<BlockInfo>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_block",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy block với retry
        let block_opt = self.retry_policy.retry(
            || async {
                provider.provider.get_block(block_id)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))?;
        
        // Chuyển đổi dữ liệu
        if let Some(block) = block_opt {
            let block_info = BlockInfo {
                number: block.number.map(|n| n.as_u64()).unwrap_or_default(),
                hash: block.hash.map(|h| format!("{:?}", h)).unwrap_or_default(),
                timestamp: block.timestamp.as_u64(),
                transaction_count: block.transactions.len(),
                gas_limit: block.gas_limit,
                gas_used: block.gas_used,
                base_fee_per_gas: block.base_fee_per_gas,
            };
            
            Ok(Some(block_info))
        } else {
            Ok(None)
        }
    }
    
    async fn get_gas_info(&self) -> Result<GasInfo, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Kiểm tra xem chain có hỗ trợ EIP-1559 không
        let eip1559_supported = self.config.gas_config.supports_eip1559;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_gas_info",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        if eip1559_supported {
            // Lấy block mới nhất để có base fee
            let block = self.retry_policy.retry(
                || async {
                    provider.provider.get_block(BlockNumber::Latest)
                        .await
                        .map_err(|e| anyhow!(e))
                },
                &context
            ).await.map_err(|e| ChainError::from_anyhow(e))?;
            
            // Nếu có base fee từ block
            if let Some(block) = block {
                if let Some(base_fee) = block.base_fee_per_gas {
                    // Tính priority fee từ cấu hình
                    let gwei_to_wei = U256::from(10).pow(U256::from(9));
                    let priority_fee = U256::from_f64_lossy(
                        self.config.gas_config.priority_fee
                    ) * gwei_to_wei;
                    
                    // Tính max fee = base_fee * 2 + priority_fee
                    let max_fee = base_fee.checked_mul(U256::from(2))
                        .and_then(|v| v.checked_add(priority_fee))
                        .unwrap_or(base_fee.checked_add(priority_fee).unwrap_or(base_fee));
                    
                    return Ok(GasInfo::new_eip1559(
                        max_fee,
                        priority_fee,
                        U256::from(self.config.gas_config.default_gas_limit),
                    ));
                }
            }
        }
        
        // Fallback cho legacy gas price
        let gas_price = self.get_gas_price().await?;
        Ok(GasInfo::new_legacy(
            gas_price,
            U256::from(self.config.gas_config.default_gas_limit),
        ))
    }
    
    async fn send_raw_transaction(&self, tx_bytes: Bytes) -> Result<PendingTransaction<'static, Provider<Http>>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "send_raw_transaction",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Retry nếu có lỗi
        let tx_hash = self.retry_policy.retry(
            Box::new(move || {
                let provider_clone = provider.provider.clone();
                let tx_bytes_clone = tx_bytes.clone();
                
                Box::pin(async move {
                    provider_clone.send_raw_transaction(tx_bytes_clone).await
                        .map_err(|e| anyhow::anyhow!(e))
                })
            }),
            &context
        ).await
        .map_err(|e| ChainError::from_anyhow(anyhow!("Failed to send raw transaction: {}", e)))?;
        
        Ok(PendingTransaction::new(tx_hash, provider.provider.clone()))
    }
    
    async fn send_transaction(&self, tx: &TransactionRequest) -> Result<PendingTransaction<'static, Provider<Http>>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "send_transaction",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Clone transaction request để sử dụng trong async closure
        let tx_clone = tx.clone();
        
        // Retry nếu có lỗi
        let tx_hash = self.retry_policy.retry(
            Box::new(move || {
                let provider_clone = provider.provider.clone();
                let tx_clone = tx_clone.clone();
                
                Box::pin(async move {
                    provider_clone.send_transaction(tx_clone, None).await
                        .map_err(|e| anyhow::anyhow!(e))
                })
            }),
            &context
        ).await
        .map_err(|e| ChainError::from_anyhow(anyhow!("Failed to send transaction: {}", e)))?;
        
        Ok(PendingTransaction::new(tx_hash, provider.provider.clone()))
    }
    
    async fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_transaction_receipt",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy receipt với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_transaction_receipt(tx_hash)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_transaction(&self, tx_hash: H256) -> Result<Option<Transaction>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_transaction",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy transaction với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_transaction(tx_hash)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_eth_balance(&self, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_eth_balance",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy số dư với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_balance(address, block)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_token_balance(&self, token: Address, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo contract ERC20
        let contract = self.create_contract(token, "erc20", provider.provider.clone())
            .await
            .map_err(|e| ChainError::ContractNotFound(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_token_balance",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy số dư token với retry
        self.retry_policy.retry(
            || async {
                let block_option = block;
                contract.method::<_, U256>("balanceOf", address)?
                    .block(block_option.unwrap_or(BlockId::Number(BlockNumber::Latest)))
                    .call()
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_token_details(&self, token: Address) -> Result<TokenDetails, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo contract ERC20
        let contract = self.create_contract(token, "erc20", provider.provider.clone())
            .await
            .map_err(|e| ChainError::ContractNotFound(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_token_details",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện các request song song
        let name_future = self.retry_policy.retry(
            || async {
                contract.method::<_, String>("name", ())?
                    .call()
                    .await
                    .map_err(|e: ethers::contract::ContractError<_>| anyhow!(e))
            },
            &context
        );
        
        let symbol_future = self.retry_policy.retry(
            || async {
                contract.method::<_, String>("symbol", ())?
                    .call()
                    .await
                    .map_err(|e: ethers::contract::ContractError<_>| anyhow!(e))
            },
            &context
        );
        
        let decimals_future = self.retry_policy.retry(
            || async {
                contract.method::<_, u8>("decimals", ())?
                    .call()
                    .await
                    .map_err(|e: ethers::contract::ContractError<_>| anyhow!(e))
            },
            &context
        );
        
        let total_supply_future = self.retry_policy.retry(
            || async {
                contract.method::<_, U256>("totalSupply", ())?
                    .call()
                    .await
                    .map_err(|e: ethers::contract::ContractError<_>| anyhow!(e))
            },
            &context
        );
        
        // Chờ tất cả các request hoàn thành
        let (name, symbol, decimals, total_supply): (Result<String>, Result<String>, Result<u8>, Result<U256>) = tokio::join!(
            name_future,
            symbol_future,
            decimals_future,
            total_supply_future
        );
        
        // Tạo token details
        let token_details = TokenDetails {
            address: token,
            name: name.map_err(|e| ChainError::from_anyhow(e))?,
            symbol: symbol.map_err(|e| ChainError::from_anyhow(e))?,
            decimals: decimals.map_err(|e| ChainError::from_anyhow(e))?,
            total_supply: total_supply.map_err(|e| ChainError::from_anyhow(e))?,
        };
        
        Ok(token_details)
    }
    
    async fn get_token_allowance(&self, token: Address, owner: Address, spender: Address) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo contract ERC20
        let contract = self.create_contract(token, "erc20", provider.provider.clone())
            .await
            .map_err(|e| ChainError::ContractNotFound(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_token_allowance",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy allowance với retry
        self.retry_policy.retry(
            || async {
                contract.method::<_, U256>("allowance", (owner, spender))?
                    .call()
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_logs",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy logs với retry
        let filter_clone = filter.clone();
        self.retry_policy.retry(
            || async {
                provider.provider.get_logs(&filter_clone)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn get_transaction_count(&self, address: Address, block: Option<BlockId>) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_transaction_count",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Thực hiện lấy nonce với retry
        self.retry_policy.retry(
            || async {
                provider.provider.get_transaction_count(address, block)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn estimate_gas(&self, tx: &TransactionRequest) -> Result<U256, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "estimate_gas",
            &provider.endpoint_info.url,
            self.chain_id,
            tx.gas_price,
        );
        
        // Thực hiện ước tính gas với retry
        let tx_clone = tx.clone();
        self.retry_policy.retry(
            || async {
                provider.provider.estimate_gas(&tx_clone)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn call(&self, tx: &TransactionRequest, block: Option<BlockId>) -> Result<Bytes, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "call",
            &provider.endpoint_info.url,
            self.chain_id,
            tx.gas_price,
        );
        
        // Thực hiện call với retry
        let tx_clone = tx.clone();
        let block_clone = block;
        self.retry_policy.retry(
            || async {
                provider.provider.call(&tx_clone, block_clone)
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))
    }
    
    async fn wait_for_transaction_receipt(
        &self,
        tx_hash: H256,
        confirmations: usize,
        timeout: std::time::Duration,
    ) -> Result<TransactionReceipt, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e: anyhow::Error| ChainError::from_anyhow(e))?;
        
        // Tạo future từ PendingTransaction
        let pending_tx = PendingTransaction::new(tx_hash, provider.provider.clone());
        
        // Sử dụng tokio timeout
        let receipt_result = tokio::time::timeout(
            timeout,
            pending_tx.confirmations(confirmations)
        ).await;
        
        match receipt_result {
            Ok(Ok(Some(receipt))) => {
                Ok(receipt)
            },
            Ok(Ok(None)) => {
                Err(ChainError::from_anyhow(anyhow!("Transaction not found or dropped: {}", tx_hash)))
            },
            Ok(Err(e)) => {
                Err(ChainError::from_anyhow(anyhow!("Transaction error: {}", e)))
            },
            Err(_) => {
                Err(ChainError::from_anyhow(anyhow!("Transaction timeout after {} ms", timeout.as_millis())))
            }
        }
    }
    
    async fn get_node_info(&self) -> Result<NodeInfo, ChainError> {
        let provider = self.get_provider().await
            .map_err(|e| ChainError::Connection(e.to_string()))?;
        
        // Tạo context cho retry
        let context = RetryContext::new(
            "get_node_info",
            &provider.endpoint_info.url,
            self.chain_id,
            None,
        );
        
        // Lấy thông tin client
        let client_version = self.retry_policy.retry(
            || async {
                provider.provider.client_version()
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))?;
        
        // Lấy thông tin syncing
        let syncing = self.retry_policy.retry(
            || async {
                provider.provider.syncing()
                    .await
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))?;
        
        // Lấy số block hiện tại
        let current_block = self.retry_policy.retry(
            || async {
                provider.provider.get_block_number()
                    .await
                    .map(|n| n.as_u64())
                    .map_err(|e| anyhow!(e))
            },
            &context
        ).await.map_err(|e| ChainError::from_anyhow(e))?;
        
        // Xác định highest block từ thông tin syncing
        let (is_syncing, highest_block) = match syncing {
            ethers::types::SyncingStatus::IsSyncing(info) => {
                (true, info.highest_block.as_u64())
            },
            _ => (false, current_block),
        };
        
        let node_info = NodeInfo {
            client_version,
            chain_id: self.chain_id,
            is_syncing,
            current_block,
            highest_block,
        };
        
        Ok(node_info)
    }
}

impl std::fmt::Debug for EVMChainAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EVMChainAdapter")
            .field("chain_id", &self.chain_id)
            .field("config", &self.config)
            .field("wallet_address", &self.wallet_address)
            .field("has_wallet", &self.wallet.is_some())
            .finish()
    }
} 
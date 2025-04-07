use crate::chain_adapters::base::{ChainAdapter, ChainConfig};
use async_trait::async_trait;
use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    signers::{LocalWallet, Signer},
    types::{Address, U256, TransactionRequest},
    contract::Contract,
};
use std::sync::Arc;
use std::str::FromStr;
use log::{info, error, debug};
use anyhow::Result;
use crate::gas_optimizer::{GasOptimizer, NetworkCongestion, get_current_gas_price};
use crate::error::TransactionError;
use crate::abi_utils;

pub struct BaseAdapter {
    config: ChainConfig,
    provider: Provider<Http>,
    wallet: Option<LocalWallet>,
    router_abi: ethers::abi::Abi,
    token_abi: ethers::abi::Abi,
}

impl BaseAdapter {
    pub async fn new() -> Result<Self> {
        let config = ChainConfig {
            name: "Base".to_string(),
            chain_id: 8453,
            rpc_url: "https://mainnet.base.org".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0x4200000000000000000000000000000000000006".to_string(), // WETH on Base
            router_address: "0xfCA736a42EE6f1BF35afDeFa3B262a4B0C4D3E6e".to_string(), // BaseSwap Router
            factory_address: "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".to_string(), // BaseSwap Factory
            explorer_url: "https://basescan.org".to_string(),
            block_time: 2000, // ~2 seconds
            default_gas_limit: 21000,
            default_gas_price: 1, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(2), // gwei
        };
        
        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        
        // Chuẩn bị ABIs
        let router_abi: ethers::abi::Abi = serde_json::from_str(abi_utils::get_router_abi())?;
        let token_abi: ethers::abi::Abi = serde_json::from_str(abi_utils::get_erc20_abi())?;
        
        Ok(Self {
            config,
            provider,
            wallet: None,
            router_abi,
            token_abi,
        })
    }
}

#[async_trait]
impl ChainAdapter for BaseAdapter {
    fn get_config(&self) -> &ChainConfig {
        &self.config
    }
    
    fn get_provider(&self) -> &Provider<Http> {
        &self.provider
    }
    
    fn get_wallet(&self) -> Option<&LocalWallet> {
        self.wallet.as_ref()
    }
    
    fn set_wallet(&mut self, wallet: LocalWallet) {
        self.wallet = Some(wallet.with_chain_id(self.config.chain_id));
    }
    
    async fn get_native_balance(&self, address: &str) -> Result<U256> {
        let address = Address::from_str(address)?;
        let balance = self.provider.get_balance(address, None).await?;
        Ok(balance)
    }
    
    async fn get_token_balance(&self, token_address: &str, wallet_address: &str) -> Result<U256> {
        let token_address = Address::from_str(token_address)?;
        let wallet_address = Address::from_str(wallet_address)?;
        
        let token_contract = Contract::new(
            token_address,
            self.token_abi.clone(),
            Arc::new(self.provider.clone())
        );
        
        let balance: U256 = token_contract
            .method("balanceOf", wallet_address)?
            .call()
            .await?;
            
        Ok(balance)
    }
    
    async fn approve_token(&self, token_address: &str, spender_address: &str, amount: U256) -> Result<Option<TransactionReceipt>> {
        let wallet = self.wallet.as_ref().ok_or_else(|| anyhow::anyhow!("Wallet chưa được thiết lập"))?;
        let client = Arc::new(SignerMiddleware::new(self.provider.clone(), wallet.clone()));
        
        let token_address = Address::from_str(token_address)?;
        let spender_address = Address::from_str(spender_address)?;
        
        let token_contract = Contract::new(
            token_address,
            self.token_abi.clone(),
            client
        );
        
        let tx = token_contract
            .method("approve", (spender_address, amount))?;
        
        // Base hỗ trợ EIP-1559
        let fee_estimator = client.estimate_eip1559_fees(None).await?;
        let tx = tx.gas(U256::from(100000))
            .max_fee_per_gas(fee_estimator.max_fee_per_gas)
            .max_priority_fee_per_gas(fee_estimator.max_priority_fee_per_gas)
            .send()
            .await?;
        
        Ok(tx.await?)
    }
    
    async fn swap_exact_eth_for_tokens(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>> {
        let wallet = self.wallet.as_ref().ok_or_else(|| anyhow::anyhow!("Wallet chưa được thiết lập"))?;
        let client = Arc::new(SignerMiddleware::new(self.provider.clone(), wallet.clone()));
        
        let router_address = Address::from_str(&self.config.router_address)?;
        let recipient_address = Address::from_str(recipient)?;
        
        let path = self.get_native_to_token_path(token_address);
        
        let router_contract = Contract::new(
            router_address,
            self.router_abi.clone(),
            client.clone()
        );
        
        let tx = router_contract
            .method(
                "swapExactETHForTokensSupportingFeeOnTransferTokens",
                (
                    min_amount_out,
                    path,
                    recipient_address,
                    U256::from(deadline)
                )
            )?
            .value(amount_in);
        
        // Thiết lập gas limit
        let tx = if let Some(gas_limit) = gas_limit {
            tx.gas(U256::from(gas_limit))
        } else {
            tx.gas(U256::from(self.config.default_gas_limit))
        };
        
        // Base hỗ trợ EIP-1559
        let tx = if gas_price.is_none() {
            let fee_estimator = client.estimate_eip1559_fees(None).await?;
            tx.max_fee_per_gas(fee_estimator.max_fee_per_gas)
                .max_priority_fee_per_gas(fee_estimator.max_priority_fee_per_gas)
                .send()
                .await?
        } else {
            let gas_price = gas_price.unwrap_or(self.config.default_gas_price);
            tx.gas_price(U256::from(gas_price))
                .send()
                .await?
        };
        
        Ok(tx.await?)
    }
    
    async fn swap_exact_tokens_for_eth(
        &self,
        token_address: &str,
        amount_in: U256,
        min_amount_out: U256,
        recipient: &str,
        deadline: u64,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
    ) -> Result<Option<TransactionReceipt>> {
        let wallet = self.wallet.as_ref().ok_or_else(|| anyhow::anyhow!("Wallet chưa được thiết lập"))?;
        let client = Arc::new(SignerMiddleware::new(self.provider.clone(), wallet.clone()));
        
        let router_address = Address::from_str(&self.config.router_address)?;
        let recipient_address = Address::from_str(recipient)?;
        
        let path = self.get_token_to_native_path(token_address);
        
        let router_contract = Contract::new(
            router_address,
            self.router_abi.clone(),
            client.clone()
        );
        
        let tx = router_contract
            .method(
                "swapExactTokensForETHSupportingFeeOnTransferTokens",
                (
                    amount_in,
                    min_amount_out,
                    path,
                    recipient_address,
                    U256::from(deadline)
                )
            )?;
        
        // Thiết lập gas limit
        let tx = if let Some(gas_limit) = gas_limit {
            tx.gas(U256::from(gas_limit))
        } else {
            tx.gas(U256::from(self.config.default_gas_limit))
        };
        
        // Base hỗ trợ EIP-1559
        let tx = if gas_price.is_none() {
            let fee_estimator = client.estimate_eip1559_fees(None).await?;
            tx.max_fee_per_gas(fee_estimator.max_fee_per_gas)
                .max_priority_fee_per_gas(fee_estimator.max_priority_fee_per_gas)
                .send()
                .await?
        } else {
            let gas_price = gas_price.unwrap_or(self.config.default_gas_price);
            tx.gas_price(U256::from(gas_price))
                .send()
                .await?
        };
        
        Ok(tx.await?)
    }
    
    async fn get_amounts_out(&self, amount_in: U256, path: Vec<Address>) -> Result<Vec<U256>> {
        let router_address = Address::from_str(&self.config.router_address)?;
        
        let router_contract = Contract::new(
            router_address,
            self.router_abi.clone(),
            Arc::new(self.provider.clone())
        );
        
        let amounts_out: Vec<U256> = router_contract
            .method("getAmountsOut", (amount_in, path))?
            .call()
            .await?;
            
        Ok(amounts_out)
    }
    
    async fn get_pair(&self, token_a: &str, token_b: &str) -> Result<Option<String>> {
        let factory_address = Address::from_str(&self.config.factory_address)?;
        let token_a = Address::from_str(token_a)?;
        let token_b = Address::from_str(token_b)?;
        
        // ABI cho factory
        let factory_abi: ethers::abi::Abi = serde_json::from_str(
            r#"[{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"},{"internalType":"address","name":"","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"}]"#
        )?;
        
        let factory_contract = Contract::new(
            factory_address,
            factory_abi,
            Arc::new(self.provider.clone())
        );
        
        let pair: Address = factory_contract
            .method("getPair", (token_a, token_b))?
            .call()
            .await?;
            
        if pair == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(format!("{:?}", pair)))
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Vec<Address> {
        let wrapped_native = Address::from_str(&self.config.wrapped_native_token).unwrap();
        let token = Address::from_str(token_address).unwrap();
        vec![wrapped_native, token]
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Vec<Address> {
        let wrapped_native = Address::from_str(&self.config.wrapped_native_token).unwrap();
        let token = Address::from_str(token_address).unwrap();
        vec![token, wrapped_native]
    }
    
    async fn create_flashbots_bundle(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        // Implement flashbots for Base (nếu có)
        Err(anyhow::anyhow!("Base chưa hỗ trợ Flashbots chính thức"))
    }
    
    async fn watch_pending_transactions(&self, callback: Box<dyn Fn(Transaction) + Send + Sync>) -> Result<()> {
        // Theo dõi mempool
        let ws_provider = Provider::<Ws>::connect("wss://base.llamarpc.com").await?;
        
        let stream = ws_provider.subscribe_pending_txs().await?;
        let mut stream = stream.transactions_unordered(256).fuse();
        
        tokio::spawn(async move {
            while let Some(tx_res) = stream.next().await {
                match tx_res {
                    Ok(tx) => {
                        callback(tx);
                    },
                    Err(e) => {
                        error!("Lỗi khi nhận giao dịch: {}", e);
                    }
                }
            }
        });
        
        Ok(())
    }
    
    fn get_gas_optimizer(&self) -> Option<&GasOptimizer> {
        None
    }
    
    fn set_gas_optimizer(&mut self, _optimizer: GasOptimizer) {
        // Mặc định không làm gì, các implementation sẽ override
    }
    
    async fn optimize_transaction_gas(&self, mut tx: TypedTransaction) -> Result<TypedTransaction> {
        if let Some(optimizer) = self.get_gas_optimizer() {
            if self.get_config().eip1559_supported {
                // Đối với EIP-1559
                let (priority_fee, max_fee) = optimizer.get_optimal_eip1559_fees(self).await?;
                
                if tx.as_eip1559_mut().is_none() {
                    // Convert to EIP1559 if not already
                    let mut eip1559_tx = TypedTransaction::Eip1559(Eip1559TransactionRequest::new());
                    
                    if let Some(to) = tx.to() {
                        eip1559_tx.set_to(*to);
                    }
                    if let Some(data) = tx.data() {
                        eip1559_tx.set_data(data.clone());
                    }
                    if let Some(value) = tx.value() {
                        eip1559_tx.set_value(*value);
                    }
                    if let Some(nonce) = tx.nonce() {
                        eip1559_tx.set_nonce(*nonce);
                    }
                    if let Some(gas) = tx.gas() {
                        eip1559_tx.set_gas(*gas);
                    }
                    
                    tx = eip1559_tx;
                }
                
                if let Some(eip1559) = tx.as_eip1559_mut() {
                    eip1559.max_priority_fee_per_gas = priority_fee;
                    eip1559.max_fee_per_gas = max_fee;
                }
                
            } else {
                // Đối với giao dịch legacy
                let gas_price = optimizer.get_optimal_gas_price(self).await?;
                tx.set_gas_price(gas_price);
            }
        }
        
        Ok(tx)
    }
    
    async fn send_transaction(&self, mut tx: TypedTransaction) -> Result<Option<TransactionReceipt>> {
        // Tối ưu gas nếu cần
        if self.get_gas_optimizer().is_some() {
            tx = self.optimize_transaction_gas(tx).await?;
        }
        
        let wallet = self.get_wallet().ok_or_else(|| anyhow::anyhow!("Wallet chưa được thiết lập"))?;
        let client = Arc::new(SignerMiddleware::new(self.get_provider().clone(), wallet.clone()));
        
        // Gửi giao dịch
        let pending_tx = client.send_transaction(tx, None).await?;
        
        // Đợi nhận receipt với timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(60), // 1 phút timeout
            pending_tx.await
        ).await {
            Ok(Ok(receipt)) => Ok(Some(receipt)),
            Ok(Err(e)) => Err(anyhow::anyhow!("Lỗi khi đợi receipt: {}", e)),
            Err(_) => Err(anyhow::anyhow!("Timeout khi đợi transaction receipt")),
        }
    }
    
    async fn send_transaction_with_retry(
        &self,
        tx: TypedTransaction,
        gas_limit: Option<u64>,
        gas_price: Option<u64>,
        operation_name: &str,
    ) -> Result<TransactionReceipt, TransactionError> {
        // Kiểm tra nếu có ví
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return Err(TransactionError::Other("Không có ví để thực hiện giao dịch".to_string())),
        };
        
        // Tạo client với ví
        let client = SignerMiddleware::new(
            self.provider.clone(),
            wallet.clone().with_chain_id(self.config.chain_id),
        );
        
        // Chuẩn bị tx với gas nếu có
        let mut tx_to_send = tx;
        
        if let Some(limit) = gas_limit {
            tx_to_send.set_gas(limit);
        }
        
        if let Some(price) = gas_price {
            tx_to_send.set_gas_price(U256::from(price));
        }
        
        // Gửi giao dịch
        let pending_tx = match client.send_transaction(tx_to_send, None).await {
            Ok(tx) => tx,
            Err(e) => return Err(TransactionError::Other(e.to_string())),
        };
        
        // Đợi biên lai
        match pending_tx.await {
            Ok(Some(receipt)) => Ok(receipt),
            Ok(None) => Err(TransactionError::Timeout),
            Err(e) => Err(TransactionError::Other(e.to_string())),
        }
    }
    
    async fn approve_token_with_retry(
        &self, 
        token_address: &str, 
        spender_address: &str, 
        amount: U256
    ) -> Result<TransactionReceipt, TransactionError> {
        let token_addr = match Address::from_str(token_address) {
            Ok(addr) => addr,
            Err(e) => return Err(TransactionError::Other(format!("Invalid token address: {}", e))),
        };
        
        let spender_addr = match Address::from_str(spender_address) {
            Ok(addr) => addr,
            Err(e) => return Err(TransactionError::Other(format!("Invalid spender address: {}", e))),
        };
        
        // Kiểm tra nếu có ví
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return Err(TransactionError::Other("Không có ví để thực hiện giao dịch".to_string())),
        };
        
        // Tạo client với ví
        let client = SignerMiddleware::new(
            self.provider.clone(),
            wallet.clone().with_chain_id(self.config.chain_id),
        );
        
        // Tạo contract ERC20 với quyền ghi
        let token_abi = abi_utils::get_erc20_abi();
        let token_abi: ethers::abi::Abi = match serde_json::from_str(token_abi) {
            Ok(abi) => abi,
            Err(e) => return Err(TransactionError::Other(format!("Invalid ERC20 ABI: {}", e))),
        };
        
        let token_contract = Contract::new(token_addr, token_abi, Arc::new(client.clone()));
        
        // Gọi hàm approve
        let tx_call = match token_contract.method("approve", (spender_addr, amount)) {
            Ok(call) => call,
            Err(e) => return Err(TransactionError::Other(format!("Error creating approve call: {}", e))),
        };
        
        // Gửi giao dịch
        let pending_tx = match tx_call.send().await {
            Ok(tx) => tx,
            Err(e) => return Err(TransactionError::Other(format!("Error sending approve transaction: {}", e))),
        };
        
        // Đợi biên lai
        match pending_tx.await {
            Ok(receipt) => Ok(receipt),
            Err(e) => Err(TransactionError::Other(format!("Error getting receipt: {}", e))),
        }
    }
}

use crate::chain_adapters::base::{ChainAdapter, ChainConfig};
use ethers::prelude::*;
use ethers::providers::{Http, Provider, Middleware};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, U256, TransactionRequest, Transaction, TransactionReceipt};
use ethers::core::types::BlockNumber;
use std::sync::Arc;
use std::str::FromStr;
use log::{info, warn, debug, error};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use crate::error::TransactionError;
use crate::abi_utils;
use serde_json;

// Adapter cho Ethereum mainnet
#[derive(Debug)]
pub struct EthereumAdapter {
    provider: Provider<Http>,
    wallet: Option<LocalWallet>,
    config: ChainConfig,
}

impl EthereumAdapter {
    // Hàm tạo mới adapter với thông tin cấu hình
    pub async fn new() -> Result<Self> {
        // Tạo cấu hình cho Ethereum mainnet
        let config = ChainConfig {
            name: "Ethereum".to_string(),
            chain_id: 1,
            rpc_url: "https://eth.llamarpc.com".to_string(),
            native_symbol: "ETH".to_string(),
            wrapped_native_token: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(),
            router_address: "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_string(),
            factory_address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f".to_string(),
            explorer_url: "https://etherscan.io".to_string(),
            block_time: 12000, // 12 giây
            default_gas_limit: 21000,
            default_gas_price: 20, // gwei
            eip1559_supported: true,
            max_priority_fee: Some(2), // gwei
        };
        
        // Tạo provider từ RPC URL
        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        
        Ok(Self {
            provider,
            wallet: None,
            config,
        })
    }
}

#[async_trait]
impl ChainAdapter for EthereumAdapter {
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
        self.wallet = Some(wallet);
    }
    
    async fn get_native_balance(&self, address: &str) -> Result<U256> {
        let addr = Address::from_str(address)?;
        let balance = self.provider.get_balance(addr, None).await?;
        Ok(balance)
    }
    
    async fn get_token_balance(&self, token_address: &str, wallet_address: &str) -> Result<U256> {
        let token_addr = Address::from_str(token_address)?;
        let wallet_addr = Address::from_str(wallet_address)?;
        
        // Tạo contract ERC20
        let token_abi = abi_utils::get_erc20_abi();
        let token_abi: ethers::abi::Abi = serde_json::from_str(token_abi)?;
        let token_contract = Contract::new(token_addr, token_abi, self.provider.clone());
        
        // Gọi hàm balanceOf
        let balance: U256 = token_contract.method("balanceOf", wallet_addr)?.call().await?;
        Ok(balance)
    }
    
    async fn approve_token(&self, token_address: &str, spender_address: &str, amount: U256) -> Result<Option<TransactionReceipt>> {
        let token_addr = Address::from_str(token_address)?;
        let spender_addr = Address::from_str(spender_address)?;
        
        // Kiểm tra nếu có ví
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return Err(anyhow!("Không có ví để thực hiện giao dịch")),
        };
        
        // Tạo client với ví
        let client = SignerMiddleware::new(
            self.provider.clone(),
            wallet.clone().with_chain_id(self.config.chain_id),
        );
        
        // Tạo contract ERC20 với quyền ghi
        let token_abi = abi_utils::get_erc20_abi();
        let token_abi: ethers::abi::Abi = serde_json::from_str(token_abi)?;
        let token_contract = Contract::new(token_addr, token_abi, Arc::new(client));
        
        // Gọi hàm approve
        let tx = token_contract.method("approve", (spender_addr, amount))?
            .send()
            .await?;
            
        // Đợi biên lai
        let receipt = tx.await?;
        Ok(Some(receipt))
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
        let token_addr = Address::from_str(token_address)?;
        let recipient_addr = Address::from_str(recipient)?;
        
        // Kiểm tra nếu có ví
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return Err(anyhow!("Không có ví để thực hiện giao dịch")),
        };
        
        // Tạo client với ví
        let client = SignerMiddleware::new(
            self.provider.clone(),
            wallet.clone().with_chain_id(self.config.chain_id),
        );
        
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)?;
        let router_abi = abi_utils::get_router_abi();
        let router_abi: ethers::abi::Abi = serde_json::from_str(router_abi)?;
        let router_contract = Contract::new(router_addr, router_abi, Arc::new(client));
        
        // Tạo path cho swap
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)?;
        let path = vec![weth_addr, token_addr];
        
        // Chuẩn bị tx
        let mut tx = router_contract.method(
            "swapExactETHForTokens", 
            (min_amount_out, path, recipient_addr, U256::from(deadline))
        )?;
        
        // Thêm value và gas
        tx.tx.set_value(amount_in);
        
        if let Some(limit) = gas_limit {
            tx.tx.set_gas(limit);
        }
        
        if let Some(price) = gas_price {
            tx.tx.set_gas_price(U256::from(price));
        }
        
        // Gửi giao dịch
        let pending_tx = tx.send().await?;
        
        // Đợi biên lai
        let receipt = pending_tx.await?;
        Ok(Some(receipt))
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
        let token_addr = Address::from_str(token_address)?;
        let recipient_addr = Address::from_str(recipient)?;
        
        // Kiểm tra nếu có ví
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return Err(anyhow!("Không có ví để thực hiện giao dịch")),
        };
        
        // Tạo client với ví
        let client = SignerMiddleware::new(
            self.provider.clone(),
            wallet.clone().with_chain_id(self.config.chain_id),
        );
        
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)?;
        let router_abi = abi_utils::get_router_abi();
        let router_abi: ethers::abi::Abi = serde_json::from_str(router_abi)?;
        let router_contract = Contract::new(router_addr, router_abi, Arc::new(client));
        
        // Tạo path cho swap
        let weth_addr = Address::from_str(&self.config.wrapped_native_token)?;
        let path = vec![token_addr, weth_addr];
        
        // Chuẩn bị tx
        let mut tx = router_contract.method(
            "swapExactTokensForETH", 
            (amount_in, min_amount_out, path, recipient_addr, U256::from(deadline))
        )?;
        
        // Thêm gas
        if let Some(limit) = gas_limit {
            tx.tx.set_gas(limit);
        }
        
        if let Some(price) = gas_price {
            tx.tx.set_gas_price(U256::from(price));
        }
        
        // Gửi giao dịch
        let pending_tx = tx.send().await?;
        
        // Đợi biên lai
        let receipt = pending_tx.await?;
        Ok(Some(receipt))
    }
    
    async fn get_amounts_out(&self, amount_in: U256, path: Vec<Address>) -> Result<Vec<U256>> {
        // Tạo contract Router
        let router_addr = Address::from_str(&self.config.router_address)?;
        let router_abi = abi_utils::get_router_abi();
        let router_abi: ethers::abi::Abi = serde_json::from_str(router_abi)?;
        let router_contract = Contract::new(router_addr, router_abi, self.provider.clone());
        
        // Gọi hàm getAmountsOut
        let amounts: Vec<U256> = router_contract.method("getAmountsOut", (amount_in, path))?.call().await?;
        Ok(amounts)
    }
    
    async fn get_pair(&self, token_a: &str, token_b: &str) -> Result<Option<String>> {
        let token_a_addr = Address::from_str(token_a)?;
        let token_b_addr = Address::from_str(token_b)?;
        
        // Tạo contract Factory
        let factory_addr = Address::from_str(&self.config.factory_address)?;
        let factory_abi = abi_utils::get_factory_abi();
        let factory_abi: ethers::abi::Abi = serde_json::from_str(factory_abi)?;
        let factory_contract = Contract::new(factory_addr, factory_abi, self.provider.clone());
        
        // Gọi hàm getPair
        let pair_addr: Address = factory_contract.method("getPair", (token_a_addr, token_b_addr))?.call().await?;
        
        // Kiểm tra nếu địa chỉ là zero address
        if pair_addr == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(format!("{:?}", pair_addr)))
        }
    }
    
    fn get_native_to_token_path(&self, token_address: &str) -> Vec<Address> {
        let token_addr = match Address::from_str(token_address) {
            Ok(addr) => addr,
            Err(_) => return Vec::new(),
        };
        
        let weth_addr = match Address::from_str(&self.config.wrapped_native_token) {
            Ok(addr) => addr,
            Err(_) => return Vec::new(),
        };
        
        vec![weth_addr, token_addr]
    }
    
    fn get_token_to_native_path(&self, token_address: &str) -> Vec<Address> {
        let token_addr = match Address::from_str(token_address) {
            Ok(addr) => addr,
            Err(_) => return Vec::new(),
        };
        
        let weth_addr = match Address::from_str(&self.config.wrapped_native_token) {
            Ok(addr) => addr,
            Err(_) => return Vec::new(),
        };
        
        vec![token_addr, weth_addr]
    }
    
    // MEV-related methods - triển khai đơn giản
    async fn create_flashbots_bundle(&self, txs: Vec<TransactionRequest>) -> Result<()> {
        // Đây chỉ là triển khai mẫu, MEV cần được triển khai riêng
        info!("FlashBots bundle sẽ được triển khai sau, hiện tại chỉ ghi log");
        Ok(())
    }
    
    // Giám sát mempool - triển khai đơn giản
    async fn watch_pending_transactions(&self, callback: Box<dyn Fn(Transaction) + Send + Sync>) -> Result<()> {
        info!("Mempool watcher sẽ được triển khai sau");
        Ok(())
    }
    
    // Thêm các phương thức retry
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
    
    // Thêm phương thức approve với retry
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

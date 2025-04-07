use ethers::{
    prelude::*,
    providers::{Http, Provider, Middleware},
    types::{Address, U256, Transaction, Filter, H256, BlockId, BlockNumber, Bytes},
    abi::{self, Token},
};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use log::{info, warn, error, debug};
use tokio::time::sleep;
use std::collections::HashMap;
use crate::chain_adapters::base::ChainConfig;
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainParams {
    pub chain_id: u64,
    pub rpc_url: String,
    pub explorer_url: String,
}

pub struct BlockchainMonitor {
    provider: Provider<Http>,
    config: BlockchainParams,
    last_block: u64,
    caching: bool,
    cache_duration: Duration,
    cache: HashMap<String, (Instant, serde_json::Value)>,
}

impl BlockchainMonitor {
    pub fn new(params: BlockchainParams) -> Result<Self> {
        let provider = Provider::<Http>::try_from(&params.rpc_url)?;
        Ok(Self {
            provider,
            config: params,
            last_block: 0,
            caching: true,
            cache_duration: Duration::from_secs(30),
            cache: HashMap::new(),
        })
    }
    
    pub fn enable_cache(&mut self, enabled: bool, duration_secs: u64) {
        self.caching = enabled;
        self.cache_duration = Duration::from_secs(duration_secs);
    }
    
    pub async fn get_latest_block(&mut self) -> Result<u64> {
        let block_number = self.provider.get_block_number().await?;
        self.last_block = block_number.as_u64();
        Ok(self.last_block)
    }
    
    pub async fn get_token_info<'a>(&'a self, token_address: &str) -> Result<TokenInfo> {
        // Check cache first if enabled
        let cache_key = format!("token_info_{}", token_address);
        if self.caching {
            if let Some((time, cached_value)) = self.cache.get(&cache_key) {
                if time.elapsed() < self.cache_duration {
                    if let Ok(token_info) = serde_json::from_value(cached_value.clone()) {
                        return Ok(token_info);
                    }
                }
            }
        }
        
        // Validate token address format
        let address = match Address::from_str(token_address) {
            Ok(addr) => addr,
            Err(e) => return Err(anyhow!("Địa chỉ token không hợp lệ: {}: {}", token_address, e)),
        };
        
        // Load ABI từ file và kiểm tra lỗi
        let abi_str = include_str!("../abi/erc20.json");
        let abi: abi::Abi = match serde_json::from_str(abi_str) {
            Ok(parsed_abi) => parsed_abi,
            Err(e) => return Err(anyhow!("Không thể parse ERC20 ABI: {}", e)),
        };
        
        // Tạo đối tượng contract
        let token_contract = Contract::new(address, abi, Arc::new(self.provider.clone()));
        
        // Khai báo biến mặc định trong trường hợp gọi contract thất bại
        let mut name = "Unknown".to_string();
        let mut symbol = "UNK".to_string();
        let mut decimals: u8 = 18;
        let mut total_supply = U256::zero();
        
        // Gọi phương thức name() với xử lý lỗi
        match token_contract.method::<_, String>("name", ())
                .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm name(): {}", e))?
                .call().await {
            Ok(fetched_name) => name = fetched_name,
            Err(e) => {
                warn!("Không thể lấy tên của token {}: {}", token_address, e);
                // Tiếp tục thực hiện các lệnh khác dù có lỗi
            }
        }
        
        // Gọi phương thức symbol() với xử lý lỗi
        match token_contract.method::<_, String>("symbol", ())
                .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm symbol(): {}", e))?
                .call().await {
            Ok(fetched_symbol) => symbol = fetched_symbol,
            Err(e) => {
                warn!("Không thể lấy ký hiệu của token {}: {}", token_address, e);
                // Tiếp tục thực hiện các lệnh khác dù có lỗi
            }
        }
        
        // Gọi phương thức decimals() với xử lý lỗi
        match token_contract.method::<_, u8>("decimals", ())
                .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm decimals(): {}", e))?
                .call().await {
            Ok(fetched_decimals) => decimals = fetched_decimals,
            Err(e) => {
                warn!("Không thể lấy decimals của token {}: {}", token_address, e);
                // Sử dụng giá trị mặc định là 18
            }
        }
        
        // Gọi phương thức totalSupply() với xử lý lỗi
        match token_contract.method::<_, U256>("totalSupply", ())
                .map_err(|e| anyhow!("Lỗi khi chuẩn bị gọi hàm totalSupply(): {}", e))?
                .call().await {
            Ok(fetched_supply) => total_supply = fetched_supply,
            Err(e) => {
                warn!("Không thể lấy tổng cung của token {}: {}", token_address, e);
                // Sử dụng giá trị mặc định là 0
            }
        }
        
        let token_info = TokenInfo {
            address: token_address.to_string(),
            name,
            symbol,
            decimals,
            total_supply,
        };
        
        // Update cache if enabled
        if self.caching {
            if let Ok(json_value) = serde_json::to_value(&token_info) {
                self.cache.insert(cache_key, (Instant::now(), json_value));
            }
        }
        
        Ok(token_info)
    }
    
    pub async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>> {
        let tx_hash = H256::from_str(tx_hash)?;
        let receipt = self.provider.get_transaction_receipt(tx_hash).await?;
        Ok(receipt)
    }
    
    pub async fn wait_for_transaction<'a>(&'a self, tx_hash: &str, timeout_secs: u64) -> Result<Option<TransactionReceipt>> {
        // Validate tx_hash input
        let tx_hash = match H256::from_str(tx_hash) {
            Ok(hash) => hash,
            Err(e) => return Err(anyhow!("Định dạng transaction hash không hợp lệ: {}: {}", tx_hash, e)),
        };
        
        // Kiểm tra giá trị timeout_secs hợp lý
        let timeout_duration = if timeout_secs == 0 || timeout_secs > 3600 {
            warn!("Giá trị timeout không hợp lệ ({}), sử dụng giá trị mặc định 60 giây", timeout_secs);
            Duration::from_secs(60) // Giá trị mặc định nếu timeout_secs không hợp lệ
        } else {
            Duration::from_secs(timeout_secs)
        };
        
        let pending_tx = PendingTransaction::new(tx_hash, &self.provider);
        
        // Sử dụng match để xử lý rõ ràng các trường hợp lỗi
        match tokio::time::timeout(
            timeout_duration,
            pending_tx.confirmations(1)
        ).await {
            Ok(result) => match result {
                Ok(receipt) => {
                    // Kiểm tra xem giao dịch có thành công không (status = 1)
                    if let Some(receipt_with_status) = receipt.as_ref() {
                        if let Some(status) = receipt_with_status.status {
                            if status.as_u64() == 0 {
                                warn!("Giao dịch {} đã được xác nhận nhưng thất bại (status=0)", tx_hash);
                            } else {
                                info!("Giao dịch {} đã được xác nhận thành công", tx_hash);
                            }
                        }
                    }
                    Ok(receipt)
                },
                Err(e) => {
                    let error_msg = format!("Lỗi khi đợi giao dịch: {}", e);
                    error!("{}", error_msg);
                    Err(anyhow!(error_msg))
                }
            },
            Err(_) => {
                let error_msg = format!("Giao dịch không được xác nhận sau {} giây", timeout_secs);
                warn!("{}", error_msg);
                Err(anyhow!("Transaction confirmation timed out after {} seconds", timeout_secs))
            }
        }
    }
    
    pub async fn get_gas_price(&self) -> Result<U256> {
        Ok(self.provider.get_gas_price().await?)
    }
    
    pub async fn get_eth_balance(&self, address: &str) -> Result<U256> {
        let address = Address::from_str(address)?;
        Ok(self.provider.get_balance(address, None).await?)
    }
    
    // Phương thức với lifetime annotation rõ ràng
    pub async fn call_contract_function<'a, T: Detokenize + 'static>(
        &'a self,
        contract_address: &'a str,
        function_name: &'a str,
        function_params: Vec<Token>,
        abi_path: &'a str
    ) -> Result<T> {
        let contract_address = Address::from_str(contract_address)?;
        let abi_str = include_str!("../abi/erc20.json");
        let abi: abi::Abi = serde_json::from_str(abi_str)?;
        
        let contract = Contract::new(
            contract_address,
            abi,
            Arc::new(self.provider.clone())
        );
        
        let result = contract
            .method::<_, T>(function_name, function_params)?
            .call()
            .await?;
        
        Ok(result)
    }
    
    // Phương thức mới, không yêu cầu lifetime params
    pub async fn call_contract_function_with_abi<T: Detokenize + 'static>(
        &self,
        contract_address: &str,
        function_name: &str,
        function_params: Vec<Token>,
        abi: abi::Abi
    ) -> Result<T> {
        // Xác thực địa chỉ contract
        let contract_address = match Address::from_str(contract_address) {
            Ok(addr) => addr,
            Err(e) => {
                return Err(anyhow!("Địa chỉ contract không hợp lệ: {}: {}", contract_address, e));
            }
        };
        
        // Kiểm tra tên hàm có trong ABI không
        if !abi.functions().any(|f| f.name == function_name) {
            return Err(anyhow!("Hàm '{}' không có trong ABI của contract", function_name));
        }
        
        // Xác thực tham số đầu vào
        if function_params.iter().any(|p| matches!(p, Token::FixedBytes(b) if b.len() > 32)) {
            return Err(anyhow!("Tham số FixedBytes có kích thước vượt quá 32 bytes"));
        }
        
        // Kiểm tra các giá trị địa chỉ
        for param in &function_params {
            if let Token::Address(addr) = param {
                // Kiểm tra địa chỉ hợp lệ
                if addr == &Address::zero() {
                    warn!("Phát hiện địa chỉ 0x0 trong tham số, có thể là lỗi");
                }
            }
        }
        
        // Tạo contract và gọi hàm
        let contract = Contract::new(
            contract_address,
            abi,
            Arc::new(self.provider.clone())
        );
        
        // Thiết lập timeout cho cuộc gọi contract để tránh treo vô hạn
        let method_call = match contract.method::<_, T>(function_name, function_params) {
            Ok(call) => call,
            Err(e) => return Err(anyhow!("Lỗi khi chuẩn bị gọi hàm {}: {}", function_name, e)),
        };
        
        // Gọi với timeout
        match tokio::time::timeout(
            Duration::from_secs(30), // 30 giây timeout
            method_call.call()
        ).await {
            Ok(result) => match result {
                Ok(data) => Ok(data),
                Err(e) => {
                    let msg = format!("Lỗi khi gọi hàm {} trên contract {}: {}", 
                        function_name, contract_address, e);
                    error!("{}", msg);
                    Err(anyhow!(msg))
                }
            },
            Err(_) => {
                let msg = format!("Timeout khi gọi hàm {} trên contract {}", 
                    function_name, contract_address);
                warn!("{}", msg);
                Err(anyhow!(msg))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: U256,
}

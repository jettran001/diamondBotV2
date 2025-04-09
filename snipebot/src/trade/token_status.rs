use ethers::{
    providers::{Middleware},
    types::{Address, U256, H256, TransactionReceipt},
    contract::Contract,
    providers::Provider
};
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::str::FromStr;
use std::collections::HashMap;
use log::{info, warn, debug, error};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{sync::RwLock};
use async_trait::async_trait;
use serde_json::Value;
use std::{fmt::Debug};
use metrics::{counter, gauge};

// Import crate modules
use crate::{
    chain_adapters::{
        ChainAdapter, 
        ChainAdapterEnum, 
        ChainError, 
        GasInfo
    },
    risk_analyzer::TokenRiskAnalysis,
};

use common::cache::{Cache, CacheEntry};

/// Cấu trúc thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub address: Address,
    pub symbol: String,
    pub decimals: u8,
    pub router: String,
    pub pair: Option<String>,
}

/// Cấu trúc lưu trữ số dư token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    pub address: Address,
    pub symbol: String,
    pub balance: U256,
}

/// Token Position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPosition {
    pub token_address: String,
    pub amount: U256,
    pub cost_basis: f64,
    pub timestamp: u64,
    pub current_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TokenSafetyLevel {
    Green,  // 🟢 Tốt
    Yellow, // 🟡 Trung bình
    Red,    // 🔴 Nguy hiểm
}

impl TokenSafetyLevel {
    pub fn to_emoji(&self) -> &'static str {
        match self {
            TokenSafetyLevel::Green => "🟢",
            TokenSafetyLevel::Yellow => "🟡",
            TokenSafetyLevel::Red => "🔴",
        }
    }
    
    pub fn to_description(&self) -> &'static str {
        match self {
            TokenSafetyLevel::Green => "Token an toàn",
            TokenSafetyLevel::Yellow => "Token có rủi ro trung bình",
            TokenSafetyLevel::Red => "Token có nhiều dấu hiệu nguy hiểm",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatus {
    pub address: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub price_usd: f64,
    pub price_native: f64,
    pub market_cap: f64,
    pub total_supply: String,
    pub circulating_supply: String,
    pub holders_count: u64,
    pub liquidity: f64,
    pub volume_24h: f64,
    pub price_change_24h: f64,
    pub pair_address: Option<String>,
    pub router_address: String,
    pub holders: Vec<HolderInfo>,
    pub last_updated: u64,
    pub safety_level: TokenSafetyLevel,        // Mức độ an toàn
    pub audit_score: Option<u8>,               // Điểm audit (0-100)
    pub liquidity_locked: Option<bool>,        // Thanh khoản đã khóa
    pub is_contract_verified: bool,            // Contract đã xác minh
    pub has_dangerous_functions: bool,         // Có các hàm nguy hiểm
    pub dangerous_functions: Vec<String>,      // Danh sách các hàm nguy hiểm
    pub pending_tx_count: u32,                 // Số lượng giao dịch đang chờ xử lý
    pub tax_info: Option<TaxInfo>,             // Thông tin về tax
}

/// Thông tin về thuế
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxInfo {
    /// Thuế mua (%)
    pub buy_tax: f64,
    /// Thuế bán (%)
    pub sell_tax: f64,
    /// Thuế chuyển (%)
    pub transfer_tax: f64,
    /// Thời gian giữ tối thiểu (phút)
    pub min_hold_time: Option<u64>
}

/// Thông tin về holder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderInfo {
    /// Địa chỉ holder
    pub address: String,
    /// Có phải là contract không
    pub is_contract: bool,
    /// Số dư token
    pub balance: String,
    /// Phần trăm sở hữu
    pub percent: f64,
    /// Số lượng holder
    pub holder_count: Option<u32>,
    /// Phân bố token
    pub token_distribution: Option<Vec<(String, f64)>>,
    /// Top holder
    pub top_holders: Option<Vec<(String, f64)>>,
    /// Độ tập trung token
    pub concentration_score: Option<f64>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPriceAlert {
    pub token_address: String,
    pub symbol: String,
    pub price_change_percent: f64,
    pub old_price: f64,
    pub new_price: f64,
    pub timestamp: u64,
    pub alert_type: PriceAlertType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriceAlertType {
    PriceIncrease,
    PriceDecrease,
    HighVolume,
    LiquidityRemoved,
    LiquidityAdded,
}

/// Thông tin về liquidity
#[derive(Debug, Clone)]
pub struct LiquidityInfo {
    /// Tổng liquidity (USD)
    pub total_liquidity: f64,
    /// Số lượng liquidity pool
    pub pool_count: u32,
    /// Phân bố liquidity
    pub liquidity_distribution: Vec<(String, f64)>,
    /// Độ sâu thị trường
    pub market_depth: f64
}

// Base trait for sync operations
pub trait TokenStatusTrackerBase: Send + Sync + 'static {
    fn get_tracked_tokens(&self) -> Vec<String>;
    fn is_tracking(&self, token_address: &str) -> bool;
    fn get_cached_status(&self, token_address: &str) -> Option<TokenStatus>;
}

// Async operations trait
#[async_trait]
pub trait AsyncTokenStatusTracker: TokenStatusTrackerBase {
    async fn add_token_to_track(&mut self, token_address: &str) -> Result<(), Box<dyn std::error::Error>>;
    async fn get_token_status(&self, token_address: &str) -> Result<Option<TokenStatus>, Box<dyn std::error::Error>>;
    async fn update_all_tokens(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}

/// Token Status Tracker
pub struct TokenStatusTracker {
    adapter: ChainAdapterEnum,
    tracked_tokens: Arc<RwLock<HashMap<String, CacheEntry<TokenStatus>>>>,
    factory_addresses: Vec<Address>,
    router_addresses: Vec<Address>,
    token_abi: ethers::abi::Abi,
    pair_abi: ethers::abi::Abi,
    weth_address: Address,
    min_alert_percent: f64,
    alert_callbacks: Vec<Box<dyn Fn(TokenPriceAlert) + Send + Sync>>,
    max_tokens: usize,
}

#[async_trait]
impl Cache for TokenStatusTracker {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        let tokens = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?;
        if let Some(entry) = tokens.get(key) {
            if !entry.is_expired() {
                if let Ok(value) = serde_json::to_string(&entry.value) {
                    return Ok(Some(serde_json::from_str(&value)?));
                }
            }
        }
        Ok(None)
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        let json_value = serde_json::to_value(value)?;
        if let Ok(token_status) = serde_json::from_value::<TokenStatus>(json_value) {
            let mut tokens = self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?;
            tokens.insert(key.to_string(), CacheEntry::new(token_status, ttl_seconds));
        }
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut tokens = self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?;
        tokens.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut tokens = self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?;
        tokens.clear();
        Ok(())
    }

    async fn cleanup_cache(&self) -> Result<()> {
        let mut tokens = self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?;
        let keys_to_remove: Vec<String> = tokens
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in keys_to_remove {
            tokens.remove(&key);
        }
        Ok(())
    }
}

impl TokenStatusTracker {
    pub fn new(
        adapter: ChainAdapterEnum,
        factory_addresses: Vec<String>,
        router_addresses: Vec<String>,
        weth_address: String,
    ) -> Result<Self> {
        // Convert string addresses to Address type
        let factories = factory_addresses.iter()
            .map(|addr| Address::from_str(addr))
            .collect::<Result<Vec<_>, _>>()?;
            
        let routers = router_addresses.iter()
            .map(|addr| Address::from_str(addr))
            .collect::<Result<Vec<_>, _>>()?;
            
        let weth = Address::from_str(&weth_address)?;
        
        // Load ABIs
        let token_abi = serde_json::from_str(abi_utils::get_erc20_abi())?;
        let pair_abi = serde_json::from_str(abi_utils::get_pair_abi())?;
        
        // Tạo LruCache với kích thước hợp lý (1000 tokens)
        let max_tokens = 1000;
        let cache = Arc::new(RwLock::new(HashMap::new()));
        
        Ok(Self {
            adapter,
            tracked_tokens: cache,
            factory_addresses: factories,
            router_addresses: routers,
            token_abi,
            pair_abi,
            weth_address: weth,
            min_alert_percent: 5.0, // Mặc định 5%
            alert_callbacks: Vec::new(),
            max_tokens,
        })
    }
    
    // Thêm token để theo dõi
    pub async fn add_token(&mut self, token_address: &str) -> Result<()> {
        // Khi sử dụng LruCache, không cần kiểm tra contains_key() vì LruCache tự động quản lý
        let status = self.get_token_status(token_address).await?;
        self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?.insert(token_address.to_string(), CacheEntry::new(status, 300));
        
        Ok(())
    }
    
    // Xóa token khỏi danh sách theo dõi
    pub fn remove_token(&mut self, token_address: &str) -> bool {
        self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e)).unwrap().remove(token_address).is_some()
    }
    
    // Lấy thông tin token
    pub async fn get_token(&self, token_address: &str) -> Result<Option<TokenStatus>, Box<dyn std::error::Error>> {
        // Khi sử dụng LruCache, cần clone token_address trước khi gọi get
        let token_key = token_address.to_string();
        if let Some(status) = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.get(&token_key) {
            return Ok(Some(status.value.clone()));
        }
        
        Ok(None)
    }
    
    // Cập nhật trạng thái tất cả tokens
    pub async fn update_all_tokens(&mut self) -> Result<Vec<TokenPriceAlert>> {
        let mut alerts = Vec::new();
        
        let tokens_to_update: Vec<String> = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.keys().cloned().collect();
        
        // Sử dụng tokio::task::JoinSet để xử lý song song
        let mut join_set = tokio::task::JoinSet::new();
        
        // Giới hạn số lượng task song song để tránh quá tải
        const MAX_CONCURRENT_UPDATES: usize = 5;
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPDATES));
        
        for token_address in tokens_to_update {
            let adapter = self.adapter.clone();
            let semaphore_clone = semaphore.clone();
            let old_status = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.get(&token_address).cloned();
            
            // Chỉ xử lý nếu token đã được theo dõi
            if let Some(old_status) = old_status {
                join_set.spawn(async move {
                    // Lấy permit từ semaphore để giới hạn số lượng cập nhật đồng thời
                    let _permit = match semaphore_clone.acquire().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            warn!("Không thể lấy semaphore permit: {}", e);
                            return (token_address, old_status, Err(format!("Lỗi semaphore: {}", e).into()));
                        }
                    };
                    
                    // Thêm timeout cho việc lấy thông tin token
                    match tokio::time::timeout(
                        Duration::from_secs(10),
                        Self::get_token_status_with_adapter(&adapter, &token_address)
                    ).await {
                        Ok(result) => {
                            // Thêm phần ghi log phù hợp
                            match &result {
                                Ok(_) => debug!("Cập nhật thành công token: {}", token_address),
                                Err(e) => debug!("Lỗi khi cập nhật token {}: {}", token_address, e),
                            }
                            
                            (token_address, old_status, result)
                        },
                        Err(_) => {
                            warn!("Timeout khi cập nhật token {}", token_address);
                            (token_address, old_status, Err("Timeout khi cập nhật token".into()))
                        }
                    }
                });
            }
        }
        
        // Sử dụng timeout để tránh đợi quá lâu
        let timeout_duration = Duration::from_secs(30);
        let start_time = Instant::now();
        
        // Thu thập kết quả từ các task
        while let Some(result) = tokio::time::timeout(
            timeout_duration.saturating_sub(start_time.elapsed()),
            join_set.join_next()
        ).await.unwrap_or(None) {
            match result {
                Ok((token_address, old_status, Ok(new_status))) => {
                    // Cập nhật trạng thái token
                    self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?.insert(token_address, CacheEntry::new(new_status, 300));
                    
                    // Kiểm tra biến động giá và tạo cảnh báo nếu cần
                    if let Some(alert) = self.check_price_alert(&token_address, &old_status, &new_status) {
                        alerts.push(alert.clone());
                        
                        // Gọi các callback
                        for callback in &self.alert_callbacks {
                            callback(alert.clone());
                        }
                    }
                },
                Ok((token_address, _, Err(error))) => {
                    debug!("Lỗi khi cập nhật token {}: {:?}", token_address, error);
                },
                Err(e) => {
                    warn!("Lỗi khi join task: {}", e);
                }
            }
        }
        
        // Hủy các task còn lại nếu quá hạn
        join_set.abort_all();
        
        // Dọn dẹp tokens cũ
        let current_time = utils::safe_now();
        let removed_count = self.cleanup_old_tokens(current_time, 24 * 3600); // 24 giờ
        if removed_count > 0 {
            debug!("Đã xóa {} token cũ khỏi cache", removed_count);
        }
        
        // Giảm kích thước cache nếu quá lớn
        let max_cache_size = 1000;
        if self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.len() > max_cache_size {
            let reduced = self.reduce_cache_size(max_cache_size);
            debug!("Đã giảm {} token từ cache", reduced);
        }
        
        Ok(alerts)
    }
    
    // Phương thức mới để lấy token status với adapter
    async fn get_token_status_with_adapter(adapter: &ChainAdapterEnum, token_address: &str) -> Result<TokenStatus> {
        let token_addr = Address::from_str(token_address)?;
        
        // Tạo token contract sử dụng provider từ adapter
        let provider = adapter.get_provider();
        let token_abi = abi_utils::get_erc20_abi();
        let token_abi: ethers::abi::Abi = serde_json::from_str(token_abi)?;
        
        let token_contract = Contract::new(
            token_addr,
            token_abi.clone(),
            provider.clone(),
        );
        
        // Tiếp tục lấy thông tin token như cũ...
        
        // Ví dụ tạm thời để biên dịch thành công
        Ok(TokenStatus {
            address: token_address.to_string(),
            symbol: "TEMP".to_string(),
            name: "Temporary Token".to_string(),
            decimals: 18,
            price_usd: 0.0,
            price_native: 0.0,
            market_cap: 0.0,
            total_supply: "0".to_string(),
            circulating_supply: "0".to_string(),
            holders_count: 0,
            liquidity: 0.0,
            volume_24h: 0.0,
            price_change_24h: 0.0,
            pair_address: None,
            router_address: "".to_string(),
            holders: vec![],
            last_updated: utils::safe_now(),
            safety_level: TokenSafetyLevel::Yellow,
            audit_score: None,
            liquidity_locked: None,
            is_contract_verified: false,
            has_dangerous_functions: false,
            dangerous_functions: vec![],
            pending_tx_count: 0,
            tax_info: None,
        })
    }
    
    // Cập nhật phương thức get_token_status để sử dụng adapter
    pub async fn get_token_status(&self, token_address: &str) -> Result<TokenStatus> {
        Self::get_token_status_with_adapter(&self.adapter, token_address).await
    }
    
    // Tìm pair address và router
    async fn find_pair_and_router(&self, token_address: &str) -> Result<(Option<String>, Option<String>)> {
        let token_addr = Address::from_str(token_address)?;
        
        // Kiểm tra từng factory
        for (i, factory_addr) in self.factory_addresses.iter().enumerate() {
            // ABI cho factory (getPair function)
            let factory_abi: ethers::abi::Abi = serde_json::from_str(
                r#"[{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"},{"internalType":"address","name":"","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"}]"#
            )?;
            
            let factory_contract = Contract::new(
                *factory_addr,
                factory_abi,
                Arc::clone(&self.adapter.get_provider())
            );
            
            // Lấy pair address
            let pair_addr: Address = factory_contract
                .method("getPair", (token_addr, self.weth_address))?
                .call()
                .await?;
                
            if pair_addr != Address::zero() {
                // Nếu có router tương ứng
                let router_address = if i < self.router_addresses.len() {
                    Some(format!("{:?}", self.router_addresses[i]))
                } else {
                    None
                };
                
                return Ok((Some(format!("{:?}", pair_addr)), router_address));
            }
        }
        
        // Không tìm thấy pair
        Ok((None, None))
    }
    
    // Lấy giá và liquidity của token
    async fn get_token_price_and_liquidity(&self, token_address: &str, pair_address: Option<&str>) -> Result<(f64, f64, f64)> {
        let pair_addr = match pair_address {
            Some(addr) => Address::from_str(addr)?,
            None => {
                // Không có pair, trả về giá 0
                return Ok((0.0, 0.0, 0.0));
            }
        };
        
        let token_addr = Address::from_str(token_address)?;
        
        // Lấy reserves từ pair contract
        let pair_contract = Contract::new(
            pair_addr,
            self.pair_abi.clone(),
            Arc::clone(&self.adapter.get_provider())
        );
        
        // Xác định token0 và token1
        let token0: Address = pair_contract.method("token0", ())?.call().await?;
        
        // Lấy reserves
        let (reserve0, reserve1, _): (U256, U256, u32) = pair_contract
            .method("getReserves", ())?
            .call()
            .await?;
            
        // Tính giá dựa trên reserves
        let (token_reserve, eth_reserve) = if token0 == token_addr {
            (reserve0, reserve1)
        } else {
            (reserve1, reserve0)
        };
        
        // Lấy số decimals của token
        let token_contract = Contract::new(
            token_addr,
            self.token_abi.clone(),
            Arc::clone(&self.adapter.get_provider())
        );
        
        let decimals: u8 = token_contract.method("decimals", ())?.call().await?;
        
        // Tính giá 1 token = bao nhiêu ETH
        let token_decimals_factor = 10u128.pow(decimals as u32);
        let eth_decimals_factor = 10u128.pow(18); // ETH có 18 decimals
        
        let price_in_eth = if !token_reserve.is_zero() {
            (eth_reserve.as_u128() as f64 * token_decimals_factor as f64) / 
            (token_reserve.as_u128() as f64 * eth_decimals_factor as f64)
        } else {
            0.0
        };
        
        // Giả sử ETH = $2000 USD
        let eth_price_usd = 2000.0;
        let price_usd = price_in_eth * eth_price_usd;
        
        // Tính liquidity (giá trị của ETH trong pair)
        let liquidity = (eth_reserve.as_u128() as f64 / 1e18) * eth_price_usd * 2.0; // *2 vì đây là liquidity ở cả hai bên
        
        Ok((price_usd, price_in_eth, liquidity))
    }
    
    // Lấy top holders
    async fn get_top_holders(&self, token_address: &str) -> Result<Vec<HolderInfo>> {
        // Trong thực tế, cần API bên ngoài để lấy dữ liệu này
        // Đây là phiên bản giả lập
        let mut holders = Vec::new();
        
        // Giả lập top holders
        for i in 0..5 {
            let addr = format!("0x{:040x}", i + 1);
            let percent = 100.0 / (i + 2) as f64;
            let balance = format!("{}", 1_000_000_000 / (i + 1));
            
            holders.push(HolderInfo {
                address: addr,
                is_contract: i == 0, // Giả sử holder đầu tiên là contract
                balance,
                percent,
            });
        }
        
        Ok(holders)
    }
    
    // Ước tính số lượng holders
    async fn estimate_holders_count(&self, token_address: &str) -> Result<u64> {
        // Trong thực tế, cần API bên ngoài
        // Giả lập kết quả
        Ok(1000)
    }
    
    // Thêm callback khi có cảnh báo giá
    pub fn add_alert_callback<F>(&mut self, callback: F)
    where
        F: Fn(TokenPriceAlert) + Send + Sync + 'static
    {
        self.alert_callbacks.push(Box::new(callback));
    }
    
    // Thiết lập ngưỡng thay đổi giá tối thiểu để tạo cảnh báo
    pub fn set_min_alert_percent(&mut self, percent: f64) {
        self.min_alert_percent = percent;
    }
    
    // Phân loại token theo mức độ an toàn
    pub fn classify_token(&self, token_status: &TokenStatus, risk_analysis: Option<&TokenRiskAnalysis>) -> TokenSafetyLevel {
        // Nếu có risk_analysis, sử dụng để phân loại
        if let Some(analysis) = risk_analysis {
            // Phân loại dựa trên điểm rủi ro
            if analysis.base.risk_score < 35.0 {
                return TokenSafetyLevel::Green;
            } else if analysis.base.risk_score < 75.0 {
                return TokenSafetyLevel::Yellow;
            } else {
                return TokenSafetyLevel::Red;
            }
        }
        
        // Phân loại dựa trên thông tin token_status nếu không có risk_analysis
        if !token_status.is_contract_verified || token_status.has_dangerous_functions {
            return TokenSafetyLevel::Red;
        }
        
        // Kiểm tra thuế
        if let Some(tax_info) = &token_status.tax_info {
            if tax_info.buy_tax > 20.0 || tax_info.sell_tax > 20.0 {
                return TokenSafetyLevel::Red;
            }
            
            if tax_info.buy_tax > 10.0 || tax_info.sell_tax > 10.0 {
                return TokenSafetyLevel::Yellow;
            }
        }
        
        // Kiểm tra thanh khoản
        if token_status.liquidity < 5000.0 {
            return TokenSafetyLevel::Red;
        }
        
        if token_status.liquidity < 50000.0 {
            return TokenSafetyLevel::Yellow;
        }
        
        // Mặc định - Green
        TokenSafetyLevel::Green
    }
    
    // Tích hợp với mempool để cập nhật pending_tx_count
    pub async fn update_pending_tx_count(&mut self, token_address: &str, count: u32) -> Result<()> {
        if let Some(status) = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.get_mut(token_address) {
            status.value.pending_tx_count = count;
            Ok(())
        } else {
            Err(anyhow!("Token không được theo dõi"))
        }
    }

    pub async fn update_token_status(&mut self, token_address: &str) -> Result<TokenStatus> {
        // Lấy thông tin cơ bản
        let token_info = self.adapter.get_token_info(token_address).await?;
        
        // Phân tích rủi ro qua RiskAnalyzer
        let risk_analysis = match &self.risk_analyzer {
            Some(analyzer) => match analyzer.analyze_token(token_address).await {
                Ok(analysis) => Some(analysis),
                Err(e) => {
                    warn!("Không thể phân tích rủi ro cho token {}: {}", token_address, e);
                    None
                }
            },
            None => None,
        };
        
        // Cập nhật trạng thái token
        let mut token_status = TokenStatus {
            address: token_address.to_string(),
            symbol: token_info.symbol.clone(),
            name: token_info.name.clone(),
            decimals: token_info.decimals,
            current_price: None,
            price_change_24h: None,
            volume_24h: None,
            market_cap: None,
            liquidity: None,
            holders_count: None,
            is_honeypot: risk_analysis.as_ref().map(|a| a.is_honeypot()).unwrap_or(false),
            buy_tax: risk_analysis.as_ref().and_then(|a| a.tax_info.as_ref().map(|t| t.buy_tax)),
            sell_tax: risk_analysis.as_ref().and_then(|a| a.tax_info.as_ref().map(|t| t.sell_tax)),
            risk_score: risk_analysis.as_ref().map(|a| a.risk_score),
            last_updated: utils::safe_now(),
            safety_level: self.calculate_safety_level(risk_analysis.as_ref()),
        };
        
        // Cập nhật thông tin thị trường
        self.update_token_market_info(&mut token_status).await?;
        
        // Lưu vào cache
        self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?.insert(token_address.to_string(), CacheEntry::new(token_status.clone(), 300));
        
        // Gửi đến AIModule nếu đạt ngưỡng an toàn
        if let Some(safety_threshold) = self.config.ai_safety_threshold {
            if token_status.safety_level >= safety_threshold {
                self.send_to_ai_module(token_address, &token_status, risk_analysis.as_ref()).await?;
            }
        }
        
        Ok(token_status)
    }

    async fn send_to_ai_module(&self, token_address: &str, status: &TokenStatus, risk_analysis: Option<&TokenRiskAnalysis>) -> Result<()> {
        if let Some(ai_module) = &self.ai_module {
            let mut ai_module = ai_module.lock().await;
            ai_module.analyze_new_token(token_address, status, risk_analysis).await?;
            
            info!("Đã gửi thông tin token {} đến AI Module để phân tích.", token_address);
        }
        
        Ok(())
    }

    pub async fn update_token_prices(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Tạo danh sách token cần cập nhật - cần sửa để lấy từ LruCache
        let token_addresses: Vec<String> = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.keys().cloned().collect();
        let count = token_addresses.len();
        
        if count == 0 {
            return Ok(());
        }
        
        // Tạo rate limiter cho các request
        let semaphore = Arc::new(Semaphore::new(5));  // Giới hạn 5 requests đồng thời
        let mut handles = Vec::with_capacity(count);
        
        // Lưu trữ lỗi xảy ra
        let errors = Arc::new(Mutex::new(Vec::new()));
        
        // Tạo các futures để cập nhật giá từng token
        for address in token_addresses {
            let adapter = self.adapter.clone();
            let semaphore_clone = semaphore.clone();
            let errors_clone = errors.clone();
            
            let handle = tokio::spawn(async move {
                // Sử dụng semaphore để giới hạn số lượng requests cùng lúc
                let _permit = match semaphore_clone.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        // Ghi nhận lỗi và thoát sớm
                        let mut errors_locked = errors_clone.lock().await;
                        errors_locked.push(format!("Lỗi semaphore cho token {}: {}", address, e));
                        return None;
                    }
                };
                
                // Cố gắng lấy giá cho token
                match Self::get_token_price_and_liquidity(&adapter, &address, None).await {
                    Ok((price_usd, price_native, liquidity)) => {
                        Some((address, TokenStatus {
                            address: address.clone(),
                            symbol: "TEMP".to_string(),
                            name: "Temporary Token".to_string(),
                            decimals: 18,
                            price_usd,
                            price_native,
                            market_cap: 0.0,
                            total_supply: "0".to_string(),
                            circulating_supply: "0".to_string(),
                            holders_count: 0,
                            liquidity,
                            volume_24h: 0.0,
                            price_change_24h: 0.0,
                            pair_address: None,
                            router_address: "".to_string(),
                            holders: vec![],
                            last_updated: utils::safe_now(),
                            safety_level: TokenSafetyLevel::Yellow,
                            audit_score: None,
                            liquidity_locked: None,
                            is_contract_verified: false,
                            has_dangerous_functions: false,
                            dangerous_functions: vec![],
                            pending_tx_count: 0,
                            tax_info: None,
                        }))
                    },
                    Err(e) => {
                        // Ghi nhận lỗi và trả về None
                        let mut errors_locked = errors_clone.lock().await;
                        errors_locked.push(format!("Lỗi lấy giá token {}: {}", address, e));
                        None
                    }
                }
            });
            
            handles.push(handle);
        }
        
        // Chờ tất cả các tasks hoàn thành
        let results = join_all(handles).await;
        
        // Cập nhật giá vào map
        let mut update_count = 0;
        for result in results {
            match result {
                Ok(Some((address, status))) => {
                    // Cập nhật giá và thời gian cập nhật
                    if let Some(token) = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.get_mut(&address) {
                        *token = CacheEntry::new(status, 300);
                        token.value.last_updated = utils::safe_now();
                        update_count += 1;
                    }
                },
                Ok(None) => { /* Đã xử lý lỗi bên trong task */ },
                Err(e) => {
                    let mut errors_locked = errors.lock().await;
                    errors_locked.push(format!("Task bị lỗi: {}", e));
                }
            }
        }
        
        // Kiểm tra các lỗi xảy ra
        let errors_locked = errors.lock().await;
        if !errors_locked.is_empty() {
            let error_count = errors_locked.len();
            // Chỉ log một số lỗi đầu tiên để tránh quá nhiều log
            let error_sample = errors_locked.iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            warn!("Có {} lỗi khi cập nhật giá token. Mẫu: {}", error_count, error_sample);
        }
        
        debug!("Đã cập nhật giá cho {}/{} tokens", update_count, count);
        Ok(())
    }

    // Lấy thông tin token với timeout để tránh deadlock
    pub async fn get_token_with_timeout(&self, token_address: &str) -> Result<Option<TokenStatus>, Box<dyn std::error::Error>> {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            self.get_token(token_address)
        ).await {
            Ok(result) => result,
            Err(_) => {
                warn!("Timeout khi lấy thông tin token {}", token_address);
                Err("Timeout khi lấy thông tin token".into())
            }
        }
    }
    
    // Cập nhật token status với timeout
    pub async fn update_token_status_with_timeout(&mut self, token_address: &str) -> Result<(), Box<dyn std::error::Error>> {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            self.update_token_status(token_address)
        ).await {
            Ok(result) => result,
            Err(_) => {
                warn!("Timeout khi cập nhật token status {}", token_address);
                Err("Timeout khi cập nhật token status".into())
            }
        }
    }
    
    /// Phương thức dọn dẹp các token cũ không còn được sử dụng
    pub fn cleanup_old_tokens(&mut self, current_time: u64, max_age_seconds: u64) -> usize {
        let tokens_to_remove: Vec<String> = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?
            .iter()
            .filter_map(|(token_address, status)| {
                if let Some(last_updated) = status.value.last_updated {
                    if current_time > last_updated && current_time - last_updated > max_age_seconds {
                        return Some(token_address.clone());
                    }
                }
                None
            })
            .collect();
        
        let count = tokens_to_remove.len();
        for token in tokens_to_remove {
            self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?.remove(token);
        }
        
        count
    }
    
    /// Giảm kích thước cache bằng cách xóa các token ít sử dụng nhất
    pub fn reduce_cache_size(&mut self, target_size: usize) -> usize {
        let current_size = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?.len();
        if current_size <= target_size {
            return 0;
        }
        
        // Sắp xếp tokens theo thời gian cập nhật gần nhất
        let mut tokens_with_time: Vec<(String, u64)> = self.tracked_tokens.read().map_err(|e| anyhow!("RwLock error: {}", e))?
            .iter()
            .map(|(addr, status)| {
                (addr.clone(), status.value.last_updated.unwrap_or(0))
            })
            .collect();
        
        // Sắp xếp theo thời gian tăng dần (cũ nhất lên đầu)
        tokens_with_time.sort_by_key(|(_, time)| *time);
        
        // Xác định số lượng token cần xóa
        let to_remove = current_size - target_size;
        
        // Xóa các token cũ nhất
        for i in 0..to_remove {
            if i < tokens_with_time.len() {
                self.tracked_tokens.write().map_err(|e| anyhow!("RwLock error: {}", e))?.remove(tokens_with_time[i].0);
            }
        }
        
        to_remove
    }
}

/// Thử lấy lock với timeout để tránh deadlock
pub async fn try_lock_tracker(token_status_tracker: &Arc<Mutex<TokenStatusTracker>>) -> Result<MutexGuard<TokenStatusTracker>, Box<dyn std::error::Error>> {
    // Thiết lập timeout 2 giây để tránh treo vô hạn
    match tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        token_status_tracker.try_lock()
    ).await {
        Ok(guard_result) => {
            match guard_result {
                Ok(guard) => Ok(guard),
                Err(e) => {
                    warn!("Không thể lấy lock TokenStatusTracker: {}", e);
                    Err(format!("Lỗi khi lấy lock: {}", e).into())
                }
            }
        },
        Err(_) => {
            warn!("Timeout khi chờ lấy lock TokenStatusTracker");
            Err("Timeout khi chờ lấy lock TokenStatusTracker".into())
        }
    }
}

/// Khôi phục trạng thái nếu phát hiện deadlock
pub async fn recover_from_deadlock(token_status_tracker: &Arc<Mutex<TokenStatusTracker>>) -> Result<(), Box<dyn std::error::Error>> {
    // Tạo tracker mới
    let new_tracker = TokenStatusTracker {
        tracked_tokens: Arc::new(RwLock::new(HashMap::new())),
        price_alerts: HashMap::new(),
        alert_callbacks: Vec::new(),
        adapter: token_status_tracker.lock().await?.adapter.clone(),
    };
    
    // Khởi tạo lại tracker
    match token_status_tracker.try_lock() {
        Ok(mut guard) => {
            *guard = new_tracker;
            Ok(())
        },
        Err(_) => {
            warn!("Không thể lấy lock để khôi phục từ deadlock, có thể cần khởi động lại dịch vụ");
            Err("Không thể khôi phục từ deadlock".into())
        }
    }
}

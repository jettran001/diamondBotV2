use ethers::types::{Address, U256};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::time::sleep;
use tracing::{info, warn, error};
use crate::chain_adapters::ChainAdapter;
use crate::types::{TradeConfig, TradeStats};
use log::{info, error, debug, warn};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use futures::Stream;
use super::config::Config;
use super::service::ServiceMessage;
use std::collections::{VecDeque};
use std::num::NonZeroUsize;
use serde::{Serialize, Deserialize};
use super::utils;
use lru::LruCache;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSwap {
    pub tx_hash: String,
    pub token_address: String,
    pub is_buy: bool,
    pub amount_usd: f64,
    pub gas_price: U256,
    pub timestamp: u64,
}

pub struct MempoolWatcher {
    provider: Provider<Http>,
    config: Config,
    tx_sender: mpsc::Sender<ServiceMessage>,
    watching_tokens: Vec<String>,
    mempool_tracker: Option<MempoolTracker>,
    counter: AtomicU32,
}

impl MempoolWatcher {
    pub fn new(config: Config, tx_sender: mpsc::Sender<ServiceMessage>) -> Self {
        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .expect("Không thể khởi tạo provider");
            
        Self {
            provider,
            config,
            tx_sender,
            watching_tokens: Vec::new(),
            mempool_tracker: Some(MempoolTracker::new(1000.0)), // $1000 cho large txs
            counter: AtomicU32::new(0),
        }
    }
    
    pub fn add_tokens_to_watch(&mut self, tokens: Vec<String>) {
        for token in tokens {
            if !self.watching_tokens.contains(&token) {
                info!("Thêm token vào danh sách theo dõi: {}", token);
                self.watching_tokens.push(token);
            }
        }
    }
    
    pub async fn start_watching(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Bắt đầu theo dõi mempool");
        
        // Tạo stream pending transactions
        let tx_stream = self.provider.watch_pending_transactions().await?;
        
        self.process_transactions(tx_stream).await?;
        
        Ok(())
    }
    
    async fn process_transactions<S: Stream<Item = Result<H256, ProviderError>> + Unpin>(
        &self,
        mut tx_stream: S
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut mempool_tracker = MempoolTracker::new(1000.0); // $1000 cho large txs
        
        // Xử lý các giao dịch trong stream
        while let Some(tx_hash_result) = tx_stream.next().await {
            match tx_hash_result {
                Ok(tx_hash) => {
                    // Lấy chi tiết transaction từ hash với timeout
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        self.provider.get_transaction(tx_hash)
                    ).await {
                        Ok(tx_result) => match tx_result {
                            Ok(Some(tx)) => {
                                if let Some(to) = tx.to {
                                    // Kiểm tra nếu tx là tới router
                                    if to.to_string().to_lowercase() == self.config.router_address.to_lowercase() {
                                        // Phân tích input data để xác định token, loại (mua/bán), và số lượng
                                        if let Some((token_address, is_buy, amount_usd)) = self.analyze_transaction(&tx).await {
                                            // Thêm vào mempool tracker
                                            let pending_swap = PendingSwap {
                                                tx_hash: format!("{:?}", tx_hash),
                                                token_address,
                                                is_buy,
                                                amount_usd,
                                                gas_price: tx.gas_price.unwrap_or_default(),
                                                timestamp: utils::safe_now(),
                                            };
                                            
                                            mempool_tracker.add_pending_swap(pending_swap.clone());
                                            
                                            // Gửi thông báo cho các token đang được theo dõi
                                            if self.watching_tokens.contains(&pending_swap.token_address) {
                                                // Gửi message về swap đang chờ xử lý
                                                if let Err(e) = self.tx_sender.send(ServiceMessage::PendingSwap(pending_swap)).await {
                                                    error!("Lỗi khi gửi thông báo: {}", e);
                                                }
                                                
                                                // Nếu là giao dịch lớn, gửi thêm thông báo
                                                if amount_usd >= 10000.0 {
                                                    if let Err(e) = self.tx_sender.send(ServiceMessage::LargeTransaction {
                                                        token_address: pending_swap.token_address.clone(),
                                                        is_buy: pending_swap.is_buy,
                                                        amount_usd: pending_swap.amount_usd,
                                                    }).await {
                                                        error!("Lỗi khi gửi thông báo giao dịch lớn: {}", e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Ok(None) => debug!("Không tìm thấy giao dịch: {:?}", tx_hash),
                            Err(e) => error!("Lỗi khi lấy giao dịch {:?}: {}", tx_hash, e),
                        },
                        Err(_) => warn!("Timeout khi lấy giao dịch {:?}", tx_hash),
                    }
                },
                Err(e) => error!("Lỗi khi nhận transaction hash: {}", e),
            }
            
            // Dọn dẹp dữ liệu cũ mỗi 100 giao dịch - sử dụng AtomicU32 thay vì static mut
            let counter = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            if counter % 100 == 0 {
                mempool_tracker.cleanup_old_data(300); // 5 phút
            }
            
            // Kiểm tra cơ hội arbitrage và sandwich
            if let Some(opportunity) = mempool_tracker.get_best_arbitrage_opportunity() {
                if opportunity.potential_profit_percent >= 1.0 { // Tối thiểu 1%
                    if let Err(e) = self.tx_sender.send(ServiceMessage::ArbitrageOpportunity(opportunity.clone())).await {
                        error!("Lỗi khi gửi thông báo cơ hội arbitrage: {}", e);
                    }
                }
            }
            
            if let Some(opportunity) = mempool_tracker.get_best_sandwich_opportunity() {
                if opportunity.potential_profit >= 50.0 { // Tối thiểu $50
                    if let Err(e) = self.tx_sender.send(ServiceMessage::SandwichOpportunity(opportunity.clone())).await {
                        error!("Lỗi khi gửi thông báo cơ hội sandwich: {}", e);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    // Phân tích giao dịch để xác định token, loại giao dịch và số lượng
    async fn analyze_transaction(&self, tx: &Transaction) -> Option<(String, bool, f64)> {
        // Sử dụng timeout cho toàn bộ phân tích để tránh treo
        match tokio::time::timeout(Duration::from_secs(1), self._analyze_transaction_internal(tx)).await {
            Ok(result) => result,
            Err(_) => {
                warn!("Timeout khi phân tích giao dịch: {:?}", tx.hash);
                None
            }
        }
    }
    
    // Phương thức nội bộ để phân tích giao dịch
    async fn _analyze_transaction_internal(&self, tx: &Transaction) -> Option<(String, bool, f64)> {
        // Phân tích input data để xác định token, loại (mua/bán), và số lượng
        // Đây là một phiên bản đơn giản, cần triển khai chi tiết hơn trong thực tế
        
        if let Some(input) = &tx.input {
            if input.len() < 4 {
                // Input quá ngắn để phân tích
                debug!("Input data quá ngắn để phân tích: {:?}", tx.hash);
                return None;
            }
            
            let input_str = match hex::encode(input) {
                s if s.len() < 140 => {
                    debug!("Input data không đủ dài: {:?}", tx.hash);
                    return None;
                },
                s => s
            };
            
            // Phân tích function signature từ input data
            let func_signature = &input_str[0..8]; // Lấy 4 bytes đầu tiên
            
            // Kiểm tra các function swaps phổ biến
            let is_buy = func_signature == "7ff36ab5" || // swapExactETHForTokens
                         func_signature == "b6f9de95" || // swapExactETHForTokensSupportingFeeOnTransferTokens
                         input_str.contains("ETHForTokens"); // Các hàm ETHForTokens khác
                         
            let is_sell = func_signature == "791ac947" || // swapExactTokensForETHSupportingFeeOnTransferTokens
                          func_signature == "18cbafe5" || // swapExactTokensForETH
                          input_str.contains("TokensForETH"); // Các hàm TokensForETH khác
            
            if is_buy || is_sell {
                // Trong thực tế, cần phân tích ABI để lấy thông tin chính xác
                
                // Tìm và trích xuất địa chỉ token
                let token_address = if is_buy {
                    // Với các giao dịch mua, địa chỉ token thường nằm trong đường dẫn token
                    // Chúng ta giả định nó là tham số cuối cùng trong path
                    if input_str.len() >= 140 {
                        match self.extract_token_address_from_path(&input_str) {
                            Some(addr) => addr,
                            None => {
                                debug!("Không thể phân tích địa chỉ token từ path: {:?}", tx.hash);
                                return None;
                            }
                        }
                    } else {
                        debug!("Input data không đủ dài để phân tích: {:?}", tx.hash);
                        return None;
                    }
                } else {
                    // Với các giao dịch bán, địa chỉ token thường là tham số đầu tiên
                    if input_str.len() >= 140 {
                        match self.extract_token_address_for_sell(&input_str) {
                            Some(addr) => addr,
                            None => {
                                debug!("Không thể phân tích địa chỉ token cho giao dịch bán: {:?}", tx.hash);
                                return None;
                            }
                        }
                    } else {
                        debug!("Input data không đủ dài để phân tích: {:?}", tx.hash);
                        return None;
                    }
                };
                
                // Tính toán giá trị trong USD
                let amount_eth = if is_buy {
                    // Cho giao dịch mua, giá trị là giá trị ETH được gửi
                    ethers::utils::format_ether(tx.value)
                } else {
                    // Cho giao dịch bán, chúng ta cần phân tích giá trị từ input
                    // Đây là một ước tính đơn giản, cần phân tích chi tiết hơn trong thực tế
                    match self.extract_amount_from_sell_input(&input_str) {
                        Some(amount) => amount,
                        None => "0.0".to_string()
                    }
                };
                
                let amount_eth: f64 = match amount_eth.parse::<f64>() {
                    Ok(val) => val,
                    Err(_) => {
                        debug!("Không thể phân tích số lượng ETH: {:?}", tx.hash);
                        0.0
                    }
                };
                
                // Lấy giá ETH từ config hoặc cache
                let eth_price = self.config.eth_price.unwrap_or(2000.0);
                let amount_usd = amount_eth * eth_price;
                
                return Some((token_address, is_buy, amount_usd));
            }
        }
        
        None
    }
    
    // Hàm hỗ trợ để trích xuất địa chỉ token từ path
    fn extract_token_address_from_path(&self, input_str: &str) -> Option<String> {
        // Đây là một phương pháp đơn giản, cần cải thiện trong thực tế
        // Thông thường địa chỉ token nằm trong 32 bytes cuối của input data
        let path_offset = input_str.len().saturating_sub(64);
        if path_offset > 0 {
            Some(format!("0x{}", &input_str[path_offset..]))
        } else {
            None
        }
    }
    
    // Hàm hỗ trợ để trích xuất địa chỉ token cho giao dịch bán
    fn extract_token_address_for_sell(&self, input_str: &str) -> Option<String> {
        // Địa chỉ token thường nằm sau function selector
        if input_str.len() >= 72 { // 4 bytes function selector + 32 bytes padding
            Some(format!("0x{}", &input_str[32..72]))
        } else {
            None
        }
    }
    
    // Hàm hỗ trợ để trích xuất số lượng từ input cho giao dịch bán
    fn extract_amount_from_sell_input(&self, input_str: &str) -> Option<String> {
        // Số lượng thường nằm sau địa chỉ token
        if input_str.len() >= 136 { // 4 bytes + 32 bytes token + 32 bytes amount
            let amount_hex = &input_str[72..136];
            if let Ok(amount_bytes) = hex::decode(amount_hex) {
                if amount_bytes.len() == 32 {
                    let amount = U256::from_big_endian(&amount_bytes);
                    return Some(ethers::utils::format_ether(amount));
                }
            }
        }
        None
    }
    
    async fn filter_transactions(&self, tx_hash: H256) -> bool {
        // Lọc nhanh dựa trên gas price, method signature, to address
        // để giảm số lượng fetch full transaction
        
        if let Ok(Some(tx)) = self.provider.get_transaction(tx_hash).await {
            if let Some(to) = tx.to {
                // Kiểm tra có phải gọi đến router, factory hoặc pair không
                if self.is_target_address(&to) {
                    return true;
                }
            }
        }
        
        false
    }
    
    fn is_target_address(&self, address: &Address) -> bool {
        // Kiểm tra có phải router, factory, hoặc pair token đang theo dõi
        // Ví dụ đơn giản: Kiểm tra với router address
        address.to_string().to_lowercase() == self.config.router_address.to_lowercase()
    }
    
    // Đếm số lượng giao dịch đang chờ xử lý cho một token
    pub async fn count_pending_txs_for_token(&self, token_address: &str) -> u32 {
        if let Some(mempool_tracker) = &self.mempool_tracker {
            if let Some(swaps) = mempool_tracker.pending_swaps.get(token_address) {
                return swaps.len() as u32;
            }
        }
        
        0 // Không có giao dịch nào đang chờ
    }
    
    // Cập nhật TokenStatus với số lượng giao dịch đang chờ
    pub async fn update_token_status_with_pending_txs(&self, token_tracker: &mut dyn TokenStatusUpdater) -> Result<(), Box<dyn std::error::Error>> {
        for token in &self.watching_tokens {
            let pending_count = self.count_pending_txs_for_token(token).await;
            token_tracker.update_pending_tx_count(token, pending_count).await?;
        }
        
        Ok(())
    }

    pub async fn process_pending_transactions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Lấy thời gian hiện tại một cách an toàn
        let current_time = utils::safe_now();
        
        // Xóa dữ liệu cũ để giảm memory leak
        self.cleanup_old_data(3600); // 1 giờ
        
        // Lấy các giao dịch đang chờ với timeout
        let pending_txns = match tokio::time::timeout(
            Duration::from_secs(10),
            self.provider.get_pending_transactions()
        ).await {
            Ok(Ok(txns)) => txns,
            Ok(Err(e)) => {
                error!("Lỗi khi lấy giao dịch đang chờ: {}", e);
                return Err(e.into());
            },
            Err(_) => {
                error!("Timeout khi lấy giao dịch đang chờ");
                return Err("Timeout khi lấy giao dịch đang chờ".into());
            }
        };
        
        // Tạo các task riêng biệt để xử lý từng giao dịch
        let mut join_set = tokio::task::JoinSet::new();
        
        // Giới hạn số lượng task xử lý đồng thời để tránh quá tải
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10)); // Giới hạn 10 task đồng thời
        
        // Chỉ xử lý tối đa 500 giao dịch mỗi lần để tránh quá tải
        let max_txs_to_process = std::cmp::min(pending_txns.len(), 500);
        let pending_txns_slice = &pending_txns[0..max_txs_to_process];
        
        for tx in pending_txns_slice {
            // Clone các biến cần thiết
            let config = self.config.clone();
            let semaphore_clone = semaphore.clone();
            let watching_tokens = self.watching_tokens.clone();
            
            // Tạo task riêng với semaphore để giới hạn số lượng đồng thời
            join_set.spawn(async move {
                // Lấy permit từ semaphore để giới hạn số lượng task đồng thời
                let _permit = match semaphore_clone.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        error!("Lỗi khi lấy semaphore permit: {}", e);
                        return Err(format!("Lỗi semaphore: {}", e));
                    }
                };
                
                // Xử lý giao dịch
                // Gửi thông báo nếu phát hiện token quan tâm
                if let Some(to) = tx.to {
                    if watching_tokens.contains(&to.to_string()) {
                        debug!("Phát hiện giao dịch đến token quan tâm: {}", to);
                    }
                }
                
                Ok(tx.hash.to_string())
            });
        }
        
        // Thu thập kết quả với timeout tổng thể
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        
        while let Some(result) = tokio::time::timeout(
            timeout.saturating_sub(start.elapsed()),
            join_set.join_next()
        ).await.unwrap_or(None) {
            match result {
                Ok(Ok(tx_hash)) => {
                    debug!("Đã xử lý giao dịch {}", tx_hash);
                }
                Ok(Err(e)) => {
                    warn!("Lỗi khi xử lý giao dịch: {}", e);
                }
                Err(e) => {
                    error!("Lỗi task: {}", e);
                }
            }
        }
        
        // Hủy các task còn lại nếu vượt quá thời gian
        join_set.abort_all();
        
        // Giới hạn kích thước các map
        self.limit_maps_size();
        
        Ok(())
    }

    /// Giới hạn kích thước các map để tránh rò rỉ bộ nhớ
    fn limit_maps_size(&mut self) {
        const MAX_SIZE: usize = 1000;
        
        // Giới hạn kích thước các map chính
        if self.transactions.len() > MAX_SIZE {
            let keys: Vec<String> = self.transactions.keys()
                .take(self.transactions.len() - MAX_SIZE)
                .cloned()
                .collect();
            
            for key in keys {
                self.transactions.remove(&key);
            }
        }
        
        // Giới hạn token_volumes
        if self.token_volumes.len() > MAX_SIZE {
            let keys: Vec<String> = self.token_volumes.keys()
                .take(self.token_volumes.len() - MAX_SIZE)
                .cloned()
                .collect();
            
            for key in keys {
                self.token_volumes.remove(&key);
            }
        }
        
        // Giới hạn size của các vector
        if self.arbitrage_opportunities.len() > MAX_SIZE {
            self.arbitrage_opportunities.truncate(MAX_SIZE);
        }
        
        if self.sandwich_attacks.len() > MAX_SIZE {
            self.sandwich_attacks.truncate(MAX_SIZE);
        }
        
        if self.frontrun_transactions.len() > MAX_SIZE {
            self.frontrun_transactions.truncate(MAX_SIZE);
        }
    }
}

// Define a trait for updating token status with pending tx count
pub trait TokenStatusUpdater {
    async fn update_pending_tx_count(&mut self, token_address: &str, count: u32) -> Result<(), Box<dyn std::error::Error>>;
}

// Thêm định nghĩa cho TokenStatusTracker nếu cần
pub struct TokenStatusTracker<M: Middleware + 'static> {
    pub tracked_tokens: HashMap<String, TokenStatus>,
    _phantom: std::marker::PhantomData<M>,
}

// Định nghĩa TokenStatus
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenStatus {
    pub pending_tx_count: u32,
    // Thêm các trường khác nếu cần
}

// Implement the trait for TokenStatusTracker
impl<M: Middleware + 'static> TokenStatusUpdater for TokenStatusTracker<M> {
    async fn update_pending_tx_count(&mut self, token_address: &str, count: u32) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(status) = self.tracked_tokens.get_mut(token_address) {
            status.pending_tx_count = count;
            Ok(())
        } else {
            Err("Token không được theo dõi".into())
        }
    }
}

// Định nghĩa cấu trúc theo dõi mempool cải tiến
pub struct MempoolTracker {
    pub pending_swaps: HashMap<String, Vec<PendingSwap>>,
    pub large_txs: HashMap<String, Vec<LargeTransaction>>,
    pub arbitrage_opportunities: LruCache<String, ArbitrageOpportunity>,
    pub tracked_tokens: HashMap<String, TokenMetrics>,
    pub sandwich_opportunities: LruCache<String, SandwichOpportunity>,
    pub min_large_tx_amount: f64, // $ amount
    pub max_items_per_token: usize, // Giới hạn số lượng items cho mỗi token
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LargeTransaction {
    pub tx_hash: String,
    pub token_address: String,
    pub is_buy: bool,
    pub amount_usd: f64,
    pub gas_price: U256,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub token_address: String,
    pub exchange1: String,
    pub price1: f64,
    pub exchange2: String,
    pub price2: f64,
    pub potential_profit_percent: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichOpportunity {
    pub token_address: String,
    pub victim_tx_hash: String,
    pub amount_usd: f64,
    pub estimated_price_impact: f64,
    pub potential_profit: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichResult {
    pub token_address: String,
    pub victim_tx_hash: String,
    pub front_tx_hash: Option<String>,
    pub back_tx_hash: Option<String>,
    pub success: bool,
    pub profit_usd: f64,
    pub front_run_confirmation_time: u64, // ms
    pub total_execution_time: u64, // ms
    pub timestamp: u64,
    pub profit: f64,
    pub front_run_gas_cost: f64,
    pub back_run_gas_cost: f64,
    pub execution_time: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenMetrics {
    pub buy_pressure: u32,
    pub sell_pressure: u32,
    pub large_buys_count: u32,
    pub large_sells_count: u32,
    pub avg_buy_amount: f64,
    pub avg_sell_amount: f64,
    pub pending_buy_volume: f64,
    pub pending_sell_volume: f64,
    pub last_updated: u64,
}

impl MempoolTracker {
    pub fn new(min_large_tx_amount: f64) -> Self {
        // Khởi tạo với kích thước LRU hợp lý
        let arb_cache_size = NonZeroUsize::new(500).unwrap();
        let sandwich_cache_size = NonZeroUsize::new(500).unwrap();
        
        Self {
            pending_swaps: HashMap::new(),
            large_txs: HashMap::new(),
            arbitrage_opportunities: LruCache::new(arb_cache_size),
            tracked_tokens: HashMap::new(),
            sandwich_opportunities: LruCache::new(sandwich_cache_size),
            min_large_tx_amount,
            max_items_per_token: 100, // Giới hạn mặc định cho mỗi token
        }
    }
    
    // Thêm giao dịch vào danh sách theo dõi
    pub fn add_pending_swap(&mut self, swap: PendingSwap) {
        // Kiểm tra xem đây có phải là giao dịch lớn không
        if swap.amount_usd >= self.min_large_tx_amount {
            let large_tx = LargeTransaction {
                tx_hash: swap.tx_hash.clone(),
                token_address: swap.token_address.clone(),
                is_buy: swap.is_buy,
                amount_usd: swap.amount_usd,
                gas_price: swap.gas_price,
                timestamp: swap.timestamp,
            };
            
            let large_txs = self.large_txs.entry(swap.token_address.clone()).or_insert_with(Vec::new);
            large_txs.push(large_tx);
        }
        
        // Cập nhật metrics
        let metrics = self.tracked_tokens.entry(swap.token_address.clone()).or_insert_with(|| TokenMetrics {
            buy_pressure: 0,
            sell_pressure: 0,
            large_buys_count: 0,
            large_sells_count: 0,
            avg_buy_amount: 0.0,
            avg_sell_amount: 0.0,
            pending_buy_volume: 0.0,
            pending_sell_volume: 0.0,
            last_updated: swap.timestamp,
        });
        
        if swap.is_buy {
            metrics.buy_pressure += 1;
            metrics.pending_buy_volume += swap.amount_usd;
            if swap.amount_usd >= self.min_large_tx_amount {
                metrics.large_buys_count += 1;
            }
            
            let total_buys = metrics.buy_pressure as f64;
            metrics.avg_buy_amount = ((metrics.avg_buy_amount * (total_buys - 1.0)) + swap.amount_usd) / total_buys;
        } else {
            metrics.sell_pressure += 1;
            metrics.pending_sell_volume += swap.amount_usd;
            if swap.amount_usd >= self.min_large_tx_amount {
                metrics.large_sells_count += 1;
            }
            
            let total_sells = metrics.sell_pressure as f64;
            metrics.avg_sell_amount = ((metrics.avg_sell_amount * (total_sells - 1.0)) + swap.amount_usd) / total_sells;
        }
        
        metrics.last_updated = swap.timestamp;
        
        let swaps = self.pending_swaps.entry(swap.token_address.clone()).or_insert_with(Vec::new);
        swaps.push(swap);
        
        // Tìm cơ hội sandwich
        self.find_sandwich_opportunities(&swap.token_address);
    }
    
    // Tìm cơ hội sandwich
    fn find_sandwich_opportunities(&mut self, token_address: &str) {
        if let Some(swaps) = self.pending_swaps.get(token_address) {
            // Tìm các giao dịch mua lớn
            let large_buys: Vec<&PendingSwap> = swaps.iter()
                .filter(|swap| swap.is_buy && swap.amount_usd >= self.min_large_tx_amount)
                .collect();
                
            for buy in large_buys {
                // Tạo cơ hội sandwich
                let opportunity = SandwichOpportunity {
                    token_address: token_address.to_string(),
                    victim_tx_hash: buy.tx_hash.clone(),
                    amount_usd: buy.amount_usd,
                    estimated_price_impact: self.estimate_price_impact(token_address, buy.amount_usd),
                    potential_profit: self.estimate_sandwich_profit(token_address, buy.amount_usd),
                    timestamp: utils::safe_now(), // Sử dụng hàm an toàn thay vì unwrap
                };
                
                // Thêm vào cache LRU nếu có lợi nhuận tiềm năng
                if opportunity.potential_profit > 0.0 {
                    self.add_sandwich_opportunity(opportunity);
                }
            }
        }
    }
    
    // Ước tính tác động giá
    fn estimate_price_impact(&self, token_address: &str, amount_usd: f64) -> f64 {
        // Mô hình đơn giản: 1% giá trị của giao dịch lớn
        // Trong thực tế, cần tính toán phức tạp hơn dựa trên độ sâu thanh khoản
        amount_usd * 0.01
    }
    
    // Ước tính lợi nhuận sandwich
    fn estimate_sandwich_profit(&self, token_address: &str, amount_usd: f64) -> f64 {
        // Mô hình đơn giản: 0.5% giá trị của giao dịch lớn
        // Trong thực tế, cần mô hình phức tạp hơn
        amount_usd * 0.005
    }
    
    // Thêm cơ hội arbitrage với LRU
    pub fn add_arbitrage_opportunity(&mut self, opportunity: ArbitrageOpportunity) {
        let key = format!("{}_{}_{}_{}", 
            opportunity.token_address,
            opportunity.exchange1,
            opportunity.exchange2,
            opportunity.timestamp
        );
        self.arbitrage_opportunities.put(key, opportunity);
    }
    
    // Thêm cơ hội sandwich với LRU
    pub fn add_sandwich_opportunity(&mut self, opportunity: SandwichOpportunity) {
        let key = format!("{}_{}", opportunity.victim_tx_hash, opportunity.timestamp);
        self.sandwich_opportunities.put(key, opportunity);
    }
    
    // Dọn dẹp dữ liệu cũ để tránh rò rỉ bộ nhớ
    pub fn cleanup_old_data(&mut self, max_age_seconds: u64) {
        let current_time = utils::safe_now();
        
        // Giới hạn kích thước của các cấu trúc dữ liệu
        const MAX_SIZE: usize = 1000;
        
        // Dọn dẹp transactions theo thời gian
        let mut tx_keys_to_remove = Vec::new();
        for (tx_hash, tx_data) in self.pending_swaps.iter() {
            if let Some(timestamp) = tx_data.iter().map(|swap| swap.timestamp).max() {
                if current_time.saturating_sub(timestamp) >= max_age_seconds {
                    tx_keys_to_remove.push(tx_hash.clone());
                }
            }
        }
        
        for key in tx_keys_to_remove {
            self.pending_swaps.remove(&key);
        }
        
        // LRU tự động quản lý kích thước, không cần phải xóa thủ công
        
        // Giới hạn kích thước các map khác
        self.limit_maps_size();
    }
    
    // Lấy cơ hội arbitrage tốt nhất
    pub fn get_best_arbitrage_opportunity(&self) -> Option<&ArbitrageOpportunity> {
        self.arbitrage_opportunities
            .iter()
            .max_by(|(_, a), (_, b)| {
                a.potential_profit_percent
                 .partial_cmp(&b.potential_profit_percent)
                 .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, op)| op)
    }
    
    // Lấy cơ hội sandwich tốt nhất
    pub fn get_best_sandwich_opportunity(&self) -> Option<&SandwichOpportunity> {
        self.sandwich_opportunities
            .iter()
            .max_by(|(_, a), (_, b)| {
                a.potential_profit
                 .partial_cmp(&b.potential_profit)
                 .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, op)| op)
    }
    
    // Lấy danh sách các cơ hội arbitrage
    pub fn get_arbitrage_opportunities(&self) -> Vec<&ArbitrageOpportunity> {
        self.arbitrage_opportunities.iter().map(|(_, op)| op).collect()
    }
    
    // Lấy danh sách các cơ hội sandwich
    pub fn get_sandwich_opportunities(&self) -> Vec<&SandwichOpportunity> {
        self.sandwich_opportunities.iter().map(|(_, op)| op).collect()
    }
    
    // Lấy áp lực mua/bán cho token
    pub fn get_token_metrics(&self, token_address: &str) -> Option<&TokenMetrics> {
        self.tracked_tokens.get(token_address)
    }
}

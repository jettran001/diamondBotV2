// External imports
use ethers::types::{Address, U256, H256, Transaction};
use ethers::providers::{Provider, Http, ProviderError};

// Standard library imports
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::str::FromStr;
use std::num::NonZeroUsize;

// Internal imports
use crate::types::*;
use crate::chain_adapters::ChainAdapter;
use super::config::Config;
use super::service::ServiceMessage;
use super::utils;

// Third party imports
use tokio::time::sleep;
use tokio::sync::mpsc;
use futures::Stream;
use futures::StreamExt;
use tracing::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
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

// Kết hợp từ MempoolMonitor
pub struct MempoolMonitorConfig {
    pub monitoring_duration: Duration,
    pub max_tokens: usize,
    pub min_transaction_value: f64,
}

impl Default for MempoolMonitorConfig {
    fn default() -> Self {
        Self {
            monitoring_duration: Duration::from_secs(3600), // 1 giờ
            max_tokens: 1000,
            min_transaction_value: 0.1, // 0.1 ETH
        }
    }
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
                    tx.value.as_u64() as f64 / 1e18
                } else {
                    // Cho giao dịch bán, cần phân tích từ input data
                    // Trong thực tế, đây sẽ là một quá trình phức tạp hơn
                    match self.extract_amount_from_sell_input(&input_str) {
                        Some(amount_str) => {
                            if let Ok(amount) = u128::from_str_radix(&amount_str, 16) {
                                amount as f64 / 1e18
                            } else {
                                0.0
                            }
                        },
                        None => 0.0
                    }
                };
                
                // Giả sử 1 ETH = 3000 USD
                // Trong thực tế, bạn cần lấy giá hiện tại từ oracle hoặc API
                let eth_price_usd = 3000.0;
                let amount_usd = amount_eth * eth_price_usd;
                
                return Some((token_address, is_buy, amount_usd));
            }
        }
        
        None
    }
    
    // Tích hợp các chức năng từ MempoolMonitor
    // Thêm token vào monitoring
    pub fn add_token(&mut self, token_address: String) {
        // Thêm token vào danh sách theo dõi
        self.add_tokens_to_watch(vec![token_address]);
    }
    
    // Đăng ký callback khi phát hiện giao dịch trong mempool
    pub fn add_callback<F>(&mut self, callback: F)
    where
        F: Fn(String, u64) + Send + Sync + 'static,
    {
        info!("Đã đăng ký callback cho mempool watcher");
        // Triển khai sau nếu cần
    }
    
    // Kiểm tra mempool và trả về giao dịch đang chờ
    pub async fn check_mempool(&self) -> Result<Vec<PendingSwap>, Box<dyn std::error::Error>> {
        // Lấy dữ liệu từ mempool_tracker nếu có
        if let Some(tracker) = &self.mempool_tracker {
            let mut result = Vec::new();
            
            // Thu thập tất cả các pending swaps từ tracker
            for (_, swaps) in tracker.pending_swaps.iter() {
                for swap in swaps {
                    result.push(swap.clone());
                }
            }
            
            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }
    
    // Tìm cơ hội sandwich
    pub async fn find_sandwich_opportunities(&self) -> Result<Vec<SandwichOpportunity>, Box<dyn std::error::Error>> {
        if let Some(tracker) = &self.mempool_tracker {
            Ok(tracker.get_sandwich_opportunities().into_iter().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }
    
    // Lấy các cơ hội arbitrage
    pub async fn find_arbitrage_opportunities(&self) -> Result<Vec<ArbitrageOpportunity>, Box<dyn std::error::Error>> {
        if let Some(tracker) = &self.mempool_tracker {
            Ok(tracker.get_arbitrage_opportunities().into_iter().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }
    
    // ... giữ lại các phương thức hiện có ...
    
    fn extract_token_address_from_path(&self, input_str: &str) -> Option<String> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
    
    fn extract_token_address_for_sell(&self, input_str: &str) -> Option<String> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
    
    fn extract_amount_from_sell_input(&self, input_str: &str) -> Option<String> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
    
    async fn filter_transactions(&self, tx_hash: H256) -> bool {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        false
    }
    
    fn is_target_address(&self, address: &Address) -> bool {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        false
    }
    
    pub async fn count_pending_txs_for_token(&self, token_address: &str) -> u32 {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        0
    }
    
    pub async fn update_token_status_with_pending_txs(&self, token_tracker: &mut dyn TokenStatusUpdater) -> Result<(), Box<dyn std::error::Error>> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Ok(())
    }
    
    pub async fn process_pending_transactions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Ok(())
    }
    
    fn limit_maps_size(&mut self) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
}

// ... giữ lại các traits, structs, và implementations khác ...

pub trait TokenStatusUpdater {
    async fn update_pending_tx_count(&mut self, token_address: &str, count: u32) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct TokenStatusTracker<M: Middleware + 'static> {
    pub tracked_tokens: HashMap<String, TokenStatus>,
    _phantom: std::marker::PhantomData<M>,
}

pub struct TokenStatus {
    pub pending_tx_count: u32,
    // Thêm các trường khác nếu cần
}

impl<M: Middleware + 'static> TokenStatusUpdater for TokenStatusTracker<M> {
    async fn update_pending_tx_count(&mut self, token_address: &str, count: u32) -> Result<(), Box<dyn std::error::Error>> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Ok(())
    }
}

pub struct MempoolTracker {
    pub pending_swaps: HashMap<String, Vec<PendingSwap>>,
    pub large_txs: HashMap<String, Vec<LargeTransaction>>,
    pub arbitrage_opportunities: LruCache<String, ArbitrageOpportunity>,
    pub tracked_tokens: HashMap<String, TokenMetrics>,
    pub sandwich_opportunities: LruCache<String, SandwichOpportunity>,
    pub min_large_tx_amount: f64, // $ amount
    pub max_items_per_token: usize, // Giới hạn số lượng items cho mỗi token
}

pub struct LargeTransaction {
    pub tx_hash: String,
    pub token_address: String,
    pub is_buy: bool,
    pub amount_usd: f64,
    pub gas_price: U256,
    pub timestamp: u64,
}

pub struct ArbitrageOpportunity {
    pub token_address: String,
    pub exchange1: String,
    pub price1: f64,
    pub exchange2: String,
    pub price2: f64,
    pub potential_profit_percent: f64,
    pub timestamp: u64,
}

pub struct SandwichOpportunity {
    pub token_address: String,
    pub victim_tx_hash: String,
    pub amount_usd: f64,
    pub estimated_price_impact: f64,
    pub potential_profit: f64,
    pub timestamp: u64,
}

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
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Self {
            pending_swaps: HashMap::new(),
            large_txs: HashMap::new(),
            arbitrage_opportunities: LruCache::new(NonZeroUsize::new(100).unwrap()),
            tracked_tokens: HashMap::new(),
            sandwich_opportunities: LruCache::new(NonZeroUsize::new(100).unwrap()),
            min_large_tx_amount,
            max_items_per_token: 100,
        }
    }
    
    pub fn add_pending_swap(&mut self, swap: PendingSwap) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
    
    fn find_sandwich_opportunities(&mut self, token_address: &str) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
    
    fn estimate_price_impact(&self, token_address: &str, amount_usd: f64) -> f64 {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        0.0
    }
    
    fn estimate_sandwich_profit(&self, token_address: &str, amount_usd: f64) -> f64 {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        0.0
    }
    
    pub fn add_arbitrage_opportunity(&mut self, opportunity: ArbitrageOpportunity) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
    
    pub fn add_sandwich_opportunity(&mut self, opportunity: SandwichOpportunity) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
    
    pub fn cleanup_old_data(&mut self, max_age_seconds: u64) {
        // ... existing code ...
        // Kế thừa từ mempool.rs
    }
    
    pub fn get_best_arbitrage_opportunity(&self) -> Option<&ArbitrageOpportunity> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
    
    pub fn get_best_sandwich_opportunity(&self) -> Option<&SandwichOpportunity> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
    
    pub fn get_arbitrage_opportunities(&self) -> Vec<&ArbitrageOpportunity> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Vec::new()
    }
    
    pub fn get_sandwich_opportunities(&self) -> Vec<&SandwichOpportunity> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        Vec::new()
    }
    
    pub fn get_token_metrics(&self, token_address: &str) -> Option<&TokenMetrics> {
        // ... existing code ...
        // Kế thừa từ mempool.rs
        None
    }
}

// Include any missing middleware types
pub trait Middleware {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mempool_watcher() {
        // TODO: Implement tests
    }
}

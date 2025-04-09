use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Result, anyhow};
use tracing::{info, warn, error};
use crate::chain_adapters::ChainAdapter;
use ethers::prelude::*;
use ethers::providers::{Http, Provider, Middleware};
use ethers::types::{U256, BlockNumber, TransactionRequest};
use std::collections::VecDeque;
use log::{debug};
use serde::{Serialize, Deserialize};
use crate::chain_adapters::ChainConfig;
use crate::metric::update_gas_metric;
use once_cell::sync::Lazy;
use crate::utils::safe_now;
use ethers::types::{Address};

/// Đánh giá chiến lược tối ưu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedStrategy {
    pub token_address: String,
    pub amount: String,
    pub gas_price: u64,
    pub gas_limit: u64,
    pub slippage: f64,
    pub expected_profit: f64,
}

/// Cấu trúc thông tin gas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasInfo {
    pub gas_price: U256,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub base_fee: U256,
    pub priority_fee: U256,
    pub max_fee: U256,
}

// Thời gian cache (5 phút)
const GAS_CACHE_DURATION: u64 = 300;

// Singleton gas price cache
static GAS_PRICE_CACHE: Lazy<RwLock<HashMap<u64, CachedGasData>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Định nghĩa kiểu dữ liệu cho gas price cache
#[derive(Debug, Clone)]
struct CachedGasData {
    chain_id: u64,
    timestamp: u64,
    gas_price: U256,
    priority_fee: Option<U256>,
    base_fee: Option<U256>,
    network_congestion: NetworkCongestion,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum NetworkCongestion {
    Low,       // Mạng nhẹ
    Medium,    // Mạng trung bình
    High,      // Mạng tắc nghẽn
    VeryHigh,  // Mạng tắc nghẽn nghiêm trọng
}

impl std::fmt::Display for NetworkCongestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkCongestion::Low => write!(f, "Low"),
            NetworkCongestion::Medium => write!(f, "Medium"),
            NetworkCongestion::High => write!(f, "High"),
            NetworkCongestion::VeryHigh => write!(f, "Very High"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasOptimizerConfig {
    pub max_gas_price: U256,               // Giá gas tối đa
    pub max_boost_percent: u64,            // % tăng tối đa
    pub priority_boost_percent: u64,       // % tăng priority fee
    pub sample_interval: u64,              // Thời gian giữa các mẫu (giây)
    pub history_window: u64,               // Kích thước cửa sổ lịch sử (giây)
    pub enable_dynamic_adjustment: bool,   // Bật tính năng tự động điều chỉnh
    pub adaptive_boost_mode: bool,         // Chế độ tăng tương thích
}

impl Default for GasOptimizerConfig {
    fn default() -> Self {
        Self {
            max_gas_price: U256::from(500_000_000_000u64), // 500 Gwei
            max_boost_percent: 50, // 50%
            priority_boost_percent: 20, // 20%
            sample_interval: 30, // 30 giây
            history_window: 3600, // 1 giờ
            enable_dynamic_adjustment: true,
            adaptive_boost_mode: true,
        }
    }
}

pub struct GasOptimizer {
    config: GasOptimizerConfig,
    gas_price_history: VecDeque<(u64, U256)>, // (timestamp, gas_price)
    base_fee_history: VecDeque<(u64, U256)>,  // (timestamp, base_fee)
    network_congestion: NetworkCongestion,
    last_update: u64,
    chain_id: u64,
}

impl GasOptimizer {
    pub fn new(gas_price_limit: U256, max_boost_percent: u64) -> Self {
        Self::with_config(GasOptimizerConfig {
            max_gas_price: gas_price_limit,
            max_boost_percent,
            ..Default::default()
        })
    }
    
    pub fn with_config(config: GasOptimizerConfig) -> Self {
        Self {
            config,
            gas_price_history: VecDeque::new(),
            base_fee_history: VecDeque::new(),
            network_congestion: NetworkCongestion::Medium,
            last_update: 0,
            chain_id: 1, // Mặc định Ethereum
        }
    }
    
    pub fn set_chain_id(&mut self, chain_id: u64) {
        self.chain_id = chain_id;
    }
    
    // Cập nhật lịch sử gas price
    pub async fn update_gas_price_history<M: Middleware>(&mut self, client: Arc<M>) -> Result<()> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        // Nếu lần cập nhật gần nhất < sample_interval, không cập nhật
        if current_time - self.last_update < self.config.sample_interval {
            return Ok(());
        }
        
        // Cập nhật timestamp
        self.last_update = current_time;
        
        // Xóa dữ liệu cũ hơn history_window
        while let Some((time, _)) = self.gas_price_history.front() {
            if current_time - time > self.config.history_window {
                self.gas_price_history.pop_front();
            } else {
                break;
            }
        }
        
        while let Some((time, _)) = self.base_fee_history.front() {
            if current_time - time > self.config.history_window {
                self.base_fee_history.pop_front();
            } else {
                break;
            }
        }
        
        // Lấy gas price hiện tại
        let gas_price = client.get_gas_price().await?;
        
        // Thêm vào lịch sử
        self.gas_price_history.push_back((current_time, gas_price));
        
        // Lấy block gần nhất để có base fee (nếu là EIP-1559)
        if let Ok(block) = client.get_block(BlockNumber::Latest).await {
            if let Some(block) = block {
                if let Some(base_fee) = block.base_fee_per_gas {
                    self.base_fee_history.push_back((current_time, base_fee));
                }
            }
        }
        
        // Cập nhật mức độ tắc nghẽn mạng
        self.update_network_congestion();
        
        // Cập nhật cache
        self.update_gas_price_cache(gas_price);
        
        // Cập nhật metrics
        update_gas_metric(self.chain_id, gas_price.as_u64(), self.network_congestion);
        
        Ok(())
    }
    
    // Cập nhật cache
    fn update_gas_price_cache(&self, gas_price: U256) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        let base_fee = self.base_fee_history.back().map(|(_, fee)| *fee);
        
        let mut priority_fee = None;
        if let Some(base) = base_fee {
            if gas_price > base {
                priority_fee = Some(gas_price - base);
            }
        }
        
        let cache_data = CachedGasData {
            chain_id: self.chain_id,
            timestamp: current_time,
            gas_price,
            priority_fee,
            base_fee,
            network_congestion: self.network_congestion,
        };
        
        let mut cache = GAS_PRICE_CACHE.write().unwrap();
        cache.insert(self.chain_id, cache_data);
    }
    
    // Cập nhật mức độ tắc nghẽn dựa trên lịch sử gas price
    fn update_network_congestion(&mut self) {
        if self.gas_price_history.len() < 2 {
            return;
        }
        
        // Lấy 10 mẫu gần nhất hoặc tất cả nếu ít hơn 10
        let recent_samples = std::cmp::min(10, self.gas_price_history.len());
        
        // Chuyển thành vec để dễ xử lý
        let samples: Vec<(u64, U256)> = self.gas_price_history
            .iter()
            .rev()
            .take(recent_samples)
            .cloned()
            .collect();
        
        // Tính gas price trung bình
        let sum = samples.iter().fold(U256::zero(), |acc, (_, price)| acc + *price);
        let avg_gas_price = sum / U256::from(samples.len());
        
        // Tính độ biến thiên
        let variance_sum = samples.iter()
            .map(|(_, price)| {
                if *price > avg_gas_price {
                    *price - avg_gas_price
                } else {
                    avg_gas_price - *price
                }
            })
            .fold(U256::zero(), |acc, val| acc + val);
        
        let variance = variance_sum / U256::from(samples.len());
        let variance_percent = if avg_gas_price > U256::zero() {
            variance * U256::from(100) / avg_gas_price
        } else {
            U256::zero()
        };
        
        // Tính tốc độ tăng trong 5 mẫu gần nhất
        let growth_rate = if samples.len() >= 5 {
            let newest = samples[0].1;
            let oldest = samples[4].1;
            
            if oldest > U256::zero() {
                // Nếu newest > oldest, tính % tăng
                if newest > oldest {
                    ((newest - oldest) * U256::from(100)) / oldest
                } else {
                    U256::zero()
                }
            } else {
                U256::zero()
            }
        } else {
            U256::zero()
        };
        
        // Xác định mức độ tắc nghẽn dựa trên độ biến thiên và tốc độ tăng
        let old_congestion = self.network_congestion;
        
        if growth_rate > U256::from(20) || variance_percent > U256::from(30) {
            // Tăng nhanh > 20% hoặc biến động > 30%
            self.network_congestion = NetworkCongestion::VeryHigh;
        } else if growth_rate > U256::from(10) || variance_percent > U256::from(15) {
            // Tăng nhanh > 10% hoặc biến động > 15%
            self.network_congestion = NetworkCongestion::High;
        } else if growth_rate > U256::from(5) || variance_percent > U256::from(5) {
            // Tăng nhanh > 5% hoặc biến động > 5%
            self.network_congestion = NetworkCongestion::Medium;
        } else {
            // Ổn định
            self.network_congestion = NetworkCongestion::Low;
        }
        
        // Ghi log nếu mức độ tắc nghẽn thay đổi
        if old_congestion != self.network_congestion {
            info!(
                "Network congestion changed: {:?} -> {:?}, avg gas price: {}, variance: {}%, growth rate: {}%",
                old_congestion, self.network_congestion, avg_gas_price, variance_percent, growth_rate
            );
        }
    }
    
    // Lấy gas price từ cache hoặc cập nhật nếu cần
    async fn get_cached_gas_data<A: ChainAdapter>(&self, adapter: &A) -> Result<CachedGasData> {
        let chain_id = adapter.get_config().chain_id;
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        // Kiểm tra cache
        {
            let cache = GAS_PRICE_CACHE.read().unwrap();
            if let Some(data) = cache.get(&chain_id) {
                // Nếu cache còn mới (< GAS_CACHE_DURATION giây)
                if current_time - data.timestamp < GAS_CACHE_DURATION {
                    return Ok(data.clone());
                }
            }
        }
        
        // Nếu không có cache hoặc cache quá cũ, lấy mới
        let provider = adapter.get_provider();
        let gas_price = provider.get_gas_price().await?;
        
        let mut base_fee = None;
        let mut priority_fee = None;
        
        // Lấy thông tin EIP-1559 nếu chain hỗ trợ
        if adapter.get_config().eip1559_supported {
            if let Ok(block) = provider.get_block(BlockNumber::Latest).await {
                if let Some(block) = block {
                    base_fee = block.base_fee_per_gas;
                    
                    // Tính priority fee nếu có base fee
                    if let Some(base) = base_fee {
                        if gas_price > base {
                            priority_fee = Some(gas_price - base);
                        }
                    }
                }
            }
        }
        
        // Tạo dữ liệu mới
        let data = CachedGasData {
            chain_id,
            timestamp: current_time,
            gas_price,
            priority_fee,
            base_fee,
            network_congestion: self.network_congestion,
        };
        
        // Cập nhật cache
        {
            let mut cache = GAS_PRICE_CACHE.write().unwrap();
            cache.insert(chain_id, data.clone());
        }
        
        Ok(data)
    }
    
    // Đề xuất giá gas tối ưu
    pub async fn get_optimal_gas_price<A: ChainAdapter>(&self, adapter: &A) -> Result<U256> {
        if adapter.get_config().eip1559_supported {
            let (max_fee, _) = self.get_optimal_eip1559_fees(adapter).await?;
            Ok(max_fee)
        } else {
            // Legacy gas price
            let provider = adapter.get_provider();
            let client = Arc::new(provider.clone());
            
            // Nếu gas_price_history rỗng, chỉ sử dụng giá gas hiện tại thay vì cập nhật history
            let current_gas_price = provider.get_gas_price().await?;
            
            // Tính tỷ lệ phần trăm tăng dựa trên mức độ tắc nghẽn
            let congestion = self.network_congestion;
            let percentage_increase = match congestion {
                NetworkCongestion::Low => 1.05, // +5%
                NetworkCongestion::Medium => 1.10, // +10%
                NetworkCongestion::High => 1.20, // +20%
                NetworkCongestion::VeryHigh => 1.30, // +30%
            };
            
            // Tính gas price tối ưu
            let multiplier = (percentage_increase * 1000.0) as u64;
            let optimal_gas = (current_gas_price.as_u128() * multiplier as u128) / 1000;
            
            Ok(U256::from(optimal_gas))
        }
    }
    
    // Đề xuất phí EIP-1559 tối ưu (với chains hỗ trợ)
    pub async fn get_optimal_eip1559_fees<A: ChainAdapter>(&self, adapter: &A) -> Result<(U256, U256)> {
        let provider = adapter.get_provider();
        let chain_id = adapter.get_config().chain_id;
        
        // Lấy dữ liệu gas từ cache hoặc cập nhật
        let gas_data = self.get_cached_gas_data(adapter).await?;
        
        // Lấy ước tính phí từ mạng
        let fee_history = provider.fee_history(5, BlockNumber::Latest, &[10.0, 50.0, 90.0]).await?;
        
        // Tính toán các priority fee percentiles
        let mut priority_fees = Vec::new();
        for rewards in fee_history.reward.iter() {
            if let Some(reward) = rewards.get(1) { // lấy percentile 50.0
                priority_fees.push(*reward);
            }
        }
        
        // Tính toán priority fee trung bình
        let priority_fee = if !priority_fees.is_empty() {
            let sum = priority_fees.iter().fold(U256::zero(), |acc, &x| acc + x);
            sum / U256::from(priority_fees.len())
        } else if let Some(pf) = gas_data.priority_fee {
            pf
        } else {
            U256::from(1_500_000_000) // 1.5 Gwei mặc định
        };
        
        // Điều chỉnh priority fee dựa trên mức độ tắc nghẽn
        let boost_factor = match gas_data.network_congestion {
            NetworkCongestion::Low => 100, // +0%
            NetworkCongestion::Medium => 120, // +20%
            NetworkCongestion::High => 150, // +50%
            NetworkCongestion::VeryHigh => 200, // +100%
        };
        
        // Giới hạn boost không vượt quá max_boost_percent
        let boost_factor = std::cmp::min(boost_factor, 100 + self.config.priority_boost_percent);
        
        let boosted_priority_fee = priority_fee * U256::from(boost_factor) / U256::from(100);
        
        // Tính toán max fee dựa trên base fee gần đây
        let base_fees: Vec<U256> = fee_history.base_fee_per_gas.clone();
        let max_fee = if !base_fees.is_empty() || gas_data.base_fee.is_some() {
            let latest_base_fee = base_fees.last()
                .cloned()
                .unwrap_or_else(|| gas_data.base_fee.unwrap_or_default());
                
            let base_fee_multiplier = match gas_data.network_congestion {
                NetworkCongestion::Low => 120, // +20%
                NetworkCongestion::Medium => 130, // +30%
                NetworkCongestion::High => 150, // +50%
                NetworkCongestion::VeryHigh => 200, // +100%
            };
            
            latest_base_fee * U256::from(base_fee_multiplier) / U256::from(100) + boosted_priority_fee
        } else {
            // Nếu không có dữ liệu base fee, sử dụng gas price hiện tại
            gas_data.gas_price
        };
        
        // Đảm bảo max fee không vượt quá giới hạn
        let capped_max_fee = if max_fee > self.config.max_gas_price {
            self.config.max_gas_price
        } else {
            max_fee
        };
        
        debug!("Tối ưu EIP-1559 fees: priority_fee={}, max_fee={} (congestion={:?})", 
              boosted_priority_fee, capped_max_fee, gas_data.network_congestion);
              
        Ok((boosted_priority_fee, capped_max_fee))
    }
    
    // Hàm tiện ích để tối ưu transaction request với giá gas phù hợp
    pub async fn optimize_transaction<A: ChainAdapter>(&self, adapter: &A, mut tx: TransactionRequest) -> Result<TransactionRequest> {
        // Kiểm tra nếu EIP-1559 được hỗ trợ
        if adapter.get_config().eip1559_supported {
            let (priority_fee, max_fee) = self.get_optimal_eip1559_fees(adapter).await?;
            
            // Thiết lập max_priority_fee_per_gas và max_fee_per_gas
            tx = tx.max_priority_fee_per_gas(priority_fee)
                   .max_fee_per_gas(max_fee);
        } else {
            // Đối với giao dịch legacy, thiết lập gas_price
            let gas_price = self.get_optimal_gas_price(adapter).await?;
            tx = tx.gas_price(gas_price);
        }
        
        Ok(tx)
    }
    
    // Tối ưu gas cho retry: mỗi lần retry sẽ tăng gas lên để đảm bảo giao dịch được xử lý
    pub fn get_optimized_gas_price(&self, retry_count: u32, current_gas_price: Option<U256>) -> U256 {
        let base_price = if let Some(price) = current_gas_price {
            price
        } else {
            // Nếu không có giá hiện tại, lấy giá gần nhất trong history
            match self.gas_price_history.back() {
                Some((_, price)) => *price,
                None => U256::from(50_000_000_000u64), // 50 Gwei default
            }
        };
        
        if retry_count == 0 {
            return base_price;
        }
        
        // Tăng theo số lần retry
        // Chiến lược: retry 1: +20%, retry 2: +50%, retry 3: +100%, retry 4+: +200%
        let increase_percent = match retry_count {
            1 => 20,
            2 => 50,
            3 => 100,
            _ => 200,
        };
        
        let new_price = base_price * U256::from(100 + increase_percent) / U256::from(100);
        
        // Đảm bảo không vượt quá giới hạn
        if new_price > self.config.max_gas_price {
            warn!("Retry gas price {} vượt quá giới hạn {}, sử dụng giới hạn", 
                new_price, self.config.max_gas_price);
            self.config.max_gas_price
        } else {
            info!("Tăng gas price do retry ({}) từ {} lên {} (+{}%)", 
                retry_count, base_price, new_price, increase_percent);
            new_price
        }
    }
    
    // Tối ưu gas limit
    pub fn get_optimized_gas_limit(&self, base_gas_limit: u64, retry_count: u32) -> u64 {
        if retry_count == 0 {
            return base_gas_limit;
        }
        
        // Tăng gas limit theo số lần retry
        // Chiến lược: retry 1: +10%, retry 2: +20%, retry 3: +30%, retry 4+: +50%
        let increase_percent = match retry_count {
            1 => 10,
            2 => 20,
            3 => 30,
            _ => 50,
        };
        
        let new_limit = (base_gas_limit as f64 * (1.0 + increase_percent as f64 / 100.0)) as u64;
        
        info!("Tăng gas limit do retry ({}) từ {} lên {} (+{}%)", 
            retry_count, base_gas_limit, new_limit, increase_percent);
            
        new_limit
    }
    
    // Thiết lập mức độ tắc nghẽn thủ công
    pub fn set_network_congestion(&mut self, congestion: NetworkCongestion) {
        self.network_congestion = congestion;
    }
    
    // Thiết lập sample interval
    pub fn set_sample_interval(&mut self, interval: u64) {
        self.config.sample_interval = interval;
    }
    
    // Thiết lập gas price limit
    pub fn set_gas_price_limit(&mut self, limit: U256) {
        self.config.max_gas_price = limit;
    }
    
    // Thiết lập max boost percent
    pub fn set_max_boost_percent(&mut self, percent: u64) {
        self.config.max_boost_percent = percent;
    }
    
    // Lấy thông tin tắc nghẽn mạng
    pub fn get_network_congestion(&self) -> NetworkCongestion {
        self.network_congestion
    }
    
    // Lấy gas price trung bình hiện tại
    pub fn get_average_gas_price(&self) -> Option<U256> {
        if self.gas_price_history.is_empty() {
            return None;
        }
        
        let sum: U256 = self.gas_price_history.iter().map(|(_, price)| *price).sum();
        Some(sum / U256::from(self.gas_price_history.len()))
    }
    
    // Lấy base fee trung bình hiện tại
    pub fn get_average_base_fee(&self) -> Option<U256> {
        if self.base_fee_history.is_empty() {
            return None;
        }
        
        let sum: U256 = self.base_fee_history.iter().map(|(_, fee)| *fee).sum();
        Some(sum / U256::from(self.base_fee_history.len()))
    }
    
    // Phân tích xu hướng gas price
    pub fn analyze_gas_trend(&self) -> Option<(f64, String)> {
        if self.gas_price_history.len() < 5 {
            return None;
        }
        
        // Lấy 5 mẫu gần nhất
        let samples: Vec<(u64, U256)> = self.gas_price_history
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect();
            
        let newest = samples[0].1;
        let oldest = samples[4].1;
        
        if oldest > U256::zero() {
            let change_pct = if newest > oldest {
                let diff = newest - oldest;
                let pct = (diff.as_u128() as f64 * 100.0) / oldest.as_u128() as f64;
                (pct, format!("Tăng {:.2}%", pct))
            } else if newest < oldest {
                let diff = oldest - newest;
                let pct = (diff.as_u128() as f64 * 100.0) / oldest.as_u128() as f64;
                (-pct, format!("Giảm {:.2}%", pct))
            } else {
                (0.0, "Ổn định".to_string())
            };
            
            Some(change_pct)
        } else {
            None
        }
    }
}

// Utility function to get current gas price with cache
pub async fn get_current_gas_price<A: ChainAdapter>(adapter: &A) -> Result<U256> {
    let chain_id = adapter.get_config().chain_id;
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
        
    // Kiểm tra cache
    {
        let cache = GAS_PRICE_CACHE.read().unwrap();
        if let Some(data) = cache.get(&chain_id) {
            // Nếu cache còn mới (< GAS_CACHE_DURATION giây)
            if current_time - data.timestamp < GAS_CACHE_DURATION {
                return Ok(data.gas_price);
            }
        }
    }
    
    // Nếu không có cache hoặc cache quá cũ, lấy mới
    let provider = adapter.get_provider();
    let gas_price = provider.get_gas_price().await?;
    
    // Cập nhật cache
    let data = CachedGasData {
        chain_id,
        timestamp: current_time,
        gas_price,
        priority_fee: None,
        base_fee: None,
        network_congestion: NetworkCongestion::Medium,
    };
    
    {
        let mut cache = GAS_PRICE_CACHE.write().unwrap();
        cache.insert(chain_id, data);
    }
    
    Ok(gas_price)
}

// Utility function to cleanup expired cache entries
pub fn cleanup_gas_cache() {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
        
    let mut cache = GAS_PRICE_CACHE.write().unwrap();
    
    // Remove entries older than 2x cache duration
    cache.retain(|_, data| current_time - data.timestamp < GAS_CACHE_DURATION * 2);
}

// Thêm hàm sumU256 để tính tổng của Vec<U256>
fn sumU256(values: &[U256]) -> U256 {
    let mut sum = U256::zero();
    for v in values {
        sum = sum.saturating_add(*v);
    }
    sum
}

// Thêm hàm tính trung bình U256
fn avgU256(values: &[U256]) -> U256 {
    if values.is_empty() {
        return U256::zero();
    }
    let sum = sumU256(values);
    sum / U256::from(values.len())
}

pub async fn calculate_priority_fee(&self, provider: Arc<Provider<Http>>) -> Result<U256, Error> {
    // Kiểm tra cache
    let cache_key = provider.url().to_string();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    
    // Kiểm tra trong bộ nhớ đệm
    if let Some(cached) = self.cache.read().await.get(&cache_key) {
        if now - cached.timestamp < 60 { // Cache valid for 1 minute
            return Ok(cached.value);
        }
    }
    
    // Lấy 10 block gần nhất
    let latest_block = provider.get_block_number().await?;
    let mut priority_fees = Vec::new();
    
    for i in 0..10 {
        if latest_block <= i.into() {
            break;
        }
        
        let block_number = latest_block - i;
        if let Some(block) = provider.get_block(block_number).await? {
            if let Some(base_fee) = block.base_fee_per_gas {
                // Xử lý các giao dịch trong block
                for tx_hash in block.transactions {
                    if let Some(tx) = provider.get_transaction(tx_hash).await? {
                        if let (Some(max_fee), Some(priority_fee)) = (tx.max_fee_per_gas, tx.max_priority_fee_per_gas) {
                            // Tính toán priority fee thực tế được trả
                            let effective_priority = if base_fee >= max_fee {
                                U256::zero()
                            } else {
                                let available = max_fee - base_fee;
                                if available < priority_fee {
                                    available
                                } else {
                                    priority_fee
                                }
                            };
                            
                            if !effective_priority.is_zero() {
                                priority_fees.push(effective_priority);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Tính priority fee trung bình
    let result = if priority_fees.is_empty() {
        // Giá trị mặc định nếu không có dữ liệu
        U256::from(1_000_000_000) // 1 gwei
    } else {
        // Sử dụng hàm trợ giúp để tính tổng
        let sum = sumU256(&priority_fees);
        sum / U256::from(priority_fees.len())
    };
    
    // Lưu vào cache
    self.cache.write().await.insert(cache_key, CacheEntry {
        value: result,
        timestamp: now,
    });
    
    Ok(result)
}

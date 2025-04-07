// Standard library imports
use std::{
    time::{Duration, Instant},
    fmt::Debug,
    sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}},
    collections::HashMap,
    error::Error,
};

// Internal imports
use crate::{
    chain_adapters::interfaces::ChainError,
    metrics::{RetryMetrics, RETRY_METRICS},
};

// Third party imports
use anyhow::{Result, anyhow};
use tracing::{info, warn, debug};
use ethers::types::U256;
use serde::{Serialize, Deserialize};
use metrics::{counter, gauge};
use async_trait::async_trait;
use backoff::backoff::Backoff;

/// Trạng thái circuit breaker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitBreakerState {
    /// Trạng thái bình thường, cho phép request
    Closed,
    /// Trạng thái mở, không cho phép request
    Open,
    /// Trạng thái bán mở, cho phép một số request
    HalfOpen,
}

/// Cấu hình cho policy retry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicyConfig {
    /// Thời gian cơ bản giữa các lần retry (ms)
    pub base_retry_interval: u64,
    /// Hệ số nhân cho mỗi lần retry
    pub backoff_factor: f64,
    /// Số lần retry tối đa
    pub max_retries: usize,
    /// Thời gian tối đa cho tất cả các retry (ms)
    pub max_total_retry_time: u64,
    /// Thời gian jitter tối đa (ms)
    pub max_jitter: u64,
    /// Danh sách lỗi có thể thử lại
    pub retryable_errors: Vec<String>,
    /// Cấu hình circuit breaker
    pub circuit_breaker: CircuitBreakerConfig,
}

/// Cấu hình cho circuit breaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Số lỗi để mở circuit
    pub error_threshold: u32,
    /// Khoảng thời gian để đếm lỗi (ms)
    pub error_window: u64,
    /// Thời gian mở circuit (ms)
    pub open_duration: u64,
    /// Số request cho phép khi ở trạng thái half open
    pub half_open_allowed_requests: u32,
}

/// Cấu trúc thông tin cho phép tăng gas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasBoostConfig {
    /// Kích hoạt tự động tăng gas
    pub enabled: bool,
    /// Hệ số nhân tối đa
    pub max_boost_factor: f64,
    /// Hệ số nhân ban đầu
    pub initial_boost_factor: f64,
    /// Hệ số tăng cho mỗi lần retry
    pub step_factor: f64,
}

impl Default for RetryPolicyConfig {
    fn default() -> Self {
        Self {
            base_retry_interval: 1000,  // 1s
            backoff_factor: 2.0,
            max_retries: 3,
            max_total_retry_time: 30000,  // 30s
            max_jitter: 500,  // 500ms
            retryable_errors: vec![
                "connection".to_string(),
                "timeout".to_string(),
                "rate limit".to_string(),
                "gas too low".to_string(),
                "nonce".to_string(),
            ],
            circuit_breaker: CircuitBreakerConfig {
                error_threshold: 5,
                error_window: 60000,  // 1 phút
                open_duration: 300000,  // 5 phút
                half_open_allowed_requests: 3,
            },
        }
    }
}

impl Default for GasBoostConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_boost_factor: 3.0,
            initial_boost_factor: 1.2,
            step_factor: 0.2,
        }
    }
}

/// Struct quản lý circuit breaker cho một endpoint
#[derive(Debug)]
pub struct CircuitBreaker {
    state: RwLock<CircuitBreakerState>,
    error_count: AtomicU64,
    last_error_time: RwLock<Instant>,
    last_state_change: RwLock<Instant>,
    half_open_requests: AtomicU64,
    config: CircuitBreakerConfig,
    endpoint_id: String,
}

impl CircuitBreaker {
    /// Tạo mới một circuit breaker
    pub fn new(endpoint_id: &str, config: CircuitBreakerConfig) -> Self {
        let now = Instant::now();
        Self {
            state: RwLock::new(CircuitBreakerState::Closed),
            error_count: AtomicU64::new(0),
            last_error_time: RwLock::new(now),
            last_state_change: RwLock::new(now),
            half_open_requests: AtomicU64::new(0),
            config,
            endpoint_id: endpoint_id.to_string(),
        }
    }
    
    /// Kiểm tra xem request có thể thực hiện không
    pub fn can_execute(&self) -> bool {
        match self.state.try_read() {
            Ok(state) => {
                let state = *state;
                
                match state {
                    CircuitBreakerState::Closed => true,
                    CircuitBreakerState::Open => {
                        // Kiểm tra xem đã đến thời gian thử lại chưa
                        match self.last_state_change.try_read() {
                            Ok(last_change) => {
                                let elapsed = last_change.elapsed();
                                if elapsed >= self.config.open_duration {
                                    // Chuyển sang trạng thái half-open
                                    match self.state.try_write() {
                                        Ok(mut state) => {
                                            *state = CircuitBreakerState::HalfOpen;
                                            match self.last_state_change.try_write() {
                                                Ok(mut time) => {
                                                    *time = Instant::now();
                                                    self.half_open_requests.store(0, Ordering::SeqCst);
                                                    debug!("Circuit breaker for {} changed to half-open state", self.endpoint_id);
                                                    true
                                                },
                                                Err(_) => {
                                                    warn!("Không thể lấy write lock cho last_state_change trong can_execute");
                                                    false
                                                }
                                            }
                                        },
                                        Err(_) => {
                                            warn!("Không thể lấy write lock cho state trong can_execute");
                                            false
                                        }
                                    }
                                } else {
                                    false
                                }
                            },
                            Err(_) => {
                                warn!("Không thể lấy read lock cho last_state_change trong can_execute");
                                false
                            }
                        }
                    },
                    CircuitBreakerState::HalfOpen => {
                        // Chỉ cho phép một số request nhất định
                        let current = self.half_open_requests.fetch_add(1, Ordering::SeqCst);
                        current < self.config.half_open_allowed_requests as u64
                    }
                }
            },
            Err(_) => {
                warn!("Không thể lấy read lock cho state trong can_execute");
                false // Mặc định không cho phép nếu không thể lấy trạng thái
            }
        }
    }
    
    /// Báo cáo lỗi
    pub fn record_failure(&self) {
        // Cập nhật thời gian lỗi gần nhất
        match self.last_error_time.try_write() {
            Ok(mut error_time) => {
                *error_time = Instant::now();
            },
            Err(_) => {
                warn!("Không thể lấy write lock cho last_error_time");
                // Tiếp tục xử lý mặc dù không cập nhật được thời gian
            }
        }
        
        // Tăng số lỗi
        let current_count = self.error_count.fetch_add(1, Ordering::SeqCst);
        
        // Lấy trạng thái hiện tại
        let state = match self.state.try_read() {
            Ok(state) => *state,
            Err(_) => {
                warn!("Không thể lấy read lock cho circuit breaker state");
                return; // Không thể xử lý thêm
            }
        };
        
        match state {
            CircuitBreakerState::Closed => {
                // Kiểm tra ngưỡng lỗi
                if current_count + 1 >= self.config.error_threshold as u64 {
                    // Mở circuit
                    match self.state.try_write() {
                        Ok(mut state) => {
                            *state = CircuitBreakerState::Open;
                            match self.last_state_change.try_write() {
                                Ok(mut time) => {
                                    *time = Instant::now();
                                },
                                Err(_) => {
                                    warn!("Không thể lấy write lock cho last_state_change khi mở circuit");
                                }
                            }
                            warn!("Circuit breaker for {} opened due to error threshold reached", self.endpoint_id);
                            
                            // Cập nhật metrics
                            counter!("circuit_breaker_trips", 1, "endpoint" => self.endpoint_id.clone());
                        },
                        Err(_) => {
                            warn!("Không thể lấy write lock cho circuit breaker state khi mở circuit");
                        }
                    }
                }
            },
            CircuitBreakerState::HalfOpen => {
                // Lỗi khi đang ở half-open, trở lại trạng thái open
                match self.state.try_write() {
                    Ok(mut state) => {
                        *state = CircuitBreakerState::Open;
                        match self.last_state_change.try_write() {
                            Ok(mut time) => {
                                *time = Instant::now();
                            },
                            Err(_) => {
                                warn!("Không thể lấy write lock cho last_state_change khi quay lại open");
                            }
                        }
                        warn!("Circuit breaker for {} reopened due to failure in half-open state", self.endpoint_id);
                        
                        // Cập nhật metrics
                        counter!("circuit_breaker_half_open_failures", 1, "endpoint" => self.endpoint_id.clone());
                    },
                    Err(_) => {
                        warn!("Không thể lấy write lock cho circuit breaker state khi quay lại open");
                    }
                }
            },
            CircuitBreakerState::Open => {
                // Đã mở, không cần làm gì thêm
            }
        }
    }
    
    /// Báo cáo thành công
    pub fn record_success(&self) {
        let state = *self.state.read().unwrap();
        
        if state == CircuitBreakerState::HalfOpen {
            // Kiểm tra số lượng request thành công đã đủ chưa
            let current = self.half_open_requests.load(Ordering::SeqCst);
            if current >= self.config.half_open_allowed_requests as u64 {
                // Đủ số request thành công, đóng circuit
                let mut state = self.state.write().unwrap();
                *state = CircuitBreakerState::Closed;
                *self.last_state_change.write().unwrap() = Instant::now();
                self.error_count.store(0, Ordering::SeqCst);
                info!("Circuit breaker for {} closed after successful half-open state", self.endpoint_id);
                
                // Cập nhật metrics
                counter!("circuit_breaker_resets", 1, "endpoint" => self.endpoint_id.clone());
            }
        }
    }
    
    /// Reset circuit breaker
    pub fn reset(&self) {
        let mut state = self.state.write().unwrap();
        *state = CircuitBreakerState::Closed;
        *self.last_state_change.write().unwrap() = Instant::now();
        self.error_count.store(0, Ordering::SeqCst);
        info!("Circuit breaker for {} manually reset", self.endpoint_id);
    }
    
    /// Lấy trạng thái hiện tại
    pub fn get_state(&self) -> CircuitBreakerState {
        *self.state.read().unwrap()
    }
}

/// Trait định nghĩa chính sách retry
#[async_trait]
pub trait RetryPolicy: Send + Sync + 'static {
    /// Kiểm tra xem có nên retry không
    fn should_retry(&self, attempt: u32, error: &anyhow::Error) -> bool;
    
    /// Lấy thời gian chờ trước khi retry
    fn get_backoff_duration(&self, attempt: u32) -> Duration;
    
    /// Thực hiện retry một hàm async
    async fn retry_async<F, T>(&self, f: F) -> Result<T> 
    where
        F: Fn() -> Result<T> + Send + Sync,
        T: Send;
        
    /// Lấy số lần retry tối đa
    fn get_max_retries(&self) -> u32;
}

/// Enum bọc các loại RetryPolicy để tránh dùng trait object
#[derive(Debug, Clone)]
pub enum RetryPolicyEnum {
    Exponential(Arc<ExponentialRetryPolicy>),
    // Có thể thêm các loại khác nếu cần
}

impl RetryPolicy for RetryPolicyEnum {
    fn should_retry(&self, attempt: u32, error: &anyhow::Error) -> bool {
        match self {
            RetryPolicyEnum::Exponential(policy) => policy.should_retry(attempt, error)
        }
    }
    
    fn get_backoff_duration(&self, attempt: u32) -> Duration {
        match self {
            RetryPolicyEnum::Exponential(policy) => policy.get_backoff_duration(attempt)
        }
    }
    
    async fn retry_async<F, T>(&self, f: F) -> Result<T> 
    where
        F: Fn() -> Result<T> + Send + Sync,
        T: Send
    {
        match self {
            RetryPolicyEnum::Exponential(policy) => policy.retry_async(f)
        }
    }
    
    fn get_max_retries(&self) -> u32 {
        match self {
            RetryPolicyEnum::Exponential(policy) => policy.get_max_retries()
        }
    }
}

/// Cấu trúc context cho retry
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Tên thao tác đang thực hiện
    pub operation_name: String,
    /// Endpoint đang sử dụng
    pub endpoint: String,
    /// Chain id
    pub chain_id: u64,
    /// Gas price hiện tại
    pub current_gas_price: Option<U256>,
    /// Thời gian tổng cộng đã retry
    pub total_retry_time: Duration,
    /// Thời điểm bắt đầu retry
    pub start_time: Instant,
    /// Số lần đã retry
    pub retry_count: usize,
}

impl RetryContext {
    /// Tạo context mới
    pub fn new(operation_name: &str, endpoint: &str, chain_id: u64, current_gas_price: Option<U256>) -> Self {
        Self {
            operation_name: operation_name.to_string(),
            endpoint: endpoint.to_string(),
            chain_id,
            current_gas_price,
            total_retry_time: Duration::from_millis(0),
            start_time: Instant::now(),
            retry_count: 0,
        }
    }
}

/// Triển khai mặc định cho retry policy
#[derive(Debug)]
pub struct ExponentialRetryPolicy {
    config: RetryPolicyConfig,
    gas_boost_config: GasBoostConfig,
    circuit_breakers: RwLock<HashMap<String, Arc<CircuitBreaker>>>,
}

impl ExponentialRetryPolicy {
    /// Tạo policy retry mới
    pub fn new(config: RetryPolicyConfig, gas_boost_config: GasBoostConfig) -> Self {
        Self {
            config,
            gas_boost_config,
            circuit_breakers: RwLock::new(HashMap::new()),
        }
    }
    
    /// Lấy circuit breaker cho endpoint
    fn get_circuit_breaker(&self, endpoint: &str) -> Arc<CircuitBreaker> {
        let mut breakers = self.circuit_breakers.write().unwrap();
        
        breakers.entry(endpoint.to_string()).or_insert_with(|| {
            Arc::new(CircuitBreaker::new(endpoint, self.config.circuit_breaker.clone()))
        }).clone()
    }
}

#[async_trait]
impl RetryPolicy for ExponentialRetryPolicy {
    fn should_retry(&self, attempt: u32, error: &anyhow::Error) -> bool {
        let config = self.config.clone();
        let error_str = error.to_string();
        Box::pin(async move {
            // Phân tích lỗi để quyết định có retry không
            error_str.contains("timeout") || 
            error_str.contains("connection refused") ||
            error_str.contains("too many requests") ||
            error_str.contains("rate limit") ||
            error_str.contains("server error") ||
            error_str.contains("network error") ||
            attempt < 3  // Luôn retry trong 3 lần đầu
        }).await.unwrap_or(false)
    }
    
    fn get_backoff_duration(&self, attempt: u32) -> Duration {
        let config = self.config.clone();
        Box::pin(async move {
            // Tính toán thời gian chờ theo cấp số nhân với jitter
            let base_delay = config.initial_backoff_ms;
            let max_delay = config.max_backoff_ms;
            let multiplier = config.backoff_multiplier;
            
            // Công thức: base_delay * (multiplier ^ attempt)
            let delay_ms = base_delay * multiplier.powi(attempt as i32);
            let delay_ms = delay_ms.min(max_delay as f64) as u64;
            
            // Thêm jitter (dao động ngẫu nhiên ±20%)
            let jitter_range = (delay_ms as f64 * 0.2) as u64;
            let mut rng = rand::thread_rng();
            let jitter = rng.gen_range(0..=jitter_range*2) as i64 - jitter_range as i64;
            
            let final_delay = std::cmp::max(10, delay_ms as i64 + jitter) as u64;
            Duration::from_millis(final_delay)
        }).await
    }
    
    async fn retry_async<F, T>(&self, f: F) -> Result<T> 
    where
        F: Fn() -> Result<T> + Send + Sync,
        T: Send
    {
        let config = self.config.clone();
        let gas_boost_config = self.gas_boost_config.clone();
        let endpoint = context.endpoint.clone();
        let circuit_breaker = self.get_circuit_breaker(&endpoint);
        let operation_name = context.operation_name.clone();

        Box::pin(async move {
            let mut current_retry = 0;
            let mut total_time = Duration::from_millis(0);
            let start_time = Instant::now();
            let mut last_error = None;
            let mut current_gas = context.current_gas_price;
            
            // Kiểm tra circuit breaker
            if !circuit_breaker.can_execute() {
                return Err(anyhow!("Circuit breaker is open for endpoint: {}", endpoint));
            }
            
            loop {
                // Kiểm tra số lần retry
                if current_retry >= config.max_retries {
                    warn!("Max retries ({}) reached for operation: {}", 
                        config.max_retries, operation_name);
                    
                    // Cập nhật metrics
                    counter!("retry_exhausted", 1, 
                            "operation" => operation_name.clone(),
                            "endpoint" => endpoint.clone());
                    
                    return Err(last_error.unwrap_or_else(|| anyhow!("Max retries reached")));
                }
                
                // Kiểm tra thời gian tổng cộng
                if total_time.as_millis() as u64 >= config.max_total_retry_time {
                    warn!("Max total retry time ({} ms) reached for operation: {}", 
                        config.max_total_retry_time, operation_name);
                    
                    // Cập nhật metrics
                    counter!("retry_timeout", 1, 
                            "operation" => operation_name.clone(),
                            "endpoint" => endpoint.clone());
                    
                    return Err(last_error.unwrap_or_else(|| anyhow!("Max retry time exceeded")));
                }
                
                // Thực hiện operation
                let result = f().await;
                
                match result {
                    Ok(value) => {
                        // Cập nhật metrics nếu đã retry
                        if current_retry > 0 {
                            counter!("retry_success", 1, 
                                    "operation" => operation_name.clone(),
                                    "endpoint" => endpoint.clone(),
                                    "retry_count" => current_retry.to_string());
                            
                            // Ghi log thành công sau khi retry
                            info!("Operation {} succeeded after {} retries in {} ms", 
                                  operation_name, current_retry, total_time.as_millis());
                            
                            // Đánh dấu circuit breaker thành công
                            circuit_breaker.record_success();
                        }
                        
                        return Ok(value);
                    }
                    Err(err) => {
                        // Lưu lỗi cuối cùng
                        last_error = Some(err.clone());
                        
                        // Ghi log lỗi
                        warn!("Operation {} failed (attempt {}): {}", 
                              operation_name, current_retry + 1, err);
                        
                        // Ghi nhận thất bại cho circuit breaker
                        circuit_breaker.record_failure();
                        
                        // Kiểm tra circuit breaker
                        if !circuit_breaker.can_execute() {
                            warn!("Circuit breaker opened for endpoint: {}", endpoint);
                            return Err(anyhow!("Circuit breaker opened during retry"));
                        }
                        
                        // Điều chỉnh gas price nếu cần
                        let chain_err = match err.downcast_ref::<ChainError>() {
                            Some(chain_err) => {
                                // Có thể điều chỉnh gas price
                                if let Some(new_gas) = self.adjust_gas_price(chain_err, current_gas) {
                                    debug!("Adjusting gas price from {:?} to {:?}", current_gas, new_gas);
                                    current_gas = Some(new_gas);
                                }
                                Some(chain_err)
                            },
                            None => None
                        };
                        
                        // Kiểm tra xem lỗi có thể retry không
                        let retryable = match chain_err {
                            Some(chain_err) => self.is_retryable(chain_err),
                            None => false
                        };
                        
                        if !retryable {
                            // Cập nhật metrics
                            counter!("retry_non_retryable", 1, 
                                    "operation" => operation_name.clone(),
                                    "endpoint" => endpoint.clone());
                            
                            warn!("Non-retryable error encountered for operation {}: {}", 
                                  operation_name, err);
                            
                            return Err(err);
                        }
                        
                        // Tính thời gian delay
                        let delay = self.get_backoff_duration(current_retry);
                        
                        // Cập nhật metrics
                        counter!("retry_attempt", 1, 
                                "operation" => operation_name.clone(),
                                "endpoint" => endpoint.clone(),
                                "retry_count" => current_retry.to_string());
                        
                        // Ghi log retry
                        debug!("Retrying operation {} in {} ms (attempt {}/{})", 
                              operation_name, delay.as_millis(), current_retry + 1, config.max_retries);
                        
                        // Delay trước khi thử lại
                        tokio::time::sleep(delay).await;
                        
                        // Cập nhật thời gian và số lần retry
                        current_retry += 1;
                        total_time = start_time.elapsed();
                    }
                }
            }
        })
    }
    
    fn get_max_retries(&self) -> u32 {
        self.config.max_retries as u32
    }
}

/// Sửa thành phương thức trả về RetryPolicyEnum
pub fn create_default_retry_policy() -> RetryPolicyEnum {
    RetryPolicyEnum::Exponential(Arc::new(ExponentialRetryPolicy::new(
        RetryPolicyConfig::default(),
        GasBoostConfig::default(),
    )))
}

/// RetryStats để thu thập thông tin về retry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStats {
    /// Số lần retry thành công
    pub successful_retries: u64,
    /// Số lần retry thất bại
    pub failed_retries: u64,
    /// Thời gian trung bình giữa các lần retry (ms)
    pub avg_retry_delay_ms: f64,
    /// Số lượng endpoint có circuit breaker mở
    pub open_circuit_breakers: u64,
    /// Danh sách endpoint đang mở circuit breaker
    pub open_endpoints: Vec<String>,
    /// Số lỗi theo loại
    pub errors_by_type: HashMap<String, u64>,
}

/// Singleton để thu thập retry stats
pub static RETRY_STATS: once_cell::sync::Lazy<RwLock<RetryStats>> = once_cell::sync::Lazy::new(|| {
    RwLock::new(RetryStats {
        successful_retries: 0,
        failed_retries: 0,
        avg_retry_delay_ms: 0.0,
        open_circuit_breakers: 0,
        open_endpoints: Vec::new(),
        errors_by_type: HashMap::new(),
    })
});

/// Lấy thống kê retry hiện tại
pub fn get_retry_stats() -> RetryStats {
    RETRY_STATS.read().unwrap().clone()
}

/// Cập nhật thống kê retry
pub fn update_retry_stats(successful: bool, delay_ms: f64, error_type: Option<&str>) {
    let mut stats = RETRY_STATS.write().unwrap();
    
    if successful {
        stats.successful_retries += 1;
    } else {
        stats.failed_retries += 1;
    }
    
    // Cập nhật thời gian trung bình
    let total_retries = stats.successful_retries + stats.failed_retries;
    stats.avg_retry_delay_ms = (stats.avg_retry_delay_ms * (total_retries - 1) as f64 + delay_ms) / total_retries as f64;
    
    // Cập nhật lỗi theo loại
    if let Some(error_type) = error_type {
        *stats.errors_by_type.entry(error_type.to_string()).or_insert(0) += 1;
    }
}

/// Cập nhật thống kê về circuit breaker
pub fn update_circuit_breaker_stats(policy: Arc<ExponentialRetryPolicy>) {
    let breakers = policy.circuit_breakers.read().unwrap();
    let mut open_count = 0;
    let mut open_endpoints = Vec::new();
    
    for (endpoint, breaker) in breakers.iter() {
        if let CircuitBreakerState::Open = breaker.get_state() {
            open_count += 1;
            open_endpoints.push(endpoint.clone());
        }
    }
    
    // Update metrics
    gauge!("open_circuit_breakers", open_count as f64);
    
    // Log if there are open circuit breakers
    if open_count > 0 {
        warn!("Circuit breakers open for endpoints: {:?}", open_endpoints);
    }
}

/// Trait giúp downcast RetryPolicy
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: RetryPolicy + 'static> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryInfo {
    pub attempts: u32,
    pub last_error: Option<String>,
    pub last_attempt: u64,
}

#[derive(Debug)]
pub enum RetryDecision {
    Retry(u64), // Thời gian chờ trước khi thử lại (ms)
    Abort,
}

pub trait RetryPolicy: Send + Sync + Debug + 'static {
    fn should_retry(&self, error: &dyn Error) -> RetryDecision;
    fn reset(&self);
}

#[derive(Debug)]
pub struct RetryCallback<T> {
    callback: T,
    current: RwLock<RetryInfo>,
}

impl<T> RetryCallback<T> {
    pub fn new(callback: T) -> Self {
        Self {
            callback,
            current: RwLock::new(RetryInfo {
                attempts: 0,
                last_error: None,
                last_attempt: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            }),
        }
    }

    pub fn get_current_safe(&self) -> RetryInfo {
        // Clone dữ liệu trước khi sử dụng để tránh deadlock
        let current = self.current.read().unwrap().clone();
        current
    }
    
    pub fn update_safe(&self, mut info: RetryInfo) -> Result<(), String> {
        // Clone dữ liệu trước khi cập nhật
        let mut current = self.current.write().unwrap();
        *current = info;
        Ok(())
    }
}

impl<T: Fn(RetryInfo) -> RetryDecision + Send + Sync + 'static> RetryPolicy for RetryCallback<T> {
    fn should_retry(&self, error: &dyn Error) -> RetryDecision {
        // Clone dữ liệu trước khi sử dụng trong closure
        let current_info = self.get_current_safe();
        let mut updated_info = current_info.clone();
        
        updated_info.attempts += 1;
        updated_info.last_error = Some(error.to_string());
        updated_info.last_attempt = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        if let Err(e) = self.update_safe(updated_info.clone()) {
            warn!("Lỗi khi cập nhật trạng thái retry: {}", e);
        }
        
        // Gọi callback với dữ liệu đã clone
        (self.callback)(updated_info)
    }
    
    fn reset(&self) {
        let new_info = RetryInfo {
            attempts: 0,
            last_error: None,
            last_attempt: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        if let Err(e) = self.update_safe(new_info) {
            warn!("Lỗi khi reset retry info: {}", e);
        }
    }
} 
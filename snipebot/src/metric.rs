use once_cell::sync::Lazy;
use prometheus::{Counter, Gauge, HistogramVec, IntCounterVec, Registry};
use std::sync::Mutex;
use std::time::Instant;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Singleton registry
pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

// Metrics transaction
pub static TRANSACTION_COUNTER: Lazy<IntCounterVec> = Lazy::new(|| {
    let counter = IntCounterVec::new(
        prometheus::opts!("snipebot_transactions_total", "Total number of transactions"),
        &["type", "status"],
    )
    .expect("Failed to create transaction counter");
    REGISTRY.register(Box::new(counter.clone())).expect("Failed to register transaction counter");
    counter
});

// Metrics token analysis
pub static TOKEN_ANALYSIS_COUNTER: Lazy<IntCounterVec> = Lazy::new(|| {
    let counter = IntCounterVec::new(
        prometheus::opts!("snipebot_token_analysis_total", "Total number of token analyses"),
        &["risk_level"],
    )
    .expect("Failed to create token analysis counter");
    REGISTRY.register(Box::new(counter.clone())).expect("Failed to register token analysis counter");
    counter
});

// Metrics mempool watching
pub static MEMPOOL_TRANSACTIONS: Lazy<Counter> = Lazy::new(|| {
    let counter = Counter::new(
        "snipebot_mempool_transactions_total", 
        "Total number of mempool transactions observed"
    ).expect("Failed to create mempool transactions counter");
    REGISTRY.register(Box::new(counter.clone())).expect("Failed to register mempool transactions counter");
    counter
});

// Metrics performance
pub static API_REQUEST_DURATION: Lazy<HistogramVec> = Lazy::new(|| {
    let histogram = HistogramVec::new(
        prometheus::histogram_opts!(
            "snipebot_api_request_duration_seconds",
            "API request duration in seconds",
            vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
        ),
        &["endpoint", "method"],
    )
    .expect("Failed to create API request duration histogram");
    REGISTRY.register(Box::new(histogram.clone())).expect("Failed to register API request duration histogram");
    histogram
});

// Metrics wallet
pub static NATIVE_BALANCE: Lazy<Gauge> = Lazy::new(|| {
    let gauge = Gauge::new(
        "snipebot_native_balance", 
        "Current native token balance in wallet"
    ).expect("Failed to create native balance gauge");
    REGISTRY.register(Box::new(gauge.clone())).expect("Failed to register native balance gauge");
    gauge
});

// Metrics cho cơ chế retry
#[derive(Debug, Default)]
pub struct RetryMetrics {
    // Số lần retry thành công
    pub successful_retries: AtomicU64,
    // Số lần retry thất bại cuối cùng
    pub failed_retries: AtomicU64,
    // Tổng số lần retry (cả thành công và thất bại)
    pub total_retries: AtomicU64,
    // Số lượng giao dịch thành công ở lần thử đầu tiên
    pub first_attempt_success: AtomicU64,
    // Thời gian trung bình để hoàn thành giao dịch (ms)
    pub avg_completion_time_ms: AtomicU64,
    // Số lần provider chính thất bại và chuyển sang dự phòng
    pub fallback_provider_activations: AtomicU64,
}

// Singleton instance cho metrics toàn cục
pub static RETRY_METRICS: Lazy<Arc<RetryMetrics>> = Lazy::new(|| {
    Arc::new(RetryMetrics::default())
});

impl RetryMetrics {
    pub fn record_retry_attempt(&self, is_success: bool, attempt_number: u32, completion_time_ms: u64) {
        self.total_retries.fetch_add(1, Ordering::SeqCst);
        
        if is_success {
            self.successful_retries.fetch_add(1, Ordering::SeqCst);
            
            if attempt_number == 1 {
                self.first_attempt_success.fetch_add(1, Ordering::SeqCst);
            }
            
            // Cập nhật thời gian hoàn thành trung bình
            let current_avg = self.avg_completion_time_ms.load(Ordering::SeqCst);
            let total_successes = self.successful_retries.load(Ordering::SeqCst);
            
            if total_successes > 0 {
                let new_avg = if total_successes == 1 {
                    completion_time_ms
                } else {
                    // Công thức cập nhật average: newAvg = oldAvg + (newValue - oldAvg) / count
                    current_avg + (completion_time_ms - current_avg) / total_successes
                };
                
                self.avg_completion_time_ms.store(new_avg, Ordering::SeqCst);
            }
        } else {
            self.failed_retries.fetch_add(1, Ordering::SeqCst);
        }
    }
    
    pub fn record_fallback_provider_activation(&self) {
        self.fallback_provider_activations.fetch_add(1, Ordering::SeqCst);
    }
    
    pub fn get_success_rate(&self) -> f64 {
        let total = self.total_retries.load(Ordering::SeqCst);
        if total == 0 {
            return 0.0;
        }
        
        let successes = self.successful_retries.load(Ordering::SeqCst);
        (successes as f64 / total as f64) * 100.0
    }
    
    pub fn report(&self) -> String {
        let total = self.total_retries.load(Ordering::SeqCst);
        let successes = self.successful_retries.load(Ordering::SeqCst);
        let failures = self.failed_retries.load(Ordering::SeqCst);
        let first_attempts = self.first_attempt_success.load(Ordering::SeqCst);
        let avg_time = self.avg_completion_time_ms.load(Ordering::SeqCst);
        let fallbacks = self.fallback_provider_activations.load(Ordering::SeqCst);
        
        let success_rate = if total > 0 {
            (successes as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        
        let first_attempt_rate = if successes > 0 {
            (first_attempts as f64 / successes as f64) * 100.0
        } else {
            0.0
        };
        
        format!(
            "Retry Metrics Report:\n\
            - Tổng số retries: {}\n\
            - Thành công: {} ({}%)\n\
            - Thất bại: {} ({}%)\n\
            - Thành công ở lần đầu: {} ({}% của các giao dịch thành công)\n\
            - Thời gian hoàn thành trung bình: {}ms\n\
            - Số lần chuyển sang provider dự phòng: {}\n",
            total,
            successes, success_rate,
            failures, 100.0 - success_rate,
            first_attempts, first_attempt_rate,
            avg_time,
            fallbacks
        )
    }
}

// Struct để đo thời gian thực hiện một hàm và tự động ghi vào metrics
pub struct Timer {
    start: Instant,
    endpoint: String,
    method: String,
}

impl Timer {
    pub fn new(endpoint: &str, method: &str) -> Self {
        Self {
            start: Instant::now(),
            endpoint: endpoint.to_string(),
            method: method.to_string(),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        API_REQUEST_DURATION
            .with_label_values(&[&self.endpoint, &self.method])
            .observe(duration);
    }
}

// Metric cho gas price
lazy_static! {
    static ref GAS_PRICE_GAUGE: prometheus::GaugeVec = prometheus::GaugeVec::new(
        prometheus::Opts::new("gas_price_gwei", "Current gas price in Gwei"),
        &["chain_id"]
    ).unwrap();
    
    static ref NETWORK_CONGESTION_GAUGE: prometheus::GaugeVec = prometheus::GaugeVec::new(
        prometheus::Opts::new("network_congestion", "Network congestion level (1-4)"),
        &["chain_id"]
    ).unwrap();
}

// Initialize metrics
pub fn init_metrics() {
    // ... Phần code hiện tại ...
    
    // Đăng ký gas metrics
    prometheus::default_registry().register(Box::new(GAS_PRICE_GAUGE.clone())).unwrap_or_else(|e| {
        eprintln!("Không thể đăng ký gas_price_gauge: {}", e);
    });
    
    prometheus::default_registry().register(Box::new(NETWORK_CONGESTION_GAUGE.clone())).unwrap_or_else(|e| {
        eprintln!("Không thể đăng ký network_congestion_gauge: {}", e);
    });
}

// Cập nhật gas metrics
pub fn update_gas_metric(chain_id: u64, gas_price: u64, congestion: NetworkCongestion) {
    // Chuyển từ wei sang gwei
    let gas_price_gwei = gas_price as f64 / 1_000_000_000.0;
    
    // Cập nhật gas price gauge
    GAS_PRICE_GAUGE.with_label_values(&[&chain_id.to_string()])
        .set(gas_price_gwei);
    
    // Cập nhật congestion gauge (1-4 scale)
    let congestion_value = match congestion {
        NetworkCongestion::Low => 1.0,
        NetworkCongestion::Medium => 2.0,
        NetworkCongestion::High => 3.0,
        NetworkCongestion::VeryHigh => 4.0,
    };
    
    NETWORK_CONGESTION_GAUGE.with_label_values(&[&chain_id.to_string()])
        .set(congestion_value);
}

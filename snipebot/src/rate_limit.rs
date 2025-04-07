use std::sync::RwLock;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use serde::{Serialize, Deserialize};
use log::warn;
use metrics::counter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst_size: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitStats {
    pub total_requests: u64,
    pub blocked_requests: u64,
    pub current_rate: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
struct RateLimitData {
    count: usize,
    window_start: u64,
}

lazy_static! {
    static ref RATE_LIMIT_STORE: RwLock<HashMap<String, RateLimitData>> = RwLock::new(HashMap::new());
    static ref RATE_LIMIT_STATS: RwLock<RateLimitStats> = RwLock::new(RateLimitStats {
        total_requests: 0,
        blocked_requests: 0,
        current_rate: 0.0,
        timestamp: 0,
    });
}

/// Kiểm tra và áp dụng giới hạn tỷ lệ
pub fn check_rate_limit(key: &str, limit: usize, window_seconds: u64) -> bool {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            warn!("Lỗi khi lấy thời gian hệ thống: {}", err);
            // Fallback an toàn: cho phép request
            return true;
        }
    };
    
    let mut rate_limits = match RATE_LIMIT_STORE.try_write() {
        Ok(data) => data,
        Err(_) => {
            warn!("Không thể lấy write lock cho rate limit store");
            // Fallback an toàn khi không thể lấy lock
            return true;
        }
    };
    
    match rate_limits.entry(key.to_string()) {
        Entry::Occupied(mut entry) => {
            let data = entry.get_mut();
            
            // Kiểm tra nếu window hiện tại là window mới
            if now - data.window_start >= window_seconds {
                // Reset window
                data.window_start = now;
                data.count = 1;
                true
            } else if data.count < limit {
                // Tăng count
                data.count += 1;
                true
            } else {
                // Quá giới hạn
                counter!("rate_limit_exceeded", 1, "key" => key.to_string());
                false
            }
        },
        Entry::Vacant(entry) => {
            // Tạo entry mới
            entry.insert(RateLimitData {
                count: 1,
                window_start: now,
            });
            true
        }
    }
}

/// Làm sạch dữ liệu rate limit cũ
pub fn cleanup_old_rate_limits() {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            warn!("Lỗi khi lấy thời gian hệ thống trong cleanup_old_rate_limits: {}", err);
            // Không thể dọn dẹp nếu không lấy được thời gian hiện tại
            return;
        }
    };
    
    // Giữ window trong 1 giờ
    let expiry = 3600;
    
    let mut rate_limits = match RATE_LIMIT_STORE.try_write() {
        Ok(data) => data,
        Err(_) => {
            warn!("Không thể lấy write lock cho rate limit store trong cleanup_old_rate_limits");
            return;
        }
    };
    
    rate_limits.retain(|_, data| now - data.window_start < expiry);
    
    debug!("Đã dọn dẹp {} entry rate limit", rate_limits.len());
}

pub fn get_rate_limit_stats() -> RateLimitStats {
    RATE_LIMIT_STATS.read().unwrap().clone()
}

pub fn update_rate_limit_stats(total: u64, blocked: u64, rate: f64) {
    if let Ok(mut stats) = RATE_LIMIT_STATS.try_write() {
        stats.total_requests = total;
        stats.blocked_requests = blocked;
        stats.current_rate = rate;
        stats.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
} 
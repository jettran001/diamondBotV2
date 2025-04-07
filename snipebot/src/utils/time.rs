use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use log::warn;

/// Hàm đợi trong khoảng thời gian (milliseconds)
pub async fn wait_ms(millis: u64) {
    sleep(Duration::from_millis(millis)).await;
}

/// Hàm đợi trong khoảng thời gian (seconds)
pub async fn wait_sec(seconds: u64) {
    sleep(Duration::from_secs(seconds)).await;
}

/// Thực hiện đo thời gian chạy một hàm bất đồng bộ
pub async fn measure_async<F, T>(operation_name: &str, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let start = Instant::now();
    let result = f.await;
    let duration = start.elapsed();
    
    println!(
        "Thao tác '{}' hoàn thành trong: {:.2?}",
        operation_name,
        duration
    );
    
    result
}

/// Hàm lấy timestamp hiện tại (seconds)
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| {
            warn!("Lỗi lấy thời gian hiện tại");
            Duration::from_secs(0)
        })
        .as_secs()
}

/// Hàm lấy timestamp hiện tại (milliseconds)
pub fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| {
            warn!("Lỗi lấy thời gian hiện tại");
            Duration::from_millis(0)
        })
        .as_millis()
}

/// Hàm chuyển đổi milliseconds thành định dạng thời gian đẹp
pub fn format_duration_ms(millis: u64) -> String {
    let seconds = millis / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours % 24, minutes % 60, seconds % 60)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes % 60, seconds % 60)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds % 60)
    } else if seconds > 0 {
        format!("{}s", seconds)
    } else {
        format!("{}ms", millis)
    }
}

/// Hàm chuyển đổi timestamp sang chuỗi thời gian đọc được
pub fn timestamp_to_readable(timestamp: u64) -> String {
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp as i64, 0)
        .unwrap_or_else(|| {
            warn!("Không thể chuyển đổi timestamp: {}", timestamp);
            chrono::Utc::now()
        });
    
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Timeout wrapper cho Future
pub async fn with_timeout<F, T>(future: F, timeout_ms: u64, operation_name: &str) -> Option<T>
where
    F: std::future::Future<Output = T>,
{
    let timeout_duration = Duration::from_millis(timeout_ms);
    
    match tokio::time::timeout(timeout_duration, future).await {
        Ok(result) => Some(result),
        Err(_) => {
            warn!("Thao tác '{}' đã timeout sau {}ms", operation_name, timeout_ms);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_wait_ms() {
        let start = Instant::now();
        wait_ms(50).await;
        let duration = start.elapsed();
        assert!(duration.as_millis() >= 50);
    }
    
    #[tokio::test]
    async fn test_measure_async() {
        async fn dummy_async_fn() -> i32 {
            wait_ms(50).await;
            42
        }
        
        let result = measure_async("dummy test", dummy_async_fn()).await;
        assert_eq!(result, 42);
    }
    
    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(1500), "1s");
        assert_eq!(format_duration_ms(60000), "1m 0s");
        assert_eq!(format_duration_ms(3661000), "1h 1m 1s");
        assert_eq!(format_duration_ms(90061000), "1d 1h 1m 1s");
    }
    
    #[tokio::test]
    async fn test_with_timeout_success() {
        async fn fast_fn() -> i32 {
            wait_ms(50).await;
            42
        }
        
        let result = with_timeout(fast_fn(), 100, "fast_fn").await;
        assert_eq!(result, Some(42));
    }
    
    #[tokio::test]
    async fn test_with_timeout_failure() {
        async fn slow_fn() -> i32 {
            wait_ms(200).await;
            42
        }
        
        let result = with_timeout(slow_fn(), 100, "slow_fn").await;
        assert_eq!(result, None);
    }
} 
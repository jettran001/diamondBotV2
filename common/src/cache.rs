// File này đã được hợp nhất vào common/cache.rs
// Vui lòng sử dụng module common::cache thay vì module này.

// Re-export từ thư mục gốc
pub use crate::cache::*;

// Deprecated: Nội dung chi tiết đã được chuyển sang common/cache.rs
// Tất cả code mới nên import từ common::cache

// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::{
    collections::HashMap,
    fmt::Debug,
    hash::Hash,
    num::NonZeroUsize,
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// Third party imports
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use lru::LruCache;
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use tracing::{info, warn, debug, error};
use tokio::sync::Mutex as AsyncMutex;

/// Trait cung cấp interface chung cho tất cả các loại cache
#[async_trait]
pub trait Cache: Send + Sync {
    /// Lấy giá trị từ cache
    async fn get<K, V>(&self, key: K) -> Result<Option<V>> 
    where 
        K: AsRef<str> + Send + Sync,
        V: for<'de> Deserialize<'de> + Send + Sync + 'static;
    
    /// Lưu giá trị vào cache
    async fn set<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync,
        V: Serialize + Send + Sync + 'static;
    
    /// Xóa một giá trị khỏi cache
    async fn remove<K>(&self, key: K) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync;
    
    /// Xóa toàn bộ cache
    async fn clear(&self) -> Result<()>;
    
    /// Dọn dẹp các giá trị đã hết hạn
    async fn cleanup(&self) -> Result<()>;

    /// Lấy số lượng phần tử trong cache
    async fn len(&self) -> Result<usize>;
    
    /// Lấy JSON từ cache
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.get(key).await
    }
    
    /// Lưu JSON vào cache
    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.set(key, value, ttl_seconds).await
    }
    
    /// Dọn dẹp cache
    async fn cleanup_cache(&self) -> Result<()> {
        self.cleanup().await
    }
}

/// Cấu trúc giá trị trong cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    /// Giá trị lưu trữ
    pub value: T,
    /// Thời gian hết hạn (Unix timestamp, giây)
    pub expires_at: u64,
}

impl<T> CacheEntry<T> {
    /// Tạo entry mới
    pub fn new(value: T, ttl_seconds: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        
        Self {
            value,
            expires_at: now + ttl_seconds,
        }
    }
    
    /// Kiểm tra entry đã hết hạn chưa
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        
        self.expires_at < now
    }
}

/// Cấu hình cho cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Thời gian sống mặc định (giây)
    pub default_ttl: u64,
    /// Kích thước tối đa của cache
    pub max_size: usize,
    /// Tần suất dọn dẹp tự động (giây)
    pub cleanup_interval: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: 300,      // 5 phút
            max_size: 10000,       // 10K phần tử
            cleanup_interval: 3600, // 1 giờ
        }
    }
}

/// Cache cơ bản lưu trong bộ nhớ
pub struct BasicCache {
    entries: RwLock<HashMap<String, String>>,
    expirations: RwLock<HashMap<String, u64>>,
    config: CacheConfig,
}

impl BasicCache {
    /// Tạo cache mới
    pub fn new(config: Option<CacheConfig>) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            expirations: RwLock::new(HashMap::new()),
            config: config.unwrap_or_default(),
        }
    }
    
    /// Kiểm tra cache đã đầy chưa
    fn is_full(&self) -> bool {
        match self.entries.read() {
            Ok(entries) => entries.len() >= self.config.max_size,
            Err(_) => false,
        }
    }
}

#[async_trait]
impl Cache for BasicCache {
    async fn get<K, V>(&self, key: K) -> Result<Option<V>> 
    where 
        K: AsRef<str> + Send + Sync,
        V: for<'de> Deserialize<'de> + Send + Sync + 'static
    {
        let key_str = key.as_ref().to_string();
        
        // Kiểm tra hết hạn
        let is_expired = match self.expirations.read() {
            Ok(expirations) => {
                match expirations.get(&key_str) {
                    Some(expires_at) => {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("SystemTime before UNIX EPOCH!")
                            .as_secs();
                        *expires_at < now
                    },
                    None => true,
                }
            },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        };
        
        if is_expired {
            // Xóa khi đã hết hạn
            let _ = self.remove(&key_str).await;
            return Ok(None);
        }
        
        // Lấy giá trị
        match self.entries.read() {
            Ok(entries) => {
                if let Some(json_str) = entries.get(&key_str) {
                    match serde_json::from_str(json_str) {
                        Ok(value) => Ok(Some(value)),
                        Err(e) => Err(anyhow!("Không thể phân tích JSON từ cache: {}", e)),
                    }
                } else {
                    Ok(None)
                }
            },
            Err(e) => Err(anyhow!("RwLock error: {}", e)),
        }
    }
    
    async fn set<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync,
        V: Serialize + Send + Sync + 'static
    {
        let key_str = key.as_ref().to_string();
        
        // Kiểm tra cache đã đầy chưa
        if self.is_full() {
            let _ = self.cleanup().await;
            // Nếu vẫn đầy, từ chối thêm giá trị mới
            if self.is_full() {
                return Err(anyhow!("Cache đã đầy, không thể thêm giá trị mới"));
            }
        }
        
        // Chuyển đổi giá trị thành JSON
        let json_str = match serde_json::to_string(&value) {
            Ok(s) => s,
            Err(e) => return Err(anyhow!("Không thể chuyển đổi giá trị thành JSON: {}", e)),
        };
        
        // Tính thời gian hết hạn
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        let expires_at = now + ttl_seconds;
        
        // Lưu vào cache
        match self.entries.write() {
            Ok(mut entries) => { entries.insert(key_str.clone(), json_str); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        match self.expirations.write() {
            Ok(mut expirations) => { expirations.insert(key_str, expires_at); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        Ok(())
    }
    
    async fn remove<K>(&self, key: K) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync
    {
        let key_str = key.as_ref().to_string();
        
        match self.entries.write() {
            Ok(mut entries) => { entries.remove(&key_str); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        match self.expirations.write() {
            Ok(mut expirations) => { expirations.remove(&key_str); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        Ok(())
    }
    
    async fn clear(&self) -> Result<()> {
        match self.entries.write() {
            Ok(mut entries) => { entries.clear(); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        match self.expirations.write() {
            Ok(mut expirations) => { expirations.clear(); },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        }
        
        Ok(())
    }
    
    async fn cleanup(&self) -> Result<()> {
        // Lấy thời điểm hiện tại
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        
        // Tìm các khóa đã hết hạn
        let expired_keys: Vec<String> = match self.expirations.read() {
            Ok(expirations) => {
                expirations.iter()
                    .filter(|(_, &expires_at)| expires_at < now)
                    .map(|(key, _)| key.clone())
                    .collect()
            },
            Err(e) => return Err(anyhow!("RwLock error: {}", e)),
        };
        
        // Xóa từng khóa
        for key in expired_keys {
            let _ = self.remove(&key).await;
        }
        
        Ok(())
    }
    
    async fn len(&self) -> Result<usize> {
        match self.entries.read() {
            Ok(entries) => Ok(entries.len()),
            Err(e) => Err(anyhow!("RwLock error: {}", e)),
        }
    }
}

/// Cache bất đồng bộ
pub struct AsyncCache {
    entries: AsyncMutex<HashMap<String, String>>,
    expirations: AsyncMutex<HashMap<String, u64>>,
    config: CacheConfig,
}

impl AsyncCache {
    /// Tạo cache mới
    pub fn new(config: Option<CacheConfig>) -> Self {
        Self {
            entries: AsyncMutex::new(HashMap::new()),
            expirations: AsyncMutex::new(HashMap::new()),
            config: config.unwrap_or_default(),
        }
    }
    
    /// Kiểm tra cache đã đầy chưa
    async fn is_full(&self) -> bool {
        self.entries.lock().await.len() >= self.config.max_size
    }
}

#[async_trait]
impl Cache for AsyncCache {
    async fn get<K, V>(&self, key: K) -> Result<Option<V>> 
    where 
        K: AsRef<str> + Send + Sync,
        V: for<'de> Deserialize<'de> + Send + Sync + 'static
    {
        let key_str = key.as_ref().to_string();
        
        // Kiểm tra hết hạn
        let is_expired = {
            let expirations = self.expirations.lock().await;
            match expirations.get(&key_str) {
                Some(expires_at) => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("SystemTime before UNIX EPOCH!")
                        .as_secs();
                    *expires_at < now
                },
                None => true,
            }
        };
        
        if is_expired {
            // Xóa khi đã hết hạn
            let _ = self.remove(&key_str).await;
            return Ok(None);
        }
        
        // Lấy giá trị
        let entries = self.entries.lock().await;
        if let Some(json_str) = entries.get(&key_str) {
            match serde_json::from_str(json_str) {
                Ok(value) => Ok(Some(value)),
                Err(e) => Err(anyhow!("Không thể phân tích JSON từ cache: {}", e)),
            }
        } else {
            Ok(None)
        }
    }
    
    async fn set<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync,
        V: Serialize + Send + Sync + 'static
    {
        let key_str = key.as_ref().to_string();
        
        // Kiểm tra cache đã đầy chưa
        if self.is_full().await {
            let _ = self.cleanup().await;
            // Nếu vẫn đầy, từ chối thêm giá trị mới
            if self.is_full().await {
                return Err(anyhow!("Cache đã đầy, không thể thêm giá trị mới"));
            }
        }
        
        // Chuyển đổi giá trị thành JSON
        let json_str = match serde_json::to_string(&value) {
            Ok(s) => s,
            Err(e) => return Err(anyhow!("Không thể chuyển đổi giá trị thành JSON: {}", e)),
        };
        
        // Tính thời gian hết hạn
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        let expires_at = now + ttl_seconds;
        
        // Lưu vào cache
        {
            let mut entries = self.entries.lock().await;
            entries.insert(key_str.clone(), json_str);
        }
        
        {
            let mut expirations = self.expirations.lock().await;
            expirations.insert(key_str, expires_at);
        }
        
        Ok(())
    }
    
    async fn remove<K>(&self, key: K) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync
    {
        let key_str = key.as_ref().to_string();
        
        {
            let mut entries = self.entries.lock().await;
            entries.remove(&key_str);
        }
        
        {
            let mut expirations = self.expirations.lock().await;
            expirations.remove(&key_str);
        }
        
        Ok(())
    }
    
    async fn clear(&self) -> Result<()> {
        {
            let mut entries = self.entries.lock().await;
            entries.clear();
        }
        
        {
            let mut expirations = self.expirations.lock().await;
            expirations.clear();
        }
        
        Ok(())
    }
    
    async fn cleanup(&self) -> Result<()> {
        // Lấy thời điểm hiện tại
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();
        
        // Tìm các khóa đã hết hạn
        let expired_keys: Vec<String> = {
            let expirations = self.expirations.lock().await;
            expirations.iter()
                .filter(|(_, &expires_at)| expires_at < now)
                .map(|(key, _)| key.clone())
                .collect()
        };
        
        // Xóa từng khóa
        for key in expired_keys {
            let _ = self.remove(&key).await;
        }
        
        Ok(())
    }
    
    async fn len(&self) -> Result<usize> {
        Ok(self.entries.lock().await.len())
    }
}

/// Cache JSON hỗ trợ serialize/deserialize
pub struct JSONCache<C: Cache> {
    cache: C,
}

impl<C: Cache> JSONCache<C> {
    /// Tạo cache JSON mới
    pub fn new(cache: C) -> Self {
        Self { cache }
    }
}

#[async_trait]
impl<C: Cache + Send + Sync + 'static> Cache for JSONCache<C> {
    async fn get<K, V>(&self, key: K) -> Result<Option<V>> 
    where 
        K: AsRef<str> + Send + Sync,
        V: for<'de> Deserialize<'de> + Send + Sync + 'static
    {
        self.cache.get(key).await
    }
    
    async fn set<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync,
        V: Serialize + Send + Sync + 'static
    {
        self.cache.set(key, value, ttl_seconds).await
    }
    
    async fn remove<K>(&self, key: K) -> Result<()>
    where 
        K: AsRef<str> + Send + Sync
    {
        self.cache.remove(key).await
    }
    
    async fn clear(&self) -> Result<()> {
        self.cache.clear().await
    }
    
    async fn cleanup(&self) -> Result<()> {
        self.cache.cleanup().await
    }
    
    async fn len(&self) -> Result<usize> {
        self.cache.len().await
    }
}

// Tạo factory để tạo instance cache
pub fn create_basic_cache(config: Option<CacheConfig>) -> BasicCache {
    BasicCache::new(config)
}

pub fn create_async_cache(config: Option<CacheConfig>) -> AsyncCache {
    AsyncCache::new(config)
}

pub fn create_json_cache<C: Cache + Send + Sync + 'static>(cache: C) -> JSONCache<C> {
    JSONCache::new(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        id: u32,
        name: String,
    }
    
    #[tokio::test]
    async fn test_cache_entry() {
        let value = TestStruct { id: 1, name: "Test".to_string() };
        let entry = CacheEntry::new(value, 60);
        
        assert_eq!(false, entry.is_expired());
        
        let value = TestStruct { id: 2, name: "Expired".to_string() };
        let mut entry = CacheEntry::new(value, 0);
        
        // Giả lập thời gian hết hạn trong quá khứ
        entry.expires_at = 0;
        assert_eq!(true, entry.is_expired());
    }
    
    #[tokio::test]
    async fn test_basic_cache() {
        let cache = create_basic_cache(None);
        
        // Test thêm và lấy
        let test_value = TestStruct { id: 42, name: "Cache Test".to_string() };
        cache.set("test", test_value.clone(), 60).await.unwrap();
        
        let retrieved: Option<TestStruct> = cache.get("test").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_value);
        
        // Test xóa
        cache.remove("test").await.unwrap();
        let retrieved: Option<TestStruct> = cache.get("test").await.unwrap();
        assert!(retrieved.is_none());
        
        // Test độ dài
        cache.set("a", 1, 60).await.unwrap();
        cache.set("b", 2, 60).await.unwrap();
        cache.set("c", 3, 60).await.unwrap();
        
        assert_eq!(cache.len().await.unwrap(), 3);
        
        // Test clear
        cache.clear().await.unwrap();
        assert_eq!(cache.len().await.unwrap(), 0);
    }
    
    #[tokio::test]
    async fn test_async_cache() {
        let cache = create_async_cache(None);
        
        // Test thêm và lấy
        let test_value = TestStruct { id: 42, name: "Async Cache Test".to_string() };
        cache.set("test", test_value.clone(), 60).await.unwrap();
        
        let retrieved: Option<TestStruct> = cache.get("test").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_value);
        
        // Test xóa
        cache.remove("test").await.unwrap();
        let retrieved: Option<TestStruct> = cache.get("test").await.unwrap();
        assert!(retrieved.is_none());
    }
    
    #[tokio::test]
    async fn test_json_cache() {
        let basic_cache = create_basic_cache(None);
        let json_cache = create_json_cache(basic_cache);
        
        // Test thêm và lấy
        let test_value = TestStruct { id: 42, name: "JSON Cache Test".to_string() };
        json_cache.set("test", test_value.clone(), 60).await.unwrap();
        
        let retrieved: Option<TestStruct> = json_cache.get("test").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_value);
    }
} 
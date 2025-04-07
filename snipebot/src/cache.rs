use std::time::{Duration, Instant};
use std::sync::RwLock;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use anyhow::Result;

/// Cấu trúc lưu trữ dữ liệu trong cache
#[derive(Debug, Clone)]
pub struct CacheEntry<T> {
    /// Giá trị được lưu trong cache
    pub value: T,
    /// Thời điểm hết hạn
    pub expires_at: Instant,
}

impl<T> CacheEntry<T> {
    /// Tạo một cache entry mới
    pub fn new(value: T, ttl_seconds: u64) -> Self {
        Self {
            value,
            expires_at: Instant::now() + Duration::from_secs(ttl_seconds),
        }
    }

    /// Kiểm tra xem entry có hết hạn không
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Instant::now()
    }
}

/// Trait cho các đối tượng có thể cache
pub trait Cacheable {
    /// Kiểu dữ liệu được lưu trong cache
    type Value;

    /// Lấy giá trị từ cache
    fn get_from_cache(&self, key: &str) -> Option<Self::Value>;

    /// Lưu giá trị vào cache
    fn store_in_cache(&self, key: &str, value: &Self::Value, ttl_seconds: u64) -> Result<()>;

    /// Dọn dẹp các entry hết hạn
    fn cleanup_cache(&self);
}

/// Cache sử dụng RwLock và HashMap
pub struct Cache<T> {
    /// Dữ liệu cache
    data: RwLock<HashMap<String, CacheEntry<T>>>,
}

impl<T: Clone> Cache<T> {
    /// Tạo một cache mới
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Lấy giá trị từ cache
    pub fn get(&self, key: &str) -> Option<T> {
        let cache = self.data.read().unwrap();
        cache.get(key)
            .filter(|entry| !entry.is_expired())
            .map(|entry| entry.value.clone())
    }

    /// Lưu giá trị vào cache
    pub fn set(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        let mut cache = self.data.write().unwrap();
        cache.insert(
            key.to_string(),
            CacheEntry::new(value, ttl_seconds)
        );
        Ok(())
    }

    /// Dọn dẹp các entry hết hạn
    pub fn cleanup(&self) {
        let mut cache = self.data.write().unwrap();
        cache.retain(|_, entry| !entry.is_expired());
    }
}

/// Cache cho dữ liệu JSON
pub type JSONCache = Cache<serde_json::Value>;

impl JSONCache {
    /// Lấy giá trị từ cache và deserialize
    pub fn get_deserialized<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.get(key)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    /// Serialize và lưu giá trị vào cache
    pub fn set_serialized<T: Serialize>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()> {
        let json = serde_json::to_value(value)?;
        self.set(key, json, ttl_seconds)
    }
} 
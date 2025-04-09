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

// Internal imports
use crate::types::{TradeConfig, RiskAnalysis};
use crate::chain_adapters::ChainAdapterEnum;
use crate::utils;
use crate::abi_utils;

// Third party imports
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use bincode;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds, TimestampSeconds};
use tracing::{debug, error, info, warn};
use redis::{self, Client, Commands, Connection, RedisResult};
use tokio::time::{Duration, timeout};

/// Cache trait cho toàn bộ hệ thống
/// Cung cấp các phương thức cơ bản để tương tác với cache
#[async_trait]
pub trait Cache: Send + Sync + 'static {
    /// Lấy giá trị từ cache
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>>;

    /// Lưu giá trị vào cache
    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()>;

    /// Xóa giá trị khỏi cache
    async fn remove(&self, key: &str) -> Result<()>;

    /// Xóa tất cả giá trị khỏi cache
    async fn clear(&self) -> Result<()>;

    /// Dọn dẹp các entry hết hạn
    async fn cleanup_cache(&self) -> Result<()>;
}

/// Cache entry
/// Cấu trúc dữ liệu để lưu trữ giá trị và thời gian hết hạn
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    /// Giá trị được lưu trong cache
    pub value: T,
    /// Thời điểm hết hạn
    #[serde_as(as = "Option<TimestampSeconds<i64>>")]
    #[serde(skip)]
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

/// Cấu hình cache
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    #[serde_as(as = "Option<TimestampSeconds<i64>>")]
    #[serde(skip)]
    pub created_at: Instant,
    /// Thời gian sống mặc định
    #[serde_as(as = "DurationSeconds<u64>")]
    pub default_ttl: Duration,
    /// Dung lượng (cho LRU cache)
    pub capacity: Option<usize>,
    /// Redis URL (cho RedisCache)
    pub redis_url: Option<String>,
    /// Redis pool size
    pub redis_pool_size: Option<usize>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            config_id: "default".to_string(),
            name: "Default Cache".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: None,
            redis_url: None,
            redis_pool_size: None,
        }
    }
}

/// Basic cache
/// Triển khai cơ bản cho Cache trait
#[derive(Debug, Clone)]
pub struct BasicCache {
    /// Cấu hình cache
    config: Arc<RwLock<CacheConfig>>,
    /// Dữ liệu cache
    entries: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl BasicCache {
    /// Tạo cache mới
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Tạo cache mới với cấu hình mặc định
    pub fn default() -> Self {
        Self::new(CacheConfig::default())
    }

    /// Cập nhật cấu hình
    pub fn update_config(&self, config: CacheConfig) -> Result<()> {
        let mut cfg = self.config.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cấu hình: {}", e))?;
        *cfg = config;
        Ok(())
    }

    /// Lấy cấu hình hiện tại
    pub fn get_config(&self) -> Result<CacheConfig> {
        let cfg = self.config.read()
            .map_err(|e| anyhow!("Không thể lấy khóa đọc cho cấu hình: {}", e))?;
        Ok(cfg.clone())
    }
}

#[async_trait]
impl Cache for BasicCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        let entries = self.entries.read()
            .map_err(|e| anyhow!("Không thể lấy khóa đọc cho cache: {}", e))?;
        
        if let Some(data) = entries.get(key) {
            let entry: CacheEntry<T> = bincode::deserialize(data)
                .with_context(|| format!("Không thể deserialize dữ liệu cache cho key: {}", key))?;
            
            if entry.is_expired() {
                debug!("Cache entry đã hết hạn cho key: {}", key);
                return Ok(None);
            }
            
            debug!("Đã tìm thấy cache entry hợp lệ cho key: {}", key);
            return Ok(Some(entry.value));
        }
        debug!("Không tìm thấy cache entry cho key: {}", key);
        Ok(None)
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        let entry = CacheEntry::new(value, ttl_seconds);
        let data = bincode::serialize(&entry)
            .with_context(|| format!("Không thể serialize dữ liệu cache cho key: {}", key))?;
        
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        entries.insert(key.to_string(), data);
        debug!("Đã lưu cache entry cho key: {} với TTL {} giây", key, ttl_seconds);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        if entries.remove(key).is_some() {
            debug!("Đã xóa cache entry cho key: {}", key);
        } else {
            debug!("Không tìm thấy cache entry để xóa cho key: {}", key);
        }
        
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        let count = entries.len();
        entries.clear();
        info!("Đã xóa {} entries từ cache", count);
        
        Ok(())
    }

    async fn cleanup_cache(&self) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        let before_count = entries.len();
        entries.retain(|key, data| {
            if let Ok(entry) = bincode::deserialize::<CacheEntry<()>>(data) {
                let valid = !entry.is_expired();
                if !valid {
                    debug!("Xóa cache entry hết hạn cho key: {}", key);
                }
                valid
            } else {
                error!("Không thể deserialize cache entry cho key: {}, xóa entry", key);
                false
            }
        });
        
        let removed = before_count - entries.len();
        if removed > 0 {
            info!("Đã xóa {} entries hết hạn từ cache", removed);
        }
        
        Ok(())
    }
}

/// LRU Cache
/// Triển khai Cache trait với LruCache bên trong để giới hạn kích thước cache
#[derive(Debug, Clone)]
pub struct LRUCache {
    /// Cấu hình cache
    config: Arc<RwLock<CacheConfig>>,
    /// Dữ liệu cache sử dụng LRU để giới hạn kích thước
    entries: Arc<RwLock<LruCache<String, Vec<u8>>>>,
}

impl LRUCache {
    /// Tạo LRU cache mới
    pub fn new(config: CacheConfig) -> Result<Self> {
        let capacity = config.capacity.unwrap_or(1000);
        let non_zero_capacity = NonZeroUsize::new(capacity)
            .ok_or_else(|| anyhow!("Dung lượng cache phải lớn hơn 0"))?;
        
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            entries: Arc::new(RwLock::new(LruCache::new(non_zero_capacity))),
        })
    }

    /// Tạo LRU cache mới với cấu hình mặc định và dung lượng chỉ định
    pub fn with_capacity(capacity: usize) -> Result<Self> {
        let mut config = CacheConfig::default();
        config.capacity = Some(capacity);
        Self::new(config)
    }

    /// Tạo LRU cache mới với cấu hình mặc định
    pub fn default() -> Result<Self> {
        let mut config = CacheConfig::default();
        config.capacity = Some(1000); // Mặc định 1000 entries
        Self::new(config)
    }

    /// Cập nhật cấu hình
    pub fn update_config(&self, config: CacheConfig) -> Result<()> {
        // Cập nhật capacity nếu khác
        if let Some(new_capacity) = config.capacity {
            let current_capacity = {
                let cfg = self.config.read()
                    .map_err(|e| anyhow!("Không thể lấy khóa đọc cho cấu hình: {}", e))?;
                cfg.capacity.unwrap_or(0)
            };
            
            if new_capacity != current_capacity {
                let non_zero_capacity = NonZeroUsize::new(new_capacity)
                    .ok_or_else(|| anyhow!("Dung lượng cache phải lớn hơn 0"))?;
                
                // Tạo cache mới với capacity mới
                let mut entries = self.entries.write()
                    .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
                
                // Lưu lại items hiện có
                let items: Vec<(String, Vec<u8>)> = entries.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                
                // Tạo cache mới và copy dữ liệu
                *entries = LruCache::new(non_zero_capacity);
                for (key, value) in items {
                    entries.put(key, value);
                }
                
                debug!("Đã cập nhật capacity của LRU cache từ {} sang {}", current_capacity, new_capacity);
            }
        }
        
        // Cập nhật config
        let mut cfg = self.config.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cấu hình: {}", e))?;
        *cfg = config;
        Ok(())
    }

    /// Lấy cấu hình hiện tại
    pub fn get_config(&self) -> Result<CacheConfig> {
        let cfg = self.config.read()
            .map_err(|e| anyhow!("Không thể lấy khóa đọc cho cấu hình: {}", e))?;
        Ok(cfg.clone())
    }
}

#[async_trait]
impl Cache for LRUCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        if let Some(data) = entries.get(key) {
            let entry: CacheEntry<T> = bincode::deserialize(data)
                .with_context(|| format!("Không thể deserialize dữ liệu cache cho key: {}", key))?;
            
            if entry.is_expired() {
                debug!("LRU cache entry đã hết hạn cho key: {}", key);
                entries.pop(key);
                return Ok(None);
            }
            
            debug!("Đã tìm thấy LRU cache entry hợp lệ cho key: {}", key);
            return Ok(Some(entry.value));
        }
        
        debug!("Không tìm thấy LRU cache entry cho key: {}", key);
        Ok(None)
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        let entry = CacheEntry::new(value, ttl_seconds);
        let data = bincode::serialize(&entry)
            .with_context(|| format!("Không thể serialize dữ liệu cache cho key: {}", key))?;
        
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        entries.put(key.to_string(), data);
        debug!("Đã lưu LRU cache entry cho key: {} với TTL {} giây", key, ttl_seconds);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        if entries.pop(key).is_some() {
            debug!("Đã xóa LRU cache entry cho key: {}", key);
        } else {
            debug!("Không tìm thấy LRU cache entry để xóa cho key: {}", key);
        }
        
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        
        let count = entries.len();
        entries.clear();
        info!("Đã xóa {} entries từ LRU cache", count);
        
        Ok(())
    }

    async fn cleanup_cache(&self) -> Result<()> {
        let mut entries = self.entries.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho cache: {}", e))?;
        let mut expired_keys = Vec::new();
        
        // Lưu danh sách các key hết hạn
        for (key, data) in entries.iter() {
            if let Ok(entry) = bincode::deserialize::<CacheEntry<()>>(data) {
                if entry.is_expired() {
                    expired_keys.push(key.clone());
                }
            } else {
                error!("Không thể deserialize LRU cache entry cho key: {}, sẽ xóa entry", key);
                expired_keys.push(key.clone());
            }
        }
        
        // Xóa các key hết hạn
        for key in &expired_keys {
            entries.pop(key);
            debug!("Xóa LRU cache entry hết hạn cho key: {}", key);
        }
        
        let removed = expired_keys.len();
        if removed > 0 {
            info!("Đã xóa {} entries hết hạn từ LRU cache", removed);
        }
        
        Ok(())
    }
}

/// Cache cho dữ liệu JSON
/// Wrapper xung quanh BasicCache để hỗ trợ serialization/deserialization JSON
#[derive(Debug, Clone)]
pub struct JSONCache {
    /// Cache cơ bản
    cache: BasicCache,
}

impl JSONCache {
    /// Tạo JSON cache mới
    pub fn new(config: CacheConfig) -> Self {
        Self {
            cache: BasicCache::new(config),
        }
    }

    /// Tạo JSON cache mới với cấu hình mặc định
    pub fn default() -> Self {
        Self::new(CacheConfig::default())
    }

    /// Lấy giá trị từ cache và deserialize
    pub async fn get_json<T: for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        if let Some(value) = self.cache.get_from_cache::<serde_json::Value>(key).await? {
            Ok(Some(serde_json::from_value(value)
                .with_context(|| format!("Không thể deserialize JSON cho key: {}", key))?))
        } else {
            Ok(None)
        }
    }

    /// Serialize và lưu giá trị vào cache
    pub async fn store_json<T: Serialize + Send + Sync + 'static>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()> {
        let json = serde_json::to_value(value)
            .with_context(|| format!("Không thể serialize thành JSON cho key: {}", key))?;
        self.cache.store_in_cache(key, json, ttl_seconds).await
    }

    /// Lấy cache cơ bản
    pub fn get_basic_cache(&self) -> &BasicCache {
        &self.cache
    }
}

/// JSON LRU Cache
/// Wrapper xung quanh LRUCache để hỗ trợ serialization/deserialization JSON
#[derive(Debug, Clone)]
pub struct JSONLRUCache {
    /// LRU cache
    cache: LRUCache,
}

impl JSONLRUCache {
    /// Tạo JSON LRU cache mới
    pub fn new(config: CacheConfig) -> Result<Self> {
        Ok(Self {
            cache: LRUCache::new(config)?,
        })
    }

    /// Tạo JSON LRU cache mới với capacity
    pub fn with_capacity(capacity: usize) -> Result<Self> {
        Ok(Self {
            cache: LRUCache::with_capacity(capacity)?,
        })
    }

    /// Tạo JSON LRU cache mới với cấu hình mặc định
    pub fn default() -> Result<Self> {
        Ok(Self {
            cache: LRUCache::default()?,
        })
    }

    /// Lấy giá trị từ cache và deserialize
    pub async fn get_json<T: for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        if let Some(value) = self.cache.get_from_cache::<serde_json::Value>(key).await? {
            Ok(Some(serde_json::from_value(value)
                .with_context(|| format!("Không thể deserialize JSON cho key: {}", key))?))
        } else {
            Ok(None)
        }
    }

    /// Serialize và lưu giá trị vào cache
    pub async fn store_json<T: Serialize + Send + Sync + 'static>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()> {
        let json = serde_json::to_value(value)
            .with_context(|| format!("Không thể serialize thành JSON cho key: {}", key))?;
        self.cache.store_in_cache(key, json, ttl_seconds).await
    }

    /// Lấy LRU cache
    pub fn get_lru_cache(&self) -> &LRUCache {
        &self.cache
    }
}

/// Triển khai Cache trait cho JSONCache
#[async_trait]
impl Cache for JSONCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.cache.remove(key).await
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear().await
    }

    async fn cleanup_cache(&self) -> Result<()> {
        self.cache.cleanup_cache().await
    }
}

/// Triển khai Cache trait cho JSONLRUCache
#[async_trait]
impl Cache for JSONLRUCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.cache.remove(key).await
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear().await
    }

    async fn cleanup_cache(&self) -> Result<()> {
        self.cache.cleanup_cache().await
    }
}

/// Wrapper cũ cho blockchain_cache.rs (để duy trì tính tương thích ngược)
#[derive(Debug, Clone)]
pub struct BlockchainCache<K, V> 
where 
    K: Eq + Hash + Clone + Debug + Send + Sync + 'static,
    V: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static
{
    lru_cache: LRUCache,
    _phantom: std::marker::PhantomData<(K, V)>,
}

impl<K, V> BlockchainCache<K, V> 
where 
    K: Eq + Hash + Clone + Debug + Send + Sync + 'static,
    V: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static
{
    /// Tạo cache mới cho blockchain
    pub fn new(capacity: usize, ttl_seconds: u64) -> Result<Self> {
        let mut config = CacheConfig::default();
        config.capacity = Some(capacity);
        config.default_ttl = Duration::from_secs(ttl_seconds);
        
        Ok(Self {
            lru_cache: LRUCache::new(config)?,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Lấy giá trị từ cache
    pub async fn get(&self, key: &K) -> Option<V> {
        let key_str = format!("{:?}", key);
        match self.lru_cache.get_from_cache::<V>(&key_str).await {
            Ok(Some(value)) => Some(value),
            _ => None,
        }
    }

    /// Thêm giá trị vào cache
    pub async fn insert(&self, key: K, value: V) {
        let key_str = format!("{:?}", key);
        if let Err(e) = self.lru_cache.store_in_cache(&key_str, value, 
            self.lru_cache.get_config().unwrap_or_default().default_ttl.as_secs()).await {
            error!("Không thể lưu vào blockchain cache: {}", e);
        }
    }
}

/// Redis cache
/// Triển khai Cache trait với Redis backend
#[derive(Debug, Clone)]
pub struct RedisCache {
    /// Cấu hình cache
    config: Arc<RwLock<CacheConfig>>,
    /// Redis client
    client: Arc<Client>,
    /// Thời gian sống mặc định
    default_ttl: u64,
}

impl RedisCache {
    /// Tạo Redis cache mới
    pub fn new(config: CacheConfig) -> Result<Self> {
        let redis_url = config.redis_url
            .clone()
            .ok_or_else(|| anyhow!("Yêu cầu Redis URL trong cấu hình cache"))?;
        
        let client = Client::open(redis_url)
            .map_err(|e| anyhow!("Không thể kết nối đến Redis: {}", e))?;
        
        let default_ttl = config.default_ttl.as_secs();
        
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            client: Arc::new(client),
            default_ttl,
        })
    }

    /// Lấy cấu hình hiện tại
    pub fn get_config(&self) -> Result<CacheConfig> {
        let cfg = self.config.read()
            .map_err(|e| anyhow!("Không thể lấy khóa đọc cho cấu hình: {}", e))?;
        Ok(cfg.clone())
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get_from_cache<T: for<'de> Deserialize<'de> + Serialize + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        let conn = self.client.get_async_connection().await
            .map_err(|e| anyhow!("Không thể lấy kết nối Redis: {}", e))?;
        
        let mut conn = conn;
        
        // Lấy dữ liệu từ Redis
        let data: Option<Vec<u8>> = conn.get(key).await
            .map_err(|e| anyhow!("Redis get error: {}", e))?;
        
        if let Some(data) = data {
            // Deserialize dữ liệu thành CacheEntry
            let entry: CacheEntry<T> = bincode::deserialize(&data)
                .with_context(|| format!("Không thể deserialize dữ liệu cache cho key: {}", key))?;
            
            if entry.is_expired() {
                debug!("Cache entry đã hết hạn cho key: {}", key);
                // Xóa key đã hết hạn
                let _: () = conn.del(key).await
                    .map_err(|e| anyhow!("Redis del error: {}", e))?;
                return Ok(None);
            }
            
            debug!("Đã tìm thấy cache entry hợp lệ cho key: {}", key);
            return Ok(Some(entry.value));
        }
        
        debug!("Không tìm thấy cache entry cho key: {}", key);
        Ok(None)
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        let conn = self.client.get_async_connection().await
            .map_err(|e| anyhow!("Không thể lấy kết nối Redis: {}", e))?;
        
        let mut conn = conn;
        
        // Tạo CacheEntry mới
        let entry = CacheEntry::new(value, ttl_seconds);
        
        // Serialize entry
        let data = bincode::serialize(&entry)
            .with_context(|| format!("Không thể serialize dữ liệu cache cho key: {}", key))?;
        
        // Lưu vào Redis với expiration
        let ttl = if ttl_seconds > 0 { ttl_seconds } else { self.default_ttl };
        
        // Lưu dữ liệu
        let _: () = conn.set(key, data).await
            .map_err(|e| anyhow!("Redis set error: {}", e))?;
        
        // Đặt expiration
        let _: () = conn.expire(key, ttl as usize).await
            .map_err(|e| anyhow!("Redis expire error: {}", e))?;
        
        debug!("Đã lưu cache entry cho key: {} với TTL {} giây", key, ttl);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let conn = self.client.get_async_connection().await
            .map_err(|e| anyhow!("Không thể lấy kết nối Redis: {}", e))?;
        
        let mut conn = conn;
        
        let removed: i64 = conn.del(key).await
            .map_err(|e| anyhow!("Redis del error: {}", e))?;
        
        if removed > 0 {
            debug!("Đã xóa cache entry cho key: {}", key);
        } else {
            debug!("Không tìm thấy cache entry để xóa cho key: {}", key);
        }
        
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let conn = self.client.get_async_connection().await
            .map_err(|e| anyhow!("Không thể lấy kết nối Redis: {}", e))?;
        
        let mut conn = conn;
        
        // Sử dụng SCAN để xóa các khóa có prefix (nếu có)
        let _: () = redis::cmd("FLUSHDB")
            .query_async(&mut conn).await
            .map_err(|e| anyhow!("Redis flushdb error: {}", e))?;
        
        info!("Đã xóa tất cả entries từ Redis cache");
        Ok(())
    }

    async fn cleanup_cache(&self) -> Result<()> {
        // Redis tự động quản lý expiration nên không cần xử lý thêm
        debug!("Redis tự động quản lý expiration, không cần cleanup thủ công");
        Ok(())
    }
}

/// JSONRedisCache
/// Wrapper cho RedisCache để làm việc trực tiếp với dữ liệu JSON
#[derive(Debug, Clone)]
pub struct JSONRedisCache {
    cache: RedisCache,
}

impl JSONRedisCache {
    /// Tạo JSON Redis cache mới
    pub fn new(config: CacheConfig) -> Result<Self> {
        Ok(Self {
            cache: RedisCache::new(config)?,
        })
    }

    /// Lấy giá trị từ cache và deserialize
    pub async fn get_json<T: for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    /// Serialize và lưu giá trị vào cache
    pub async fn store_json<T: Serialize + Send + Sync + 'static>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    /// Lấy redis cache
    pub fn get_redis_cache(&self) -> &RedisCache {
        &self.cache
    }
}

#[async_trait]
impl Cache for JSONRedisCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.cache.remove(key).await
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear().await
    }

    async fn cleanup_cache(&self) -> Result<()> {
        self.cache.cleanup_cache().await
    }
}

/// Redis Pool Config
/// Cấu hình cho pool Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisPoolConfig {
    /// URL kết nối Redis
    pub redis_url: String,
    /// Kích thước pool
    pub pool_size: usize,
    /// Thời gian sống của kết nối
    pub connection_ttl: Option<Duration>,
    /// Số kết nối tối thiểu
    pub min_idle: Option<usize>,
    /// Timeout kết nối
    pub connection_timeout: Option<Duration>,
}

impl Default for RedisPoolConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://127.0.0.1:6379".to_string(),
            pool_size: 10,
            connection_ttl: Some(Duration::from_secs(60 * 5)), // 5 phút
            min_idle: Some(2),
            connection_timeout: Some(Duration::from_secs(5)),
        }
    }
}

/// Redis Pool
/// Pool connection cho Redis để sử dụng lại các kết nối
#[derive(Debug, Clone)]
pub struct RedisPool {
    /// Cấu hình pool
    config: RedisPoolConfig,
    /// Redis client
    client: Arc<Client>,
    /// Connections
    connections: Arc<RwLock<Vec<Connection>>>,
}

impl RedisPool {
    /// Tạo pool mới
    pub fn new(config: RedisPoolConfig) -> Result<Self> {
        let client = Client::open(config.redis_url.clone())
            .map_err(|e| anyhow!("Không thể kết nối tới Redis: {}", e))?;
        
        Ok(Self {
            config,
            client: Arc::new(client),
            connections: Arc::new(RwLock::new(Vec::new())),
        })
    }
    
    /// Lấy kết nối từ pool
    pub async fn get_conn(&self) -> Result<Connection> {
        let mut connections = self.connections.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho connections: {}", e))?;
        
        if let Some(conn) = connections.pop() {
            return Ok(conn);
        }
        
        self.client.get_connection()
            .map_err(|e| anyhow!("Không thể lấy kết nối Redis: {}", e))
    }
    
    /// Trả kết nối về pool
    pub async fn return_conn(&self, conn: Connection) -> Result<()> {
        let mut connections = self.connections.write()
            .map_err(|e| anyhow!("Không thể lấy khóa ghi cho connections: {}", e))?;
        
        if connections.len() < self.config.pool_size {
            connections.push(conn);
        }
        
        Ok(())
    }
}

// Redis PubSub Cache
/// Triển khai cache với PubSub để invalidate cache distributed
#[derive(Debug, Clone)]
pub struct RedisPubSubCache {
    /// Redis cache
    cache: RedisCache,
    /// Cache key cho channel invalidate
    invalidate_channel: String,
}

impl RedisPubSubCache {
    /// Tạo RedisPubSubCache mới
    pub fn new(config: CacheConfig, invalidate_channel: &str) -> Result<Self> {
        Ok(Self {
            cache: RedisCache::new(config)?,
            invalidate_channel: invalidate_channel.to_string(),
        })
    }
    
    /// Invalidate key trong tất cả các instance
    pub async fn invalidate(&self, key: &str) -> Result<()> {
        let mut conn = self.cache.client.clone();
        let invalidate_channel = self.invalidate_channel.clone();
        let redis_cache = self.cache.clone();
        
        tokio::spawn(async move {
            match conn.get_connection() {
                Ok(mut conn) => {
                    let mut pubsub = conn.as_pubsub();
                    if let Err(e) = pubsub.subscribe(&invalidate_channel) {
                        error!("Không thể subscribe vào channel {}: {}", invalidate_channel, e);
                        return;
                    }
                    
                    loop {
                        match pubsub.get_message() {
                            Ok(msg) => {
                                let key: String = msg.get_payload().unwrap_or_default();
                                if !key.is_empty() {
                                    debug!("Nhận invalidate cho key: {}", key);
                                    if let Err(e) = redis_cache.remove(&key) {
                                        error!("Không thể xóa cache cho key {}: {}", key, e);
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Lỗi khi nhận message: {}", e);
                                break;
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Không thể kết nối tới Redis để listen invalidations: {}", e);
                }
            }
        });
        
        Ok(())
    }
}

// Triển khai Cache cho RedisPubSubCache
#[async_trait]
impl Cache for RedisPubSubCache {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        self.cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let result = self.cache.remove(key).await;
        if result.is_ok() {
            // Thông báo cho các instance khác
            if let Err(e) = self.invalidate(key).await {
                warn!("Không thể invalidate key {} trên các instance khác: {}", key, e);
            }
        }
        result
    }

    async fn clear(&self) -> Result<()> {
        let result = self.cache.clear().await;
        if result.is_ok() {
            // Thông báo cho các instance khác
            if let Err(e) = self.invalidate("*").await {
                warn!("Không thể invalidate tất cả keys trên các instance khác: {}", e);
            }
        }
        result
    }

    async fn cleanup_cache(&self) -> Result<()> {
        self.cache.cleanup_cache().await
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test CacheEntry
    #[test]
    fn test_cache_entry() {
        let entry = CacheEntry::new("test".to_string(), 3600);
        assert_eq!(entry.value, "test");
        assert!(!entry.is_expired());
    }

    /// Test CacheConfig
    #[test]
    fn test_cache_config() {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: Some(100),
        };
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test");
        assert_eq!(config.capacity, Some(100));
    }

    /// Test BasicCache
    #[tokio::test]
    async fn test_basic_cache() {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: None,
        };
        let cache = BasicCache::new(config);
        assert!(cache.config.read().unwrap().config_id == "test");
        
        // Test get_from_cache/store_in_cache
        let key = "test_key";
        let value = "test_value".to_string();
        
        // Ban đầu không có gì trong cache
        let result = cache.get_from_cache::<String>(key).await.unwrap();
        assert!(result.is_none());
        
        // Lưu value vào cache
        cache.store_in_cache(key, value.clone(), 60).await.unwrap();
        
        // Kiểm tra value đã được lưu
        let result = cache.get_from_cache::<String>(key).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), value);
        
        // Xóa value
        cache.remove(key).await.unwrap();
        
        // Kiểm tra value đã bị xóa
        let result = cache.get_from_cache::<String>(key).await.unwrap();
        assert!(result.is_none());
    }

    /// Test LRUCache
    #[tokio::test]
    async fn test_lru_cache() -> Result<()> {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: Some(2), // Chỉ lưu được 2 items
        };
        let cache = LRUCache::new(config)?;
        
        // Test get_from_cache/store_in_cache
        let key1 = "test_key1";
        let key2 = "test_key2";
        let key3 = "test_key3";
        let value1 = "test_value1".to_string();
        let value2 = "test_value2".to_string();
        let value3 = "test_value3".to_string();
        
        // Lưu 2 values vào cache
        cache.store_in_cache(key1, value1.clone(), 60).await?;
        cache.store_in_cache(key2, value2.clone(), 60).await?;
        
        // Kiểm tra cả 2 values đều có trong cache
        let result1 = cache.get_from_cache::<String>(key1).await?;
        let result2 = cache.get_from_cache::<String>(key2).await?;
        assert!(result1.is_some());
        assert!(result2.is_some());
        assert_eq!(result1.unwrap(), value1);
        assert_eq!(result2.unwrap(), value2);
        
        // Lưu value thứ 3, sẽ đẩy value thứ 1 ra do LRU
        cache.store_in_cache(key3, value3.clone(), 60).await?;
        
        // Kiểm tra value1 đã bị đẩy ra
        let result1 = cache.get_from_cache::<String>(key1).await?;
        let result3 = cache.get_from_cache::<String>(key3).await?;
        assert!(result1.is_none());
        assert!(result3.is_some());
        assert_eq!(result3.unwrap(), value3);
        
        Ok(())
    }

    /// Test JSONCache
    #[tokio::test]
    async fn test_json_cache() {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: None,
        };
        let cache = JSONCache::new(config);
        
        // Test struct để serialization
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }
        
        let key = "test_json";
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        
        // Ban đầu không có gì trong cache
        let result = cache.get_json::<TestData>(key).await.unwrap();
        assert!(result.is_none());
        
        // Lưu data vào cache
        cache.store_json(key, &data, 60).await.unwrap();
        
        // Kiểm tra data đã được lưu
        let result = cache.get_json::<TestData>(key).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), data);
    }

    /// Test JSONLRUCache
    #[tokio::test]
    async fn test_json_lru_cache() -> Result<()> {
        let config = CacheConfig {
            config_id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            created_at: Instant::now(),
            default_ttl: Duration::from_secs(3600),
            capacity: Some(2), // Chỉ lưu được 2 items
        };
        let cache = JSONLRUCache::new(config)?;
        
        // Test struct để serialization
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }
        
        let key1 = "test_json1";
        let key2 = "test_json2";
        let key3 = "test_json3";
        
        let data1 = TestData {
            name: "test1".to_string(),
            value: 42,
        };
        
        let data2 = TestData {
            name: "test2".to_string(),
            value: 43,
        };
        
        let data3 = TestData {
            name: "test3".to_string(),
            value: 44,
        };
        
        // Lưu 2 data vào cache
        cache.store_json(key1, &data1, 60).await?;
        cache.store_json(key2, &data2, 60).await?;
        
        // Kiểm tra cả 2 data đều có trong cache
        let result1 = cache.get_json::<TestData>(key1).await?;
        let result2 = cache.get_json::<TestData>(key2).await?;
        assert!(result1.is_some());
        assert!(result2.is_some());
        assert_eq!(result1.unwrap(), data1);
        assert_eq!(result2.unwrap(), data2);
        
        // Lưu data thứ 3, sẽ đẩy data thứ 1 ra do LRU
        cache.store_json(key3, &data3, 60).await?;
        
        // Kiểm tra data1 đã bị đẩy ra
        let result1 = cache.get_json::<TestData>(key1).await?;
        let result3 = cache.get_json::<TestData>(key3).await?;
        assert!(result1.is_none());
        assert!(result3.is_some());
        assert_eq!(result3.unwrap(), data3);
        
        Ok(())
    }

    /// Test BlockchainCache
    #[tokio::test]
    async fn test_blockchain_cache() -> Result<()> {
        let cache = BlockchainCache::<String, String>::new(2, 60)?;
        
        // Test get/insert
        let key1 = "test_key1".to_string();
        let key2 = "test_key2".to_string();
        let key3 = "test_key3".to_string();
        let value1 = "test_value1".to_string();
        let value2 = "test_value2".to_string();
        let value3 = "test_value3".to_string();
        
        // Ban đầu không có gì trong cache
        let result = cache.get(&key1).await;
        assert!(result.is_none());
        
        // Lưu 2 values vào cache
        cache.insert(key1.clone(), value1.clone()).await;
        cache.insert(key2.clone(), value2.clone()).await;
        
        // Kiểm tra cả 2 values đều có trong cache
        let result1 = cache.get(&key1).await;
        let result2 = cache.get(&key2).await;
        assert!(result1.is_some());
        assert!(result2.is_some());
        assert_eq!(result1.unwrap(), value1);
        assert_eq!(result2.unwrap(), value2);
        
        // Lưu value thứ 3, sẽ đẩy value thứ 1 ra do LRU
        cache.insert(key3.clone(), value3.clone()).await;
        
        // Kiểm tra value1 đã bị đẩy ra
        let result1 = cache.get(&key1).await;
        let result3 = cache.get(&key3).await;
        assert!(result1.is_none());
        assert!(result3.is_some());
        assert_eq!(result3.unwrap(), value3);
        
        Ok(())
    }

    /// Test cleanup_cache
    #[tokio::test]
    async fn test_cleanup_cache() -> Result<()> {
        let cache = BasicCache::default();
        
        // Lưu value hết hạn ngay lập tức
        cache.store_in_cache("expired", "value", 0).await?;
        
        // Lưu value còn hạn
        cache.store_in_cache("valid", "value", 3600).await?;
        
        // Chạy cleanup
        cache.cleanup_cache().await?;
        
        // Kiểm tra value hết hạn đã bị xóa
        let expired = cache.get_from_cache::<String>("expired").await?;
        assert!(expired.is_none());
        
        // Kiểm tra value còn hạn vẫn còn
        let valid = cache.get_from_cache::<String>("valid").await?;
        assert!(valid.is_some());
        
        Ok(())
    }

    /// Test update_config for LRUCache
    #[tokio::test]
    async fn test_lru_update_config() -> Result<()> {
        let mut config = CacheConfig::default();
        config.capacity = Some(2);
        let cache = LRUCache::new(config)?;
        
        // Lưu 2 items
        cache.store_in_cache("key1", "value1", 3600).await?;
        cache.store_in_cache("key2", "value2", 3600).await?;
        
        // Cập nhật config với capacity lớn hơn
        let mut new_config = CacheConfig::default();
        new_config.capacity = Some(5);
        cache.update_config(new_config)?;
        
        // Items cũ vẫn còn
        let value1 = cache.get_from_cache::<String>("key1").await?;
        let value2 = cache.get_from_cache::<String>("key2").await?;
        assert!(value1.is_some());
        assert!(value2.is_some());
        
        // Lưu thêm 3 items mới (không bị đẩy ra do capacity đã tăng)
        cache.store_in_cache("key3", "value3", 3600).await?;
        cache.store_in_cache("key4", "value4", 3600).await?;
        cache.store_in_cache("key5", "value5", 3600).await?;
        
        // Kiểm tra tất cả 5 items đều còn
        let value1 = cache.get_from_cache::<String>("key1").await?;
        let value5 = cache.get_from_cache::<String>("key5").await?;
        assert!(value1.is_some());
        assert!(value5.is_some());
        
        Ok(())
    }
} 
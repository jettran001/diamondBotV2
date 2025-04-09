use std::sync::Arc;
use anyhow::{Result, anyhow};
use redis::{Client, Connection, Commands, RedisResult, AsyncCommands};
use serde::{Serialize, Deserialize};
use log::{info, warn, error, debug};
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};
use std::collections::HashMap;

// Import Cache trait từ common/cache.rs
use common::cache::{Cache, CacheEntry, CacheConfig, RedisCache};

/// Cấu hình Redis
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedisConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    pub db: i64,
    pub pool_size: usize,
    pub timeout_ms: u64,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            db: 0,
            pool_size: 10,
            timeout_ms: 5000,
        }
    }
}

/// Redis connection pool
pub struct RedisPool {
    client: Client,
    connections: Mutex<Vec<Connection>>,
    config: RedisConfig,
}

impl RedisPool {
    pub fn new(config: RedisConfig) -> Result<Self> {
        let redis_url = match &config.password {
            Some(pass) => format!("redis://:{}@{}:{}/{}", pass, config.host, config.port, config.db),
            None => format!("redis://{}:{}/{}", config.host, config.port, config.db),
        };
        
        let client = Client::open(redis_url)?;
        
        Ok(Self {
            client,
            connections: Mutex::new(Vec::with_capacity(config.pool_size)),
            config,
        })
    }
    
    /// Lấy một kết nối từ pool
    pub async fn get_conn(&self) -> Result<Connection> {
        let mut connections = self.connections.lock().await;
        
        if let Some(conn) = connections.pop() {
            // Kiểm tra kết nối còn hoạt động không
            if let Ok(()) = redis::cmd("PING").query(&mut &conn as &mut dyn redis::ConnectionLike) {
                return Ok(conn);
            }
            // Kết nối không phản hồi, tạo mới
        }
        
        // Tạo kết nối mới nếu không còn kết nối trong pool
        match timeout(
            Duration::from_millis(self.config.timeout_ms),
            self.client.get_connection()
        ).await {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(e)) => Err(anyhow!("Redis connection error: {}", e)),
            Err(_) => Err(anyhow!("Redis connection timeout after {} ms", self.config.timeout_ms)),
        }
    }
    
    /// Trả kết nối về pool
    pub async fn return_conn(&self, conn: Connection) {
        let mut connections = self.connections.lock().await;
        
        // Chỉ giữ lại kết nối nếu pool chưa đầy
        if connections.len() < self.config.pool_size {
            connections.push(conn);
        }
    }
    
    /// Lấy client
    pub fn get_client(&self) -> Client {
        self.client.clone() 
    }
}

/// Redis Cache Service - là một wrapper xung quanh RedisCache từ common/cache.rs
pub struct RedisCacheService {
    redis_cache: Arc<dyn Cache>,
}

impl RedisCacheService {
    pub fn new(pool: Arc<RedisPool>, default_ttl: u64) -> Result<Self> {
        // Chuyển đổi cấu hình từ pool sang CacheConfig
        let redis_url = match &pool.config.password {
            Some(pass) => format!("redis://:{}@{}:{}/{}", pass, pool.config.host, pool.config.port, pool.config.db),
            None => format!("redis://{}:{}/{}", pool.config.host, pool.config.port, pool.config.db),
        };
        
        let cache_config = CacheConfig {
            config_id: "redis_cache".to_string(),
            name: "Redis Cache".to_string(),
            version: "1.0.0".to_string(),
            created_at: std::time::Instant::now(),
            default_ttl: Duration::from_secs(default_ttl),
            capacity: None,
            redis_url: Some(redis_url),
            redis_pool_size: Some(pool.config.pool_size),
        };
        
        let redis_cache = RedisCache::new(cache_config)?;
        
        Ok(Self {
            redis_cache: Arc::new(redis_cache),
        })
    }
    
    /// Lấy giá trị từ cache
    pub async fn get<T: for<'de> Deserialize<'de> + Serialize + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        self.redis_cache.get_from_cache(key).await
    }
    
    /// Lưu giá trị vào cache
    pub async fn set<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: &T, ttl: Option<u64>) -> Result<()> {
        let ttl_seconds = ttl.unwrap_or(0); // Nếu không chỉ định, sử dụng default_ttl
        self.redis_cache.store_in_cache(key, value, ttl_seconds).await
    }
    
    /// Xóa key khỏi cache
    pub async fn delete(&self, key: &str) -> Result<bool> {
        self.redis_cache.remove(key).await.map(|_| true)
    }
    
    /// Kiểm tra key có tồn tại không
    pub async fn exists(&self, key: &str) -> Result<bool> {
        // Dùng get_from_cache với type đơn giản để kiểm tra tồn tại
        Ok(self.redis_cache.get_from_cache::<()>(key).await?.is_some())
    }
    
    /// Cập nhật thời gian hết hạn
    pub async fn expire(&self, key: &str, ttl: u64) -> Result<bool> {
        // Triển khai dựa trên expiration của RedisCache
        // Không triển khai được trực tiếp, chỉ có thể set lại giá trị
        // Nên tạm thời trả về Ok(false)
        warn!("Cannot directly expire a key with the new Cache architecture. Consider using set with the same value and a new TTL.");
        Ok(false)
    }
    
    /// Increment một giá trị số
    pub async fn incr(&self, key: &str, delta: i64) -> Result<i64> {
        // Cần phải đọc, increment, ghi lại
        // Đây là một workaround vì Cache trait không hỗ trợ atomic increment
        let current_value: Option<i64> = self.redis_cache.get_from_cache(key).await?;
        let new_value = current_value.unwrap_or(0) + delta;
        self.redis_cache.store_in_cache(key, new_value, 0).await?;
        Ok(new_value)
    }
}

/// Dịch vụ Redis Pub/Sub
pub struct RedisPubSubService {
    pool: Arc<RedisPool>,
}

impl RedisPubSubService {
    pub fn new(pool: Arc<RedisPool>) -> Self {
        Self {
            pool,
        }
    }
    
    /// Publish một thông điệp
    pub async fn publish<T: Serialize>(&self, channel: &str, message: &T) -> Result<i64> {
        let mut conn = self.pool.get_conn().await?;
        
        let serialized = serde_json::to_string(message)?;
        let result: RedisResult<i64> = redis::cmd("PUBLISH")
            .arg(channel)
            .arg(serialized)
            .query(&mut &conn as &mut dyn redis::ConnectionLike);
            
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(receivers) => Ok(receivers),
            Err(e) => Err(anyhow!("Redis publish error: {}", e)),
        }
    }
    
    /// Tạo một subscriber mới
    pub async fn create_subscriber(&self) -> Result<redis::aio::PubSub> {
        let client = self.pool.get_client();
        let con = client.get_async_connection().await?;
        let pubsub = con.into_pubsub();
        Ok(pubsub)
    }
    
    /// Subscribe vào một kênh và xử lý thông điệp
    pub async fn subscribe_and_process<F, T>(&self, channel: &str, handler: F) -> Result<()>
    where
        F: Fn(T) -> Result<()> + Send + Sync + 'static,
        T: for<'de> Deserialize<'de> + Send + 'static,
    {
        let mut pubsub = self.create_subscriber().await?;
        pubsub.subscribe(channel).await?;
        
        let mut msg_stream = pubsub.on_message();
        
        // Bắt đầu xử lý thông điệp trong một task riêng
        tokio::spawn(async move {
            while let Some(msg) = msg_stream.next().await {
                let payload: String = msg.get_payload().unwrap_or_else(|e| {
                    error!("Failed to get message payload: {}", e);
                    "".to_string()
                });
                
                if payload.is_empty() {
                    continue;
                }
                
                match serde_json::from_str::<T>(&payload) {
                    Ok(data) => {
                        if let Err(e) = handler(data) {
                            error!("Error handling message: {}", e);
                        }
                    },
                    Err(e) => {
                        error!("Failed to deserialize message: {}", e);
                    }
                }
            }
        });
        
        Ok(())
    }
}

/// Dịch vụ Redis Queue
pub struct RedisQueueService {
    pool: Arc<RedisPool>,
}

impl RedisQueueService {
    pub fn new(pool: Arc<RedisPool>) -> Self {
        Self {
            pool,
        }
    }
    
    /// Thêm một item vào queue
    pub async fn enqueue<T: Serialize>(&self, queue_name: &str, item: &T) -> Result<()> {
        let mut conn = self.pool.get_conn().await?;
        
        let serialized = serde_json::to_string(item)?;
        let result: RedisResult<i64> = conn.rpush(queue_name, serialized);
        
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Redis enqueue error: {}", e)),
        }
    }
    
    /// Lấy một item từ queue (blocking)
    pub async fn dequeue<T: for<'de> Deserialize<'de>>(&self, queue_name: &str, timeout_seconds: u64) -> Result<Option<T>> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<Option<String>> = redis::cmd("BLPOP")
            .arg(queue_name)
            .arg(timeout_seconds)
            .query(&mut &conn as &mut dyn redis::ConnectionLike);
            
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(Some(data)) => {
                // BLPOP trả về (key, value)
                let parts: Vec<&str> = data.split(',').collect();
                if parts.len() == 2 {
                    let value = parts[1].trim();
                    Ok(Some(serde_json::from_str(value)?))
                } else {
                    Err(anyhow!("Unexpected BLPOP response format"))
                }
            },
            Ok(None) => Ok(None), // Timeout
            Err(e) => Err(anyhow!("Redis dequeue error: {}", e)),
        }
    }
    
    /// Lấy độ dài của queue
    pub async fn queue_length(&self, queue_name: &str) -> Result<usize> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<usize> = conn.llen(queue_name);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(len) => Ok(len),
            Err(e) => Err(anyhow!("Redis queue length error: {}", e)),
        }
    }
}

/// Redis service chính, gom các dịch vụ con
pub struct RedisService {
    pub cache: RedisCacheService,
    pub pubsub: RedisPubSubService,
    pub queue: RedisQueueService,
    pool: Arc<RedisPool>,
}

impl RedisService {
    pub fn new(config: RedisConfig) -> Result<Self> {
        let pool = Arc::new(RedisPool::new(config.clone())?);
        
        Ok(Self {
            cache: RedisCacheService::new(pool.clone(), 3600)?, // 1 giờ mặc định
            pubsub: RedisPubSubService::new(pool.clone()),
            queue: RedisQueueService::new(pool.clone()),
            pool,
        })
    }
    
    /// Kiểm tra kết nối Redis
    pub async fn check_connection(&self) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<String> = redis::cmd("PING").query(&mut &conn as &mut dyn redis::ConnectionLike);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(response) => Ok(response == "PONG"),
            Err(e) => Err(anyhow!("Redis connection check failed: {}", e)),
        }
    }
    
    /// Tạo Redis Lock
    pub async fn create_lock(&self, lock_name: &str, token: &str, ttl_seconds: u64) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<String> = redis::cmd("SET")
            .arg(format!("lock:{}", lock_name))
            .arg(token)
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query(&mut &conn as &mut dyn redis::ConnectionLike);
            
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(response) => Ok(response == "OK"),
            Err(e) => Err(anyhow!("Redis create lock error: {}", e)),
        }
    }
    
    /// Giải phóng lock
    pub async fn release_lock(&self, lock_name: &str, token: &str) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        // Lua script để đảm bảo chỉ xóa lock nếu nó thuộc về chúng ta
        let script = r#"
            if redis.call("get", KEYS[1]) == ARGV[1] then
                return redis.call("del", KEYS[1])
            else
                return 0
            end
        "#;
        
        let result: RedisResult<i32> = redis::Script::new(script)
            .key(format!("lock:{}", lock_name))
            .arg(token)
            .invoke(&mut &conn as &mut dyn redis::ConnectionLike);
            
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(deleted) => Ok(deleted == 1),
            Err(e) => Err(anyhow!("Redis release lock error: {}", e)),
        }
    }
}

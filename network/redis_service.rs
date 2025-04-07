use std::sync::Arc;
use anyhow::{Result, anyhow};
use redis::{Client, Connection, Commands, RedisResult, AsyncCommands};
use serde::{Serialize, Deserialize};
use log::{info, warn, error, debug};
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};
use std::collections::HashMap;

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
}

/// Redis Cache Service
pub struct RedisCacheService {
    pool: Arc<RedisPool>,
    default_ttl: u64, // seconds
}

impl RedisCacheService {
    pub fn new(pool: Arc<RedisPool>, default_ttl: u64) -> Self {
        Self {
            pool,
            default_ttl,
        }
    }
    
    /// Lấy giá trị từ cache
    pub async fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<Option<String>> = conn.get(key);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(Some(data)) => {
                Ok(Some(serde_json::from_str(&data)?))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow!("Redis get error: {}", e)),
        }
    }
    
    /// Lưu giá trị vào cache
    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Option<u64>) -> Result<()> {
        let mut conn = self.pool.get_conn().await?;
        
        let serialized = serde_json::to_string(value)?;
        let expire_seconds = ttl.unwrap_or(self.default_ttl);
        
        let result: RedisResult<()> = if expire_seconds > 0 {
            conn.set_ex(key, serialized, expire_seconds as usize)
        } else {
            conn.set(key, serialized)
        };
        
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Redis set error: {}", e)),
        }
    }
    
    /// Xóa key khỏi cache
    pub async fn delete(&self, key: &str) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<i32> = conn.del(key);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(deleted) => Ok(deleted > 0),
            Err(e) => Err(anyhow!("Redis delete error: {}", e)),
        }
    }
    
    /// Kiểm tra key có tồn tại không
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<bool> = conn.exists(key);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(exists) => Ok(exists),
            Err(e) => Err(anyhow!("Redis exists error: {}", e)),
        }
    }
    
    /// Cập nhật thời gian hết hạn
    pub async fn expire(&self, key: &str, ttl: u64) -> Result<bool> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<bool> = conn.expire(key, ttl as usize);
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(updated) => Ok(updated),
            Err(e) => Err(anyhow!("Redis expire error: {}", e)),
        }
    }
    
    /// Increment một giá trị số
    pub async fn incr(&self, key: &str, delta: i64) -> Result<i64> {
        let mut conn = self.pool.get_conn().await?;
        
        let result: RedisResult<i64> = if delta == 1 {
            conn.incr(key, 1)
        } else {
            conn.incr_by(key, delta)
        };
        
        self.pool.return_conn(conn).await;
        
        match result {
            Ok(new_value) => Ok(new_value),
            Err(e) => Err(anyhow!("Redis increment error: {}", e)),
        }
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
        let client = self.pool.client.clone();
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
            cache: RedisCacheService::new(pool.clone(), 3600), // 1 giờ mặc định
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

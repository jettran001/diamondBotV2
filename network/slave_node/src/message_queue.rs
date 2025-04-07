use std::sync::Arc;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::StreamExt;
use log::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, timeout};

use crate::redis_service::RedisService;

/// Thông tin message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub id: String,
    pub queue: String,
    pub publisher: String,
    pub timestamp: u64,
    pub retry_count: u32,
    pub priority: MessagePriority,
    pub correlation_id: Option<String>,
    pub reply_to: Option<String>,
    pub expiration: Option<u64>,
}

/// Độ ưu tiên message
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    Low,
    Normal,
    High,
    Critical,
}

/// Message với payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message<T> where T: Serialize + for<'de> Deserialize<'de> {
    pub metadata: MessageMetadata,
    pub payload: T,
}

/// Trait cho Message Producer
#[async_trait]
pub trait MessageProducer<T> where T: Serialize + for<'de> Deserialize<'de> + Send + Sync {
    /// Gửi message vào queue
    async fn send(&self, queue: &str, payload: T, priority: MessagePriority) -> Result<String>;
    
    /// Gửi message với correlation ID
    async fn send_with_correlation(&self, queue: &str, payload: T, correlation_id: &str, priority: MessagePriority) -> Result<String>;
    
    /// Gửi message và đợi phản hồi
    async fn send_and_receive(&self, queue: &str, payload: T, timeout_secs: u64) -> Result<Option<Message<T>>>;
}

/// Trait cho Message Consumer
#[async_trait]
pub trait MessageConsumer<T> where T: Serialize + for<'de> Deserialize<'de> + Send + Sync {
    /// Nhận message từ queue
    async fn receive(&self, queue: &str, timeout_secs: u64) -> Result<Option<Message<T>>>;
    
    /// Nhận message và tự động xác nhận
    async fn receive_and_ack(&self, queue: &str, timeout_secs: u64) -> Result<Option<Message<T>>>;
    
    /// Xác nhận đã xử lý message
    async fn acknowledge(&self, message: &Message<T>) -> Result<()>;
    
    /// Từ chối message và đưa lại vào queue
    async fn reject(&self, message: &Message<T>, requeue: bool) -> Result<()>;
}

/// Triển khai Redis Message Queue
pub struct RedisMessageQueue {
    redis: Arc<RedisService>,
    node_id: String,
}

impl RedisMessageQueue {
    pub fn new(redis: Arc<RedisService>, node_id: &str) -> Self {
        Self {
            redis,
            node_id: node_id.to_string(),
        }
    }
    
    /// Tạo key cho queue
    fn queue_key(&self, queue: &str) -> String {
        format!("queue:{}", queue)
    }
    
    /// Tạo key cho processing set
    fn processing_key(&self, queue: &str) -> String {
        format!("processing:{}", queue)
    }
    
    /// Tạo message metadata
    fn create_metadata(&self, queue: &str, priority: MessagePriority, correlation_id: Option<String>, reply_to: Option<String>, expiration: Option<u64>) -> MessageMetadata {
        MessageMetadata {
            id: uuid::Uuid::new_v4().to_string(),
            queue: queue.to_string(),
            publisher: self.node_id.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            retry_count: 0,
            priority,
            correlation_id,
            reply_to,
            expiration,
        }
    }
    
    /// Tạo key cho message cụ thể
    fn message_key(&self, message_id: &str) -> String {
        format!("message:{}", message_id)
    }
    
    /// Tạo key cho reply
    fn reply_key(&self, correlation_id: &str) -> String {
        format!("reply:{}", correlation_id)
    }
}

/// Triển khai Message Producer cho Redis
#[async_trait]
impl<T> MessageProducer<T> for RedisMessageQueue
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync
{
    async fn send(&self, queue: &str, payload: T, priority: MessagePriority) -> Result<String> {
        // Tạo metadata
        let metadata = self.create_metadata(queue, priority, None, None, None);
        
        // Tạo message
        let message = Message {
            metadata: metadata.clone(),
            payload,
        };
        
        // Serialize message
        let message_data = serde_json::to_string(&message)?;
        
        // Lưu message vào Redis
        let message_key = self.message_key(&metadata.id);
        self.redis.cache.set(&message_key, &message_data, Some(3600)).await?;
        
        // Đưa reference vào queue
        let queue_key = self.queue_key(queue);
        
        // Mức độ ưu tiên quyết định vị trí trong queue
        let score = match priority {
            MessagePriority::Low => 0,
            MessagePriority::Normal => 100,
            MessagePriority::High => 200,
            MessagePriority::Critical => 300,
        } as i64 + (chrono::Utc::now().timestamp() / 1000);
        
        let result = redis::cmd("ZADD")
            .arg(&queue_key)
            .arg(score)
            .arg(&metadata.id)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        Ok(metadata.id)
    }
    
    async fn send_with_correlation(&self, queue: &str, payload: T, correlation_id: &str, priority: MessagePriority) -> Result<String> {
        // Tạo metadata với correlation ID
        let metadata = self.create_metadata(queue, priority, Some(correlation_id.to_string()), None, None);
        
        // Tạo message
        let message = Message {
            metadata: metadata.clone(),
            payload,
        };
        
        // Serialize message
        let message_data = serde_json::to_string(&message)?;
        
        // Lưu message vào Redis
        let message_key = self.message_key(&metadata.id);
        self.redis.cache.set(&message_key, &message_data, Some(3600)).await?;
        
        // Đưa reference vào queue
        let queue_key = self.queue_key(queue);
        
        // Mức độ ưu tiên quyết định vị trí trong queue
        let score = match priority {
            MessagePriority::Low => 0,
            MessagePriority::Normal => 100,
            MessagePriority::High => 200,
            MessagePriority::Critical => 300,
        } as i64 + (chrono::Utc::now().timestamp() / 1000);
        
        let result = redis::cmd("ZADD")
            .arg(&queue_key)
            .arg(score)
            .arg(&metadata.id)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        Ok(metadata.id)
    }
    
    async fn send_and_receive(&self, queue: &str, payload: T, timeout_secs: u64) -> Result<Option<Message<T>>> {
        // Tạo correlation ID
        let correlation_id = uuid::Uuid::new_v4().to_string();
        
        // Tạo reply queue
        let reply_queue = format!("reply:{}", self.node_id);
        
        // Tạo metadata
        let metadata = self.create_metadata(
            queue, 
            MessagePriority::High, 
            Some(correlation_id.clone()), 
            Some(reply_queue.clone()),
            Some(chrono::Utc::now().timestamp() as u64 + timeout_secs),
        );
        
        // Tạo message
        let message = Message {
            metadata: metadata.clone(),
            payload,
        };
        
        // Serialize message
        let message_data = serde_json::to_string(&message)?;
        
        // Lưu message vào Redis
        let message_key = self.message_key(&metadata.id);
        self.redis.cache.set(&message_key, &message_data, Some(timeout_secs as u64)).await?;
        
        // Đưa reference vào queue
        let queue_key = self.queue_key(queue);
        
        // Mức độ ưu tiên cho RPC call
        let score = 250 + (chrono::Utc::now().timestamp() / 1000);
        
        let result = redis::cmd("ZADD")
            .arg(&queue_key)
            .arg(score)
            .arg(&metadata.id)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        // Đợi phản hồi
        let reply_key = self.reply_key(&correlation_id);
        
        // Tạo channel để nhận phản hồi từ Redis pub/sub
        let (tx, mut rx) = mpsc::channel(1);
        
        // Subscribe vào Redis channel để đợi phản hồi
        let pubsub = self.redis.pubsub.create_subscriber().await?;
        pubsub.subscribe(&reply_key).await?;
        
        let mut pubsub_stream = pubsub.on_message();
        
        // Tạo task để lắng nghe phản hồi
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = pubsub_stream.next().await {
                let payload: String = msg.get_payload().unwrap_or_default();
                if !payload.is_empty() {
                    if let Ok(message) = serde_json::from_str::<Message<T>>(&payload) {
                        let _ = tx_clone.send(message).await;
                        break;
                    }
                }
            }
        });
        
        // Đợi phản hồi với timeout
        match timeout(Duration::from_secs(timeout_secs), rx.recv()).await {
            Ok(Some(message)) => Ok(Some(message)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }
}

/// Triển khai Message Consumer cho Redis
#[async_trait]
impl<T> MessageConsumer<T> for RedisMessageQueue
where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync
{
    async fn receive(&self, queue: &str, timeout_secs: u64) -> Result<Option<Message<T>>> {
        // Lấy message có mức ưu tiên cao nhất từ queue
        let queue_key = self.queue_key(queue);
        let processing_key = self.processing_key(queue);
        
        // Dùng BZPOPMAX để lấy item có score cao nhất với timeout
        let result: Option<(String, String, i64)> = redis::cmd("BZPOPMAX")
            .arg(&queue_key)
            .arg(timeout_secs)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        if let Some((_, message_id, score)) = result {
            // Lấy message data từ Redis
            let message_key = self.message_key(&message_id);
            
            if let Some(message_data) = self.redis.cache.get::<String>(&message_key).await? {
                // Deserialize message
                let mut message: Message<T> = serde_json::from_str(&message_data)?;
                
                // Tăng retry_count
                message.metadata.retry_count += 1;
                
                // Đưa vào processing set với expiration là 5 phút
                let new_score = chrono::Utc::now().timestamp() + 300;
                redis::cmd("ZADD")
                    .arg(&processing_key)
                    .arg(new_score)
                    .arg(&message_id)
                    .query_async(&mut self.redis.get_connection().await?)
                    .await?;
                    
                return Ok(Some(message));
            }
        }
        
        Ok(None)
    }
    
    async fn receive_and_ack(&self, queue: &str, timeout_secs: u64) -> Result<Option<Message<T>>> {
        match self.receive(queue, timeout_secs).await? {
            Some(message) => {
                self.acknowledge(&message).await?;
                Ok(Some(message))
            },
            None => Ok(None),
        }
    }
    
    async fn acknowledge(&self, message: &Message<T>) -> Result<()> {
        // Xóa message khỏi processing set
        let processing_key = self.processing_key(&message.metadata.queue);
        
        redis::cmd("ZREM")
            .arg(&processing_key)
            .arg(&message.metadata.id)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        // Xóa message data
        let message_key = self.message_key(&message.metadata.id);
        self.redis.cache.delete(&message_key).await?;
        
        // Nếu có reply_to, gửi phản hồi
        if let Some(reply_to) = &message.metadata.reply_to {
            if let Some(correlation_id) = &message.metadata.correlation_id {
                // Tạo reply message
                let reply_key = self.reply_key(correlation_id);
                
                // Serialize message và publish
                let message_data = serde_json::to_string(message)?;
                self.redis.pubsub.publish(&reply_key, &message_data).await?;
            }
        }
        
        Ok(())
    }
    
    async fn reject(&self, message: &Message<T>, requeue: bool) -> Result<()> {
        // Xóa message khỏi processing set
        let processing_key = self.processing_key(&message.metadata.queue);
        
        redis::cmd("ZREM")
            .arg(&processing_key)
            .arg(&message.metadata.id)
            .query_async(&mut self.redis.get_connection().await?)
            .await?;
            
        if requeue {
            // Kiểm tra giới hạn retry
            if message.metadata.retry_count < 5 {
                // Đưa lại vào queue với mức ưu tiên giảm dần
                let queue_key = self.queue_key(&message.metadata.queue);
                
                // Mức độ ưu tiên giảm dần theo số lần retry
                let priority_score = match message.metadata.priority {
                    MessagePriority::Low => 0,
                    MessagePriority::Normal => 100,
                    MessagePriority::High => 200,
                    MessagePriority::Critical => 300,
                } as i64 - (message.metadata.retry_count as i64 * 10);
                
                let score = priority_score + (chrono::Utc::now().timestamp() / 1000);
                
                redis::cmd("ZADD")
                    .arg(&queue_key)
                    .arg(score)
                    .arg(&message.metadata.id)
                    .query_async(&mut self.redis.get_connection().await?)
                    .await?;
            }
        }
        
        Ok(())
    }
}

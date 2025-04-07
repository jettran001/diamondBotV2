use redis::{AsyncCommands, Client, RedisError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use serde::{Serialize, Deserialize};

use crate::config::Config;
use crate::storage::Transaction;

#[derive(Clone)]
pub struct RedisStorage {
    client: Client,
    config: Arc<Config>,
}

impl RedisStorage {
    pub fn new(config: Arc<Config>) -> Result<Self, RedisError> {
        let client = Client::open(config.redis_url.clone())?;
        Ok(Self { client, config })
    }
    
    // Lưu transaction vào Redis
    pub async fn save_transaction(&self, tx: &Transaction) -> Result<(), RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let json = serde_json::to_string(tx).unwrap();
        
        // Lưu theo key: transaction:{id}
        let key = format!("transaction:{}", tx.id);
        let _: () = conn.set_ex(key, json, 86400 * 30).await?; // Lưu 30 ngày
        
        // Thêm id vào sorted set để dễ truy vấn theo thời gian
        let _: () = conn.zadd("transactions", tx.id.clone(), tx.timestamp as f64).await?;
        
        debug!("Đã lưu transaction {} vào Redis", tx.id);
        Ok(())
    }
    
    // Lấy transaction từ Redis
    pub async fn get_transaction(&self, id: &str) -> Result<Option<Transaction>, RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("transaction:{}", id);
        
        let json: Option<String> = conn.get(key).await?;
        
        match json {
            Some(data) => {
                let tx: Transaction = serde_json::from_str(&data).unwrap();
                Ok(Some(tx))
            },
            None => Ok(None),
        }
    }
    
    // Lấy tất cả các transaction gần đây
    pub async fn get_recent_transactions(&self, limit: usize) -> Result<Vec<Transaction>, RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        
        // Lấy các id mới nhất từ sorted set
        let ids: Vec<String> = conn.zrevrange("transactions", 0, (limit - 1) as isize).await?;
        
        let mut transactions = Vec::with_capacity(ids.len());
        
        for id in ids {
            let key = format!("transaction:{}", id);
            let json: Option<String> = conn.get(key).await?;
            
            if let Some(data) = json {
                if let Ok(tx) = serde_json::from_str::<Transaction>(&data) {
                    transactions.push(tx);
                }
            }
        }
        
        Ok(transactions)
    }
    
    // Cập nhật token cache
    pub async fn cache_token_info(&self, token_address: &str, info: &serde_json::Value) -> Result<(), RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("token:{}", token_address);
        
        let json = serde_json::to_string(info).unwrap();
        let _: () = conn.set_ex(key, json, 3600).await?; // Cache 1 giờ
        
        Ok(())
    }
    
    // Lấy thông tin token từ cache
    pub async fn get_cached_token_info(&self, token_address: &str) -> Result<Option<serde_json::Value>, RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("token:{}", token_address);
        
        let json: Option<String> = conn.get(key).await?;
        
        match json {
            Some(data) => {
                let info: serde_json::Value = serde_json::from_str(&data).unwrap();
                Ok(Some(info))
            },
            None => Ok(None),
        }
    }
    
    // Lưu kết quả phân tích token
    pub async fn save_token_analysis(&self, token_address: &str, analysis: &crate::analysis::TokenAnalysisResult) -> Result<(), RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("token_analysis:{}", token_address);
        
        let json = serde_json::to_string(analysis).unwrap();
        let _: () = conn.set_ex(key, json, 86400).await?; // Lưu 1 ngày
        
        debug!("Đã lưu phân tích token {} vào Redis", token_address);
        Ok(())
    }
    
    // Lấy kết quả phân tích token
    pub async fn get_token_analysis(&self, token_address: &str) -> Result<Option<crate::analysis::TokenAnalysisResult>, RedisError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("token_analysis:{}", token_address);
        
        let json: Option<String> = conn.get(key).await?;
        
        match json {
            Some(data) => {
                let analysis: crate::analysis::TokenAnalysisResult = serde_json::from_str(&data).unwrap();
                Ok(Some(analysis))
            },
            None => Ok(None),
        }
    }
}

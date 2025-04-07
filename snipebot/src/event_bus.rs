use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SystemEvent {
    NewTransaction(String), // Transaction hash
    TokenAnalyzed(String, bool), // Token address, is_risky
    SnipeExecuted(SnipeResult),
    MempoolTransaction(String), // Transaction hash
    ServerStatus(bool), // Is online
}

pub struct EventBus {
    tx: broadcast::Sender<SystemEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.tx.subscribe()
    }
    
    pub fn publish(&self, event: SystemEvent) -> Result<(), broadcast::error::SendError<SystemEvent>> {
        self.tx.send(event)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SnipebotError {
    #[error("Blockchain error: {0}")]
    BlockchainError(String),
    
    #[error("Redis storage error: {0}")]
    RedisError(#[from] redis::RedisError),
    
    #[error("Token analysis error: {0}")]
    AnalysisError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub backoff_multiplier: f64,
}

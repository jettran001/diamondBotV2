use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: Uuid,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub status: TransactionStatus,
    pub gas_price: u64,
    pub gas_limit: u64,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event_type: String,
    pub data: HashMap<String, String>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
} 
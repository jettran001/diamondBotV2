use std::sync::Mutex;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use tokio::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Transaction {
    pub id: String,
    pub token: String,
    pub amount: String,
    pub status: String,
    pub timestamp: u64,
}

#[derive(Debug, Default)]
pub struct Storage {
    transactions: Mutex<HashMap<String, Transaction>>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            transactions: Mutex::new(HashMap::new()),
        }
    }
    
    pub fn add_transaction(&self, tx: Transaction) {
        let mut txs = self.transactions.lock().unwrap();
        txs.insert(tx.id.clone(), tx);
    }
    
    pub fn get_transaction(&self, id: &str) -> Option<Transaction> {
        let txs = self.transactions.lock().unwrap();
        txs.get(id).cloned()
    }
    
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        let txs = self.transactions.lock().unwrap();
        txs.values().cloned().collect()
    }
    
    pub async fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let txs = self.transactions.lock().unwrap();
        let txs_vec: Vec<Transaction> = txs.values().cloned().collect();
        let json = serde_json::to_string(&txs_vec)?;
        fs::write(path, json).await?;
        Ok(())
    }
    
    pub async fn load_from_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !Path::new(path).exists() {
            return Ok(());
        }
        
        let json = fs::read_to_string(path).await?;
        let txs_vec: Vec<Transaction> = serde_json::from_str(&json)?;
        
        let mut txs = self.transactions.lock().unwrap();
        txs.clear();
        
        for tx in txs_vec {
            txs.insert(tx.id.clone(), tx);
        }
        
        Ok(())
    }
}
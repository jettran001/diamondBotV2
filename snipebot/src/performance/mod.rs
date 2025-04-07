use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct PerformanceTracker {
    // Thông tin hiệu suất tổng quát
    pub total_transactions: u64,
    pub successful_transactions: u64,
    pub failed_transactions: u64,
    pub profit_history: Vec<(u64, f64)>, // (timestamp, profit)
    pub gas_used_history: Vec<(u64, u64)>, // (timestamp, gas_used)
    
    // Thông tin về thời gian xử lý
    pub transaction_processing_times: Vec<Duration>,
    pub average_transaction_time: Duration,
    
    // Thông tin về chiến lược
    pub strategy_performance: HashMap<String, StrategyPerformance>,
    pub token_performance: HashMap<String, TokenPerformance>,
}

#[derive(Debug)]
pub struct StrategyPerformance {
    pub strategy_name: String,
    pub executed_count: u64,
    pub success_rate: f64,
    pub average_profit: f64,
    pub total_profit: f64,
    pub average_gas_cost: u64,
}

#[derive(Debug)]
pub struct TokenPerformance {
    pub token_address: String,
    pub token_symbol: String,
    pub trade_count: u64,
    pub profit_loss: f64,
    pub average_slippage: f64,
    pub liquidity_depth: f64,
    pub last_trade_timestamp: u64,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        PerformanceTracker {
            total_transactions: 0,
            successful_transactions: 0,
            failed_transactions: 0,
            profit_history: Vec::new(),
            gas_used_history: Vec::new(),
            transaction_processing_times: Vec::new(),
            average_transaction_time: Duration::from_secs(0),
            strategy_performance: HashMap::new(),
            token_performance: HashMap::new(),
        }
    }
    
    pub fn record_transaction(&mut self, success: bool, profit: f64, gas_used: u64, processing_time: Duration) {
        self.total_transactions += 1;
        
        if success {
            self.successful_transactions += 1;
        } else {
            self.failed_transactions += 1;
        }
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.profit_history.push((timestamp, profit));
        self.gas_used_history.push((timestamp, gas_used));
        self.transaction_processing_times.push(processing_time);
        
        // Cập nhật thời gian xử lý trung bình
        let total_time: Duration = self.transaction_processing_times.iter().sum();
        self.average_transaction_time = total_time / self.transaction_processing_times.len() as u32;
    }
    
    pub fn update_strategy_performance(&mut self, strategy_name: &str, success: bool, profit: f64, gas_cost: u64) {
        let entry = self.strategy_performance.entry(strategy_name.to_string()).or_insert(
            StrategyPerformance {
                strategy_name: strategy_name.to_string(),
                executed_count: 0,
                success_rate: 0.0,
                average_profit: 0.0,
                total_profit: 0.0,
                average_gas_cost: 0,
            }
        );
        
        entry.executed_count += 1;
        entry.total_profit += profit;
        entry.average_profit = entry.total_profit / entry.executed_count as f64;
        
        let success_count = if success { 
            entry.success_rate * (entry.executed_count - 1) as f64 + 1.0 
        } else { 
            entry.success_rate * (entry.executed_count - 1) as f64 
        };
        
        entry.success_rate = success_count / entry.executed_count as f64;
        
        // Tính trung bình có trọng số cho gas cost
        entry.average_gas_cost = ((entry.average_gas_cost as f64 * (entry.executed_count - 1) as f64 + gas_cost as f64) 
            / entry.executed_count as f64) as u64;
    }
    
    pub fn update_token_performance(&mut self, token_address: &str, token_symbol: &str, profit: f64, slippage: f64, liquidity: f64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let entry = self.token_performance.entry(token_address.to_string()).or_insert(
            TokenPerformance {
                token_address: token_address.to_string(),
                token_symbol: token_symbol.to_string(),
                trade_count: 0,
                profit_loss: 0.0,
                average_slippage: 0.0,
                liquidity_depth: 0.0,
                last_trade_timestamp: timestamp,
            }
        );
        
        entry.trade_count += 1;
        entry.profit_loss += profit;
        
        // Cập nhật trung bình có trọng số
        entry.average_slippage = (entry.average_slippage * (entry.trade_count - 1) as f64 + slippage) / entry.trade_count as f64;
        entry.liquidity_depth = (entry.liquidity_depth * (entry.trade_count - 1) as f64 + liquidity) / entry.trade_count as f64;
        entry.last_trade_timestamp = timestamp;
    }
    
    pub fn get_average_profit(&self) -> f64 {
        if self.profit_history.is_empty() {
            return 0.0;
        }
        
        let total_profit: f64 = self.profit_history.iter().map(|(_, profit)| profit).sum();
        total_profit / self.profit_history.len() as f64
    }
    
    pub fn get_success_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            return 0.0;
        }
        
        self.successful_transactions as f64 / self.total_transactions as f64
    }
    
    pub fn get_best_performing_strategy(&self) -> Option<&StrategyPerformance> {
        self.strategy_performance.values().max_by(|a, b| {
            a.average_profit.partial_cmp(&b.average_profit).unwrap_or(std::cmp::Ordering::Equal)
        })
    }
    
    pub fn get_best_performing_token(&self) -> Option<&TokenPerformance> {
        self.token_performance.values().max_by(|a, b| {
            a.profit_loss.partial_cmp(&b.profit_loss).unwrap_or(std::cmp::Ordering::Equal)
        })
    }
} 
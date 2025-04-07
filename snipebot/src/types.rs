// External imports
use ethers::types::{Address, U256};
use serde::{Serialize, Deserialize};
use chrono;

// Re-exports
pub use crate::token_status::{TokenStatus, TokenSafetyLevel};

/// Các loại thông báo dịch vụ
#[derive(Debug)]
pub enum ServiceMessage {
    /// Cảnh báo số dư dự trữ thấp
    ReserveBalanceAlert {
        current_percent: f64
    },
    /// Cập nhật trạng thái token
    TokenStatusUpdate {
        token_address: String,
        status: String
    },
    /// Cảnh báo rủi ro
    RiskAlert {
        token_address: String,
        risk_level: String
    }
}

/// Thông tin đăng ký dịch vụ
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Cấp độ đăng ký
    pub level: SubscriptionLevel,
    /// Thời gian đăng ký (ngày)
    pub duration_days: u64,
    /// Ngày bắt đầu
    pub start_date: chrono::DateTime<chrono::Utc>,
    /// Ngày hết hạn
    pub expiry_date: chrono::DateTime<chrono::Utc>
}

impl Subscription {
    /// Tạo subscription mới
    pub fn new(level: SubscriptionLevel, duration_days: u64) -> Self {
        let start_date = chrono::Utc::now();
        let expiry_date = start_date + chrono::Duration::days(duration_days as i64);
        
        Self {
            level,
            duration_days,
            start_date,
            expiry_date
        }
    }
    
    /// Kiểm tra subscription còn hiệu lực
    pub fn is_valid(&self) -> bool {
        chrono::Utc::now() < self.expiry_date
    }
}

/// Các cấp độ đăng ký
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionLevel {
    /// Miễn phí
    Free,
    /// Cơ bản
    Basic,
    /// Nâng cao
    Premium,
    /// Chuyên nghiệp
    Professional
}

/// Thông tin token cơ bản
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub address: Address,
    pub symbol: String,
    pub decimals: u8,
    pub router: String,
    pub pair: Option<String>,
}

/// Thông tin số dư token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    pub token_address: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub balance: String,
    pub balance_usd: Option<f64>,
}

/// Thông tin số dư ví
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    pub address: String,
    pub chain_id: u64,
    pub native_balance: String,
    pub native_balance_usd: f64,
    pub tokens: Vec<TokenBalance>,
}

/// Thông tin thống kê hệ thống
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub uptime: u64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_stats: NetworkStats,
    pub version: String,
}

/// Thông tin thống kê mạng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

/// Tham số cho chiến lược arbitrage
#[derive(Debug, Clone)]
pub struct ArbitrageParams {
    pub token_address: String,
    pub route: Vec<String>,
    pub amount: String,
    pub min_profit: f64,
    pub max_slippage: f64,
}

/// Thông tin hoạt động token trong mempool
#[derive(Debug, Clone)]
pub struct MempoolTokenActivity {
    pub token_address: String,
    pub pending_buy_count: usize,
    pub pending_sell_count: usize,
    pub average_buy_size_usd: f64,
    pub average_sell_size_usd: f64,
    pub potential_victims: Vec<PotentialVictim>,
    pub last_analyzed: u64,
}

#[derive(Debug, Clone)]
pub struct PotentialVictim {
    pub tx_hash: String,
    pub amount_usd: f64,
    pub gas_price: U256,
    pub timestamp: u64,
} 
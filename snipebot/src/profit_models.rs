use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::mempool::PendingSwap;

#[derive(Debug, Clone)]
pub struct ProfitDecision {
    pub token_address: String,
    pub recommended_action: ProfitAction,
    pub expected_profit: f64,
    pub reasoning: String,
    pub all_scenarios: Vec<ProfitScenario>,
    pub current_token_price: f64,
    pub current_profit_usd: f64,
    pub decision_time: u64,
}

#[derive(Debug, Clone)]
pub struct ProfitScenario {
    pub action: ProfitAction,
    pub expected_profit: f64,
    pub probability_success: f64,
    pub risk_factor: f64,
    pub time_horizon: u64,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfitAction {
    TakeProfit,
    HoldForPriceTarget {
        target_price: f64,
        time_limit_seconds: u64,
    },
    ContinueSandwich {
        max_additional_buys: u32,
        max_time_seconds: u64,
    },
    DCABuy {
        additional_amount_percent: u32,
        intervals: u32,
        time_frame_seconds: u64,
    },
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub action_executed: ProfitAction,
    pub success: bool,
    pub transaction_hash: Option<String>,
    pub profit_usd: f64,
    pub execution_time: u64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MempoolTokenActivity {
    pub token_address: String,
    pub pending_buy_count: usize,
    pub pending_sell_count: usize,
    pub average_buy_size_usd: f64,
    pub average_sell_size_usd: f64,
    pub potential_victims: Vec<PendingSwap>,
    pub last_analyzed: u64,
}

#[derive(Debug, Clone)]
pub struct CompetitorAnalysis {
    pub token_address: String,
    pub active_mev_bots: usize,
    pub sandwich_bot_count: usize,
    pub frontrun_bot_count: usize,
    pub arbitrage_bot_count: usize,
    pub average_gas_multiplier: f64,
    pub competitors_aggression: f64,
    pub last_analyzed: u64,
}

impl fmt::Display for ProfitAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProfitAction::TakeProfit => write!(f, "Chốt lời ngay"),
            ProfitAction::HoldForPriceTarget { target_price, time_limit_seconds } => {
                write!(f, "Giữ để đạt giá mục tiêu ${:.4} trong {} giờ", 
                       target_price, time_limit_seconds / 3600)
            },
            ProfitAction::ContinueSandwich { max_additional_buys, max_time_seconds } => {
                write!(f, "Tiếp tục sandwich tối đa {} lần trong {} giờ", 
                       max_additional_buys, max_time_seconds / 3600)
            },
            ProfitAction::DCABuy { additional_amount_percent, intervals, time_frame_seconds } => {
                write!(f, "DCA mua thêm {}% thành {} lần trong {} giờ", 
                       additional_amount_percent, intervals, time_frame_seconds / 3600)
            },
        }
    }
}

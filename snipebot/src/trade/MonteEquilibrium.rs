use ethers::prelude::*;
use ethers::providers::{Http, Provider, Middleware};
use ethers::types::{U256, H256, TransactionRequest, Transaction, Address};
use web3::types::TransactionParameters;
use std::sync::Arc;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use log::{info, warn, debug, error};
use rand::prelude::*;
use rand_distr::{Normal, Distribution};
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use crate::chain_adapters::trait_adapter::ChainAdapter;
use crate::mempool::{MempoolTracker, PendingSwap, SandwichOpportunity};
use statrs::distribution::{Normal as StatNormal, ContinuousCDF};
use statrs::statistics::Statistics;
use std::fmt;
use crate::profit_models::{
    ProfitDecision, 
    ProfitScenario, 
    ProfitAction, 
    ExecutionResult, 
    MempoolTokenActivity, 
    CompetitorAnalysis
};
use crate::config::Config;
use crate::utils;
use crate::token_status::TokenStatus;

/// Chi phí ước tính
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstimatedCost {
    pub gas_price_gwei: f64,
    pub swap_cost_eth: f64,
    pub approve_cost_eth: f64,
    pub swap_cost_usd: f64,
    pub approve_cost_usd: f64,
}

/// Tham số ưu tiên cho sandwich
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichPreferredParams {
    pub front_run_amount_percent_multiplier: f64,
    pub front_run_gas_multiplier_adjustment: f64,
    pub back_run_gas_multiplier_adjustment: f64,
    pub use_flashbots: bool,
}

// Cấu trúc dữ liệu chứa thông tin về các cầu thủ khác trong mạng lưới
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPlayer {
    pub recent_transactions: Vec<PlayerTransaction>,
    pub estimated_gas_strategy: GasStrategy,
    pub success_rate: f64,
    pub avg_profit: f64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerTransaction {
    pub tx_hash: H256,
    pub gas_price: U256,
    pub block_number: Option<u64>,
    pub success: bool,
    pub profit: Option<f64>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GasStrategy {
    Conservative,   // Thường tăng ít hơn (5-10%)
    Moderate,       // Tăng vừa phải (10-20%)
    Aggressive,     // Tăng nhiều (20-50%)
    VeryAggressive, // Tăng rất nhiều (>50%)
    Unknown,
}

// Cấu trúc chính cho Nash Equilibrium Gas Optimizer
pub struct NashEquilibriumGasOptimizer {
    players: HashMap<String, NetworkPlayer>, // địa chỉ ví -> thông tin người chơi
    mempool_state: MempoolState,
    recent_blocks: Vec<BlockInfo>,
    my_address: String,
    my_strategy: GasStrategy,
    profit_history: Vec<(u64, f64)>, // (timestamp, profit)
    simulation_results: HashMap<String, Vec<SimulationResult>>, // token -> kết quả mô phỏng
    config: OptimizerConfig,
}

#[derive(Debug, Clone)]
pub struct MempoolState {
    pub pending_tx_count: usize,
    pub avg_gas_price: U256,
    pub gas_price_percentiles: HashMap<u8, U256>, // 10, 25, 50, 75, 90 percentile
    pub congestion_level: u8, // 1-10
    pub mev_bot_activity: u8, // 1-10, đánh giá mức độ hoạt động của MEV bot
    pub last_updated: u64,
}

#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub block_number: u64,
    pub base_fee: U256,
    pub gas_used_percent: f64,
    pub successful_mev_txs: Vec<H256>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub strategy: String,
    pub gas_multiplier: f64,
    pub success_probability: f64,
    pub expected_profit: f64,
    pub worst_case_profit: f64,
    pub best_case_profit: f64,
    pub simulated_tx_count: usize,
    pub timestamp: u64,
    pub scenario: SandwichScenario,
    pub estimated_gas_cost: f64,
    pub adjusted_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    pub min_profit_threshold: f64,
    pub risk_tolerance: RiskLevel,
    pub max_gas_multiplier: f64,
    pub simulation_count: usize,
    pub player_data_ttl: u64, // thời gian sống của dữ liệu người chơi (giây)
    pub use_flashbots: bool,
    pub sandwich_enabled: bool,
    pub frontrun_enabled: bool,
    pub mev_detection_enabled: bool, // Cấu hình để bật/tắt phát hiện MEV
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RiskLevel {
    VeryLow,    // Chỉ thực hiện giao dịch khi có >90% khả năng thành công
    Low,        // >80% khả năng thành công
    Medium,     // >70% khả năng thành công
    High,       // >60% khả năng thành công
    VeryHigh,   // >50% khả năng thành công
}

// Game Theory Optimizer và Value at Risk (VaR) cho src/MonteEquilibrium.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameTheoryOptimizer {
    // Lưu trữ thông tin về các người chơi trong game
    pub players: HashMap<String, PlayerModel>,
    // Chiến lược của chúng ta
    pub my_strategy: FrontRunStrategy,
    // Lịch sử các quyết định và kết quả
    pub decision_history: VecDeque<GameDecision>,
    // Mô hình học máy đơn giản để dự đoán hành vi
    pub model: BehaviorPredictionModel,
    // Cấu hình
    pub config: GameTheoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameTheoryConfig {
    // Số lượng quyết định gần nhất để lưu trữ
    pub history_size: usize,
    // Tần suất cập nhật mô hình (theo số blocks)
    pub model_update_frequency: u64,
    // Mức độ quên (0-1): càng gần 1 càng nhanh quên thông tin cũ
    pub forgetting_factor: f64,
    // Tỷ lệ thử nghiệm chiến lược mới
    pub exploration_rate: f64,
    // Trọng số cho các tham số trong hàm mục tiêu
    pub profit_weight: f64,
    pub gas_cost_weight: f64,
    pub success_rate_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerModel {
    // Địa chỉ ví của người chơi
    pub address: String,
    // Mô hình hành vi
    pub behavior: PlayerBehavior,
    // Lịch sử tương tác
    pub interaction_history: Vec<PlayerAction>,
    // Chiến lược được đoán
    pub predicted_strategy: FrontRunStrategy,
    // Thời điểm cập nhật cuối
    pub last_updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerBehavior {
    // Mức độ hung hăng trong việc đặt gas price (0-1)
    pub aggression: f64,
    // Mức độ thích ứng (0-1)
    pub adaptability: f64,
    // Khả năng quan sát (0-1): mức độ chính xác trong quan sát mempool
    pub observability: f64,
    // Tỷ lệ thành công lịch sử
    pub success_rate: f64,
    // Xu hướng sử dụng Flashbots (0-1)
    pub flashbots_tendency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerAction {
    // Loại hành động
    pub action_type: ActionType,
    // Transaction hash
    pub tx_hash: Option<H256>,
    // Gas price
    pub gas_price: U256,
    // Block number
    pub block_number: u64,
    // Thành công hay thất bại
    pub success: bool,
    // Thời điểm
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    FrontRun,
    BackRun,
    Sandwich,
    Arbitrage,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDecision {
    // Chiến lược đã chọn
    pub chosen_strategy: FrontRunStrategy,
    // Block number
    pub block_number: u64,
    // Trạng thái mempool
    pub mempool_state: MempoolSummary,
    // Kết quả
    pub outcome: Option<GameOutcome>,
    // Timestamp
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolSummary {
    // Số lượng giao dịch đang chờ
    pub pending_tx_count: usize,
    // Mức độ tắc nghẽn (1-10)
    pub congestion_level: u8,
    // Gas price trung bình
    pub avg_gas_price: f64,
    // Gas price phân vị 90
    pub gas_price_percentile_90: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameOutcome {
    // Thành công hay thất bại
    pub success: bool,
    // Lợi nhuận (có thể âm nếu thất bại)
    pub profit: f64,
    // Chi phí gas
    pub gas_cost: f64,
    // Block được include
    pub included_in_block: Option<u64>,
    // Thời gian xác nhận (ms)
    pub confirmation_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontRunStrategy {
    // Tỷ lệ tăng gas so với giao dịch mục tiêu
    pub gas_price_multiplier: f64,
    // Thời điểm thực hiện tương đối (0: ngay lập tức, 1: đợi đến gần hết block)
    pub timing: f64,
    // Có sử dụng flashbots hay không
    pub use_flashbots: bool,
    // Có sử dụng multi-block MEV bundles hay không
    pub use_multi_block: bool,
    // Loại chiến lược
    pub strategy_type: StrategyType,
    // Kích thước giao dịch (tỷ lệ của tài sản)
    pub position_size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorPredictionModel {
    // Lần cập nhật cuối
    pub last_updated: u64,
    // Số lượng quan sát đã học
    pub observations_count: usize,
    // Ma trận chuyển tiếp Markov đơn giản
    pub transition_matrix: HashMap<StrategyType, HashMap<StrategyType, f64>>,
    // Tham số hồi quy cho dự đoán gas price
    pub regression_params: RegressionParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionParams {
    pub intercept: f64,
    pub congestion_coef: f64,
    pub timing_coef: f64,
    pub strategy_type_coefs: HashMap<StrategyType, f64>,
}

impl NashEquilibriumGasOptimizer {
    pub fn new(my_address: String, config: OptimizerConfig) -> Self {
        Self {
            players: HashMap::new(),
            mempool_state: MempoolState {
                pending_tx_count: 0,
                avg_gas_price: U256::zero(),
                gas_price_percentiles: HashMap::new(),
                congestion_level: 5,
                mev_bot_activity: 5,
                last_updated: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
            recent_blocks: Vec::new(),
            my_address,
            my_strategy: GasStrategy::Moderate,
            profit_history: Vec::new(),
            simulation_results: HashMap::new(),
            config,
        }
    }
    
    // Cập nhật trạng thái mempool
    pub async fn update_mempool_state<A: ChainAdapter>(&mut self, adapter: &A) -> Result<()> {
        let provider = adapter.get_provider();
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        // Lấy ước tính số lượng giao dịch đang chờ
        let pending_tx_count = provider.txpool_status().await?;
        self.mempool_state.pending_tx_count = pending_tx_count.pending.as_u64() as usize;
        
        // Lấy giá gas hiện tại
        let current_gas_price = provider.get_gas_price().await?;
        
        // Cập nhật giá gas trung bình
        self.mempool_state.avg_gas_price = current_gas_price;
        
        // Cập nhật mức độ tắc nghẽn dựa trên pending tx count
        let max_pending = 10000; // giả định số lượng tối đa
        let congestion = (self.mempool_state.pending_tx_count as f64 / max_pending as f64 * 10.0)
            .min(10.0)
            .max(1.0);
        self.mempool_state.congestion_level = congestion as u8;
        
        // Cập nhật timestamp
        self.mempool_state.last_updated = current_time;
        
        Ok(())
    }
    
    // Cập nhật thông tin block mới
    pub async fn update_block_info<A: ChainAdapter>(&mut self, block_number: u64, adapter: &A) -> Result<()> {
        let provider = adapter.get_provider();
        let block = provider.get_block(block_number).await?;
        
        if let Some(block) = block {
            let base_fee = block.base_fee_per_gas.unwrap_or(U256::zero());
            let gas_used_percent = if let Some(gas_limit) = block.gas_limit.as_u128().checked_div(100) {
                block.gas_used.as_u128() as f64 / gas_limit as f64
            } else {
                0.0
            };
            
            let timestamp = block.timestamp.as_u64();
            
            // Tìm các giao dịch MEV thành công trong block
            let successful_mev_txs = self.detect_mev_transactions(block).await?;
            
            let block_info = BlockInfo {
                block_number,
                base_fee,
                gas_used_percent,
                successful_mev_txs,
                timestamp,
            };
            
            // Thêm vào và giới hạn số lượng block lưu trữ (giữ 100 block gần nhất)
            self.recent_blocks.push(block_info);
            if self.recent_blocks.len() > 100 {
                self.recent_blocks.remove(0);
            }
        }
        
        Ok(())
    }
    
    // Phát hiện các giao dịch MEV trong block
    async fn detect_mev_transactions(&self, block: Block<H256>) -> Result<Vec<H256>> {
        let mut mev_txs = Vec::new();
        
        // Phân tích các giao dịch trong block để tìm MEV
        let txs = block.transactions;
        if txs.is_empty() {
            return Ok(mev_txs);
        }
        
        // 1. Chuẩn bị dữ liệu để phân tích
        let mut tx_details = Vec::new();
        let mut tx_by_hash = HashMap::new();
        
        // Tạo mapping của giao dịch đến contract và function
        for tx_hash in &txs {
            if let Ok(Some(tx)) = self.provider.get_transaction(*tx_hash).await {
                let receiver = tx.to;
                let input_data = tx.input.to_string();
                
                tx_by_hash.insert(*tx_hash, tx.clone());
                
                // Thêm thông tin hash, from, to, function signature (4 bytes đầu của input)
                let function_sig = if input_data.len() >= 10 {
                    input_data[0..10].to_string()
                } else {
                    "".to_string()
                };
                
                tx_details.push((
                    *tx_hash,
                    tx.from,
                    receiver,
                    function_sig,
                    tx.gas_price.unwrap_or_default(),
                    tx.value
                ));
            }
        }
        
        // 2. Phát hiện MEV theo các heuristics
        
        // Heuristic 1: Phát hiện sandwich attack
        // Trong sandwich attack, thường có 3 giao dịch liên tiếp liên quan đến cùng một token/pair:
        // 1. Buy từ attacker
        // 2. Buy/sell từ victim
        // 3. Sell từ attacker (cùng address với 1)
        let mut potential_sandwiches = Vec::new();
        
        // Tạo mapping nhóm theo contract đích và function
        let mut txs_by_receiver = HashMap::new();
        for (tx_hash, from, to, func_sig, gas_price, _) in &tx_details {
            if let Some(to_addr) = to {
                let key = (to_addr, func_sig.clone());
                txs_by_receiver.entry(key).or_insert_with(Vec::new).push((*tx_hash, *from, *gas_price));
            }
        }
        
        // Kiểm tra từng nhóm giao dịch gọi cùng contract + cùng function
        for ((contract, func_sig), txs_in_group) in txs_by_receiver {
            // Bỏ qua nếu chỉ có 1 giao dịch
            if txs_in_group.len() < 2 {
                continue;
            }
            
            // Tìm kiếm mẫu: cao-thấp-cao trong gas price
            if func_sig.starts_with("0xf305d719") || // Các hàm swap ở DEXs
               func_sig.starts_with("0x38ed1739") ||
               func_sig.starts_with("0x7ff36ab5") || 
               func_sig.starts_with("0x4a25d94a") ||
               func_sig.starts_with("0x18cbafe5") {
                
                // Sắp xếp theo thứ tự giao dịch (vị trí trong block)
                let mut sorted_txs = txs_in_group.clone();
                sorted_txs.sort_by_key(|(hash, _, _)| {
                    txs.iter().position(|h| h == hash).unwrap_or(usize::MAX)
                });
                
                // Xác định các nhóm giao dịch có thể là sandwich
                if sorted_txs.len() >= 3 {
                    for i in 0..sorted_txs.len()-2 {
                        let (first_hash, first_from, first_gas) = sorted_txs[i];
                        let (_, mid_from, mid_gas) = sorted_txs[i+1];
                        let (last_hash, last_from, last_gas) = sorted_txs[i+2];
                        
                        // Kiểm tra mẫu: cùng address gửi giao dịch đầu và cuối, gas giữa thấp hơn
                        if first_from == last_from && 
                           first_from != mid_from &&
                           first_gas >= mid_gas &&
                           last_gas >= mid_gas {
                            // Có thể là sandwich attack
                            debug!("Phát hiện sandwich attack: {:?} -> ? -> {:?}", first_hash, last_hash);
                            potential_sandwiches.push(first_hash);
                            potential_sandwiches.push(last_hash);
                            mev_txs.push(first_hash);
                            mev_txs.push(last_hash);
                        }
                    }
                }
            }
        }
        
        // Heuristic 2: Phát hiện frontrunning
        // Trong frontrunning, thường có 2 giao dịch liên tiếp gọi cùng một hàm:
        // 1. Giao dịch frontrun với gas price cao
        // 2. Giao dịch victim với gas price thấp hơn
        for ((contract, func_sig), txs_in_group) in &txs_by_receiver {
            if txs_in_group.len() < 2 {
                continue;
            }
            
            // Sắp xếp theo thứ tự giao dịch
            let mut sorted_txs = txs_in_group.clone();
            sorted_txs.sort_by_key(|(hash, _, _)| {
                txs.iter().position(|h| h == hash).unwrap_or(usize::MAX)
            });
            
            // Xem xét các cặp giao dịch liên tiếp
            for i in 0..sorted_txs.len()-1 {
                let (first_hash, first_from, first_gas) = sorted_txs[i];
                let (second_hash, second_from, second_gas) = sorted_txs[i+1];
                
                // Kiểm tra: giao dịch đầu có gas cao hơn và địa chỉ khác nhau
                if first_from != second_from && 
                   first_gas > second_gas && 
                   first_gas > second_gas * U256::from(120) / U256::from(100) { // gas cao hơn 20%
                    // Có thể là frontrunning
                    debug!("Phát hiện frontrunning: {:?} (gas: {:?}) -> {:?} (gas: {:?})", 
                           first_hash, first_gas, second_hash, second_gas);
                    
                    // Thêm vào danh sách MEV
                    if !mev_txs.contains(&first_hash) {
                        mev_txs.push(first_hash);
                    }
                }
            }
        }
        
        // Heuristic 3: Phát hiện arbitrage
        // Arbitrage thường là một loạt giao dịch swap qua các DEX khác nhau
        // và kết thúc với token ban đầu nhưng với số lượng cao hơn
        for (tx_hash, from, _, func_sig, gas_price, value) in &tx_details {
            // Kiểm tra nếu đây là một giao dịch swap với value thấp
            if func_sig.starts_with("0x") && value == &U256::zero() {
                // Kiểm tra xem có giao dịch swap nào khác từ cùng một địa chỉ trong cùng block
                let swaps_from_same_address = tx_details.iter()
                    .filter(|(h, f, _, fs, _, v)| 
                        f == from && 
                        h != tx_hash && 
                        fs.starts_with("0x") && 
                        v == &U256::zero())
                    .count();
                
                // Nếu có ít nhất 2 swap (tổng 3 giao dịch) từ cùng một địa chỉ, có thể là arbitrage
                if swaps_from_same_address >= 2 {
                    debug!("Phát hiện arbitrage: {:?} từ địa chỉ {:?}", tx_hash, from);
                    
                    // Thêm vào danh sách MEV
                    if !mev_txs.contains(tx_hash) {
                        mev_txs.push(*tx_hash);
                    }
                }
            }
        }
        
        debug!("Phát hiện {} giao dịch MEV trong block {}", mev_txs.len(), block.number.unwrap_or_default());
        Ok(mev_txs)
    }
    
    // Phân tích hành vi của các người chơi khác
    pub async fn analyze_players<A: ChainAdapter>(&mut self, adapter: &A) -> Result<()> {
        // Clone data needed from self before mutable borrow
        let my_address = self.my_address.clone();
        let mut players_to_update = Vec::new();
        
        // First pass: collect players to update
        for (address, player) in &self.players {
            if utils::safe_now() - player.last_seen > self.config.player_data_ttl {
                players_to_update.push(address.clone());
            }
        }
        
        // Second pass: update each player
        for address in players_to_update {
            let from_str = address.clone();
            
            // Create new player if needed
            let mut player = self.players.entry(from_str.clone()).or_insert_with(|| NetworkPlayer {
                recent_transactions: Vec::new(),
                estimated_gas_strategy: GasStrategy::Unknown,
                success_rate: 0.0,
                avg_profit: 0.0,
                last_seen: utils::safe_now(),
            });
            
            // Clone player data before analysis
            let player_data = player.clone();
            
            // Analyze player strategy
            let strategy = self.estimate_gas_strategy(&player_data);
            
            // Update player with new strategy
            player.estimated_gas_strategy = strategy;
            player.last_seen = utils::safe_now();
        }
        
        Ok(())
    }
    
    // Ước tính chiến lược gas của người chơi
    fn estimate_gas_strategy(&self, player: &NetworkPlayer) -> GasStrategy {
        if player.recent_transactions.is_empty() {
            return GasStrategy::Unknown;
        }
        
        // Tính gas price trung bình so với base fee
        let mut ratios = Vec::new();
        
        for tx in &player.recent_transactions {
            if let Some(block_number) = tx.block_number {
                if let Some(block_info) = self.recent_blocks.iter().find(|b| b.block_number == block_number) {
                    if block_info.base_fee > U256::zero() {
                        let ratio = tx.gas_price.as_u128() as f64 / block_info.base_fee.as_u128() as f64;
                        ratios.push(ratio);
                    }
                }
            }
        }
        
        if ratios.is_empty() {
            return GasStrategy::Unknown;
        }
        
        // Tính trung bình
        let avg_ratio = ratios.iter().sum::<f64>() / ratios.len() as f64;
        
        // Phân loại chiến lược
        if avg_ratio < 1.1 {
            GasStrategy::Conservative
        } else if avg_ratio < 1.2 {
            GasStrategy::Moderate
        } else if avg_ratio < 1.5 {
            GasStrategy::Aggressive
        } else {
            GasStrategy::VeryAggressive
        }
    }
    
    // Tính toán Nash Equilibrium cho gas price
    pub fn calculate_nash_equilibrium_gas(&self, base_gas_price: U256) -> Result<U256> {
        // Phân tích các chiến lược của người chơi khác
        let mut strategy_counts = HashMap::new();
        
        for player in self.players.values() {
            let count = strategy_counts.entry(&player.estimated_gas_strategy).or_insert(0);
            *count += 1;
        }
        
        // Nếu không có đủ dữ liệu, sử dụng chiến lược mặc định
        if strategy_counts.is_empty() {
            let multiplier = match self.my_strategy {
                GasStrategy::Conservative => 1.05,
                GasStrategy::Moderate => 1.1,
                GasStrategy::Aggressive => 1.2,
                GasStrategy::VeryAggressive => 1.5,
                GasStrategy::Unknown => 1.1,
            };
            
            return Ok(base_gas_price * U256::from((multiplier * 100.0) as u64) / U256::from(100));
        }
        
        // Áp dụng Nash Equilibrium:
        // Tìm chiến lược tốt nhất dựa trên chiến lược của các người chơi khác
        
        // Nếu đa số người chơi đang dùng chiến lược Aggressive, chúng ta nên dùng VeryAggressive
        // Ngược lại, nếu đa số đang dùng Conservative, chúng ta có thể dùng Moderate để tiết kiệm
        
        let total_players = self.players.len();
        let aggressive_count = strategy_counts.get(&GasStrategy::Aggressive).unwrap_or(&0) +
                             strategy_counts.get(&GasStrategy::VeryAggressive).unwrap_or(&0);
        
        let conservative_count = strategy_counts.get(&GasStrategy::Conservative).unwrap_or(&0) +
                               strategy_counts.get(&GasStrategy::Moderate).unwrap_or(&0);
                               
        // Thích ứng dựa trên tỷ lệ
        let aggressive_ratio = aggressive_count as f64 / total_players as f64;
        let conservative_ratio = conservative_count as f64 / total_players as f64;
        
        // Áp dụng lý thuyết Nash Equilibrium: chọn chiến lược tốt nhất dựa trên đối thủ
        let optimal_multiplier = if aggressive_ratio > 0.7 {
            // Khi đa số đối thủ dùng Aggressive, ta cần tăng mạnh
            1.6 // VeryAggressive + thêm chút nữa
        } else if aggressive_ratio > 0.5 {
            // Khi có nhiều đối thủ Aggressive
            1.5 // VeryAggressive
        } else if conservative_ratio > 0.8 {
            // Khi gần như tất cả đối thủ đều Conservative
            1.15 // Moderate+
        } else if conservative_ratio > 0.6 {
            // Khi đa số đối thủ là Conservative
            1.2 // Aggressive
        } else {
            // Trường hợp cân bằng
            1.3 // Giữa Aggressive và VeryAggressive
        };
        
        // Giới hạn bởi max_gas_multiplier trong config
        let capped_multiplier = optimal_multiplier.min(self.config.max_gas_multiplier);
        
        // Điều chỉnh thêm dựa trên mức độ tắc nghẽn mạng
        let congestion_multiplier = 1.0 + (self.mempool_state.congestion_level as f64 - 5.0) / 50.0;
        let final_multiplier = capped_multiplier * congestion_multiplier;
        
        // Đảm bảo final_multiplier không quá cao
        let safe_multiplier = final_multiplier.min(self.config.max_gas_multiplier);
        
        // Tính gas price cuối cùng
        let multiplier_int = (safe_multiplier * 100.0).min(u64::MAX as f64) as u64;
        let gas_price = base_gas_price * U256::from(multiplier_int) / U256::from(100);
        
        Ok(gas_price)
    }
    
    // Mô phỏng Monte Carlo cho chiến lược front-run
    pub async fn run_monte_carlo_simulation<A: ChainAdapter>(
        &mut self,
        token_address: &str,
        opportunity: &SandwichOpportunity,
        mempool_tracker: &MempoolTracker,
        adapter: &A
    ) -> Result<SimulationStrategy> {
        // Chuẩn bị các tham số mô phỏng
        let victim_tx = opportunity.victim_tx_hash.clone();
        let victim_amount_usd = opportunity.amount_usd;
        let base_gas_price = adapter.get_provider().get_gas_price().await?;
        
        // Cấu hình các chiến lược để thử nghiệm
        let strategies = vec![
            ("conservative", 1.05),
            ("moderate", 1.1),
            ("aggressive", 1.2),
            ("very_aggressive", 1.5),
            ("extreme", 2.0),
        ];
        
        let mut simulation_results = Vec::new();
        
        // Chạy mô phỏng cho mỗi chiến lược
        for (strategy_name, gas_multiplier) in strategies {
            let result = self.simulate_frontrun_strategy(
                token_address,
                victim_amount_usd,
                base_gas_price,
                gas_multiplier,
                &self.mempool_state,
                adapter,
            ).await?;
            
            simulation_results.push((strategy_name.to_string(), gas_multiplier, result));
        }
        
        // Lưu kết quả mô phỏng
        let token_key = token_address.to_string();
        let mut results = Vec::new();
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        for (strategy_name, gas_multiplier, result) in &simulation_results {
            results.push(SimulationResult {
                strategy: strategy_name.clone(),
                gas_multiplier: *gas_multiplier,
                success_probability: result.success_probability,
                expected_profit: result.expected_profit,
                worst_case_profit: result.worst_case_profit,
                best_case_profit: result.best_case_profit,
                simulated_tx_count: result.simulated_tx_count,
                timestamp: current_time,
            });
        }
        
        self.simulation_results.insert(token_key, results);
        
        // Chọn chiến lược tối ưu dựa trên risk tolerance
        let min_success_probability = match self.config.risk_tolerance {
            RiskLevel::VeryLow => 0.9,
            RiskLevel::Low => 0.8,
            RiskLevel::Medium => 0.7,
            RiskLevel::High => 0.6,
            RiskLevel::VeryHigh => 0.5,
        };
        
        // Lọc các chiến lược thỏa mãn yêu cầu success probability
        let qualified_strategies: Vec<_> = simulation_results.iter()
            .filter(|(_, _, result)| result.success_probability >= min_success_probability)
            .collect();
        
        // Nếu không có chiến lược nào thỏa mãn, chọn chiến lược có success probability cao nhất
        if qualified_strategies.is_empty() {
            let best_strategy = simulation_results.iter()
                .max_by(|(_, _, a), (_, _, b)| 
                    a.success_probability
                     .partial_cmp(&b.success_probability)
                     .unwrap_or(std::cmp::Ordering::Equal))
                .ok_or_else(|| anyhow!("Không có chiến lược nào cho mô phỏng"))?;
                
            return Ok(SimulationStrategy {
                strategy_name: best_strategy.0.clone(),
                gas_multiplier: best_strategy.1,
                gas_price: base_gas_price * U256::from((best_strategy.1 * 100.0) as u64) / U256::from(100),
                expected_profit: best_strategy.2.expected_profit,
                success_probability: best_strategy.2.success_probability,
            });
        }
        
        // Trong số các chiến lược thỏa mãn, chọn chiến lược có expected profit cao nhất
        let optimal_strategy = qualified_strategies.iter()
            .max_by(|(_, _, a), (_, _, b)| 
                a.expected_profit
                 .partial_cmp(&b.expected_profit)
                 .unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| anyhow!("Không tìm thấy chiến lược tối ưu"))?;
            
        Ok(SimulationStrategy {
            strategy_name: optimal_strategy.0.clone(),
            gas_multiplier: optimal_strategy.1,
            gas_price: base_gas_price * U256::from((optimal_strategy.1 * 100.0) as u64) / U256::from(100),
            expected_profit: optimal_strategy.2.expected_profit,
            success_probability: optimal_strategy.2.success_probability,
        })
    }
    
    // Mô phỏng một chiến lược front-run cụ thể
    async fn simulate_frontrun_strategy<A: ChainAdapter>(
        &mut self,
        token_address: &str,
        victim_amount_usd: f64,
        base_gas_price: U256,
        gas_multiplier: f64,
        mempool_state: &MempoolState,
        adapter: &A,
    ) -> Result<SimulationMetrics> {
        let simulation_count = self.config.simulation_count;
        let gas_price = base_gas_price * U256::from((gas_multiplier * 100.0) as u64) / U256::from(100);
        
        // Tham số mô phỏng
        let front_run_amount_eth = 0.1; // Giả định đầu tư 0.1 ETH cho front-run
        let eth_price_usd = adapter.get_eth_price().await?;
        let front_run_amount_usd = front_run_amount_eth * eth_price_usd;
        
        // Mô hình sẽ mô phỏng:
        // 1. Khả năng giao dịch của chúng ta được đưa vào block trước victim (success probability)
        // 2. Tác động của giao dịch front-run lên giá (price impact)
        // 3. Khả năng back-run (bán) sau khi victim mua
        // 4. Tổng lợi nhuận/chi phí
        
        let mut successes = 0;
        let mut total_profit = 0.0;
        let mut profits = Vec::with_capacity(simulation_count);
        
        let mut rng = rand::thread_rng();
        
        // Phân phối chuẩn cho các thông số ngẫu nhiên
        let price_impact_dist = match Normal::new(0.01, 0.005) {
            Ok(dist) => dist,
            Err(e) => {
                warn!("Lỗi khi tạo phân phối chuẩn cho price impact: {:?}. Sử dụng giá trị mặc định.", e);
                return Ok(SimulationMetrics {
                    profit_potential: 0.0,
                    risk_level: 10.0,
                    execution_time_ms: 0,
                    gas_used: 0,
                    success_probability: 0.0,
                    expected_profit: 0.0,
                    worst_case_profit: 0.0,
                    best_case_profit: 0.0,
                    simulated_tx_count: 0,
                });
            }
        };
        
        let competition_factor_dist = match Normal::new(0.5, 0.2) {
            Ok(dist) => dist,
            Err(e) => {
                warn!("Lỗi khi tạo phân phối chuẩn cho competition factor: {:?}. Sử dụng giá trị mặc định.", e);
                return Ok(SimulationMetrics {
                    profit_potential: 0.0,
                    risk_level: 10.0,
                    execution_time_ms: 0,
                    gas_used: 0,
                    success_probability: 0.0,
                    expected_profit: 0.0,
                    worst_case_profit: 0.0,
                    best_case_profit: 0.0,
                    simulated_tx_count: 0,
                });
            }
        };
        
        for _ in 0..simulation_count {
            // Mô phỏng khả năng giao dịch của chúng ta được xác nhận trước victim
            // Phụ thuộc vào gas_multiplier và mức độ tắc nghẽn mạng
            let base_success_prob = 0.4 + gas_multiplier * 0.2;
            let congestion_factor = mempool_state.congestion_level as f64 / 10.0;
            let competition_factor = competition_factor_dist.sample(&mut rng).max(0.1).min(0.9);
            
            let success_prob = base_success_prob * (1.0 - congestion_factor * 0.5) * (1.0 - competition_factor);
            let is_success = rng.gen::<f64>() < success_prob;
            
            if is_success {
                successes += 1;
                
                // Mô phỏng tác động giá của front-run (chúng ta mua trước)
                let our_price_impact = price_impact_dist.sample(&mut rng).max(0.001).min(0.05);
                
                // Victim mua sau chúng ta, tạo thêm tác động giá
                let victim_price_impact = (victim_amount_usd / front_run_amount_usd) * our_price_impact;
                
                // Tổng tác động giá
                let total_price_impact = our_price_impact + victim_price_impact;
                
                // Tính lợi nhuận gộp (không tính phí gas)
                let sandwich_profit = front_run_amount_usd * total_price_impact;
                
                // Tính phí gas (cho cả front-run và back-run) an toàn hơn
                let gas_limit = 250000; // gas limit cho mỗi giao dịch
                
                // Chuyển đổi gas_price thành f64 an toàn hơn
                let gas_price_gwei = if gas_price.as_u128() > u128::MAX / 2 {
                    warn!("Gas price quá lớn, sử dụng giá trị an toàn");
                    50.0 // Giá trị mặc định an toàn, 50 Gwei
                } else {
                    (gas_price.as_u128() as f64) / 1e9
                };
                
                // Tính tổng chi phí gas
                let total_gas_cost = (gas_price_gwei * gas_limit as f64 * 2.0) / eth_price_usd;
                
                // Lợi nhuận ròng
                let net_profit = sandwich_profit - total_gas_cost;
                profits.push(net_profit);
                total_profit += net_profit;
            } else {
                // Nếu thất bại, chúng ta phải trả phí gas mà không nhận được lợi nhuận
                let gas_limit = 250000; // gas limit cho giao dịch front-run
                
                // Chuyển đổi gas_price thành f64 an toàn hơn
                let gas_price_gwei = if gas_price.as_u128() > u128::MAX / 2 {
                    warn!("Gas price quá lớn, sử dụng giá trị an toàn");
                    50.0 // Giá trị mặc định an toàn, 50 Gwei
                } else {
                    (gas_price.as_u128() as f64) / 1e9
                };
                
                // Tính chi phí gas
                let gas_cost = (gas_price_gwei * gas_limit as f64) / eth_price_usd;
                
                // Trong trường hợp thất bại, chỉ mất chi phí front-run
                let net_profit = -gas_cost;
                profits.push(net_profit);
                total_profit += net_profit;
            }
        }
        
        // Tính toán các chỉ số
        let success_probability = successes as f64 / simulation_count as f64;
        let expected_profit = total_profit / simulation_count as f64;
        let worst_case_profit = profits.iter().copied().fold(f64::INFINITY, f64::min);
        let best_case_profit = profits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        
        Ok(SimulationMetrics {
            profit_potential: expected_profit,
            risk_level: 10.0 - (success_probability * 10.0), // Mức độ rủi ro (0-10)
            execution_time_ms: 250, // Ước lượng thời gian thực thi ms
            gas_used: 250000 * 2, // Gas cho front-run và back-run
            success_probability,
            expected_profit,
            worst_case_profit,
            best_case_profit,
            simulated_tx_count: simulation_count,
        })
    }
    
    // Quyết định xem có nên thực hiện front-run không
    pub async fn should_frontrun<A: ChainAdapter>(
        &self,
        opportunity: &SandwichOpportunity,
        strategy: &SimulationStrategy,
        adapter: &A
    ) -> bool {
        // Kiểm tra xem expected profit có lớn hơn ngưỡng tối thiểu không
        if strategy.expected_profit < self.config.min_profit_threshold {
            return false;
        }
        
        // Kiểm tra xem success probability có đủ cao không dựa trên risk tolerance
        let min_success_probability = match self.config.risk_tolerance {
            RiskLevel::VeryLow => 0.9,
            RiskLevel::Low => 0.8,
            RiskLevel::Medium => 0.7,
            RiskLevel::High => 0.6,
            RiskLevel::VeryHigh => 0.5,
        };
        
        strategy.success_probability >= min_success_probability
    }

    // Áp dụng chiến lược tối ưu vào tham số giao dịch
    pub fn apply_strategy_to_transaction(&self, 
                                        strategy: &FrontRunStrategy, 
                                        base_gas_price: U256) -> TransactionParameters {
        // Kiểm tra tham số đầu vào
        if strategy.gas_price_multiplier <= 0.0 {
            warn!("Gas price multiplier phải lớn hơn 0, đang sử dụng giá trị mặc định 1.0");
            return TransactionParameters {
                gas_price: base_gas_price,
                use_flashbots: strategy.use_flashbots,
                position_size: strategy.position_size,
                timing_delay: None,
                retry_count: 1,
            };
        }
        
        // Tính gas price an toàn hơn, tránh overflow
        let gas_price = if let Some(base_u64) = base_gas_price.as_u64().checked_mul(
            (strategy.gas_price_multiplier * 1_000_000.0) as u64
        ) {
            U256::from(base_u64 / 1_000_000)
        } else {
            warn!("Overflow khi tính gas price, sử dụng giá trị mặc định");
            base_gas_price
        };
        
        // Kiểm tra giá trị position_size
        let position_size = if strategy.position_size <= 0.0 || strategy.position_size > 1.0 {
            warn!("Position size không hợp lệ ({}), đang sử dụng giá trị mặc định 0.1", strategy.position_size);
            0.1
        } else {
            strategy.position_size
        };
        
        TransactionParameters {
            gas_price,
            use_flashbots: strategy.use_flashbots,
            position_size,
            timing_delay: if strategy.timing < 0.2 {
                None // Không delay
            } else {
                // Tính toán delay dựa trên timing parameter (ms)
                Some(Duration::from_millis((strategy.timing * 500.0) as u64))
            },
            retry_count: if strategy.strategy_type == StrategyType::Aggressive {
                3 // Aggressive retry nhiều hơn
            } else {
                1
            },
        }
    }
    
    // Ghi lại quyết định và kết quả
    pub fn record_decision(&mut self, decision: GameDecision) {
        // Thêm vào lịch sử
        self.decision_history.push_back(decision);
        
        // Giới hạn kích thước lịch sử
        while self.decision_history.len() > self.config.history_size {
            self.decision_history.pop_front();
        }
    }
    
    // Tính toán tỷ lệ thành công
    pub fn calculate_success_rate(&self) -> f64 {
        let decisions_with_outcome: Vec<_> = self.decision_history.iter()
            .filter(|d| d.outcome.is_some())
            .collect();
            
        if decisions_with_outcome.is_empty() {
            return 0.5; // Giá trị mặc định khi chưa có dữ liệu
        }
        
        let successful_count = decisions_with_outcome.iter()
            .filter(|d| d.outcome.as_ref().unwrap().success)
            .count();
            
        successful_count as f64 / decisions_with_outcome.len() as f64
    }

    fn update_decision_outcome(&mut self, updated_decision: &GameDecision) {
        for i in 0..self.decision_history.len() {
            let decision = &mut self.decision_history[i];
            if decision.opportunity_id == updated_decision.opportunity_id && 
               decision.timestamp == updated_decision.timestamp {
               *decision = updated_decision.clone();
               break;
            }
        }
    }

    // Tính toán ma trận tương quan
    fn calculate_correlation_matrix(&self, assets: &HashMap<String, TokenStatus>) -> HashMap<(String, String), f64> {
        let mut correlation_matrix = HashMap::new();
        let symbols: Vec<_> = assets.keys().cloned().collect();
        
        for i in 0..symbols.len() {
            for j in i..symbols.len() {
                let symbol_i = &symbols[i];
                let symbol_j = &symbols[j];
                
                if i == j {
                    correlation_matrix.insert((symbol_i.clone(), symbol_j.clone()), 1.0);
                } else {
                    let asset_i = &assets[symbol_i];
                    let asset_j = &assets[symbol_j];
                    
                    let correlation = if let (Some(prices_i), Some(prices_j)) = (&asset_i.price_history, &asset_j.price_history) {
                        self.calculate_correlation(prices_i, prices_j)
                    } else {
                        0.5 // Giá trị mặc định nếu không có dữ liệu
                    };
                    
                    correlation_matrix.insert((symbol_i.clone(), symbol_j.clone()), correlation);
                    correlation_matrix.insert((symbol_j.clone(), symbol_i.clone()), correlation);
                }
            }
        }
        
        correlation_matrix
    }

    // Tính toán tương quan giữa hai chuỗi giá
    fn calculate_correlation(&self, prices_a: &VecDeque<f64>, prices_b: &VecDeque<f64>) -> f64 {
        if prices_a.len() < 2 || prices_b.len() < 2 {
            return 0.0;
        }
        
        let min_len = prices_a.len().min(prices_b.len());
        
        // Tính giá trị trung bình
        let mean_a: f64 = prices_a.iter().take(min_len).sum::<f64>() / min_len as f64;
        let mean_b: f64 = prices_b.iter().take(min_len).sum::<f64>() / min_len as f64;
        
        // Tính tử số và mẫu số cho hệ số tương quan Pearson
        let mut numerator = 0.0;
        let mut denom_a = 0.0;
        let mut denom_b = 0.0;
        
        for i in 0..min_len {
            let a = prices_a[i] - mean_a;
            let b = prices_b[i] - mean_b;
            
            numerator += a * b;
            denom_a += a * a;
            denom_b += b * b;
        }
        
        // Tránh chia cho 0
        if denom_a.abs() < 1e-10 || denom_b.abs() < 1e-10 {
            return 0.0;
        }
        
        numerator / (denom_a.sqrt() * denom_b.sqrt())
    }

    // Tính volatility của danh mục
    fn calculate_portfolio_volatility(
        &self,
        assets: &HashMap<String, TokenStatus>,
        correlation_matrix: &HashMap<(String, String), f64>,
        allocations: &HashMap<String, f64>
    ) -> f64 {
        let mut variance = 0.0;
        
        // Tính phương sai danh mục dựa trên công thức:
        // Var(p) = Σᵢ Σⱼ wᵢ wⱼ σᵢ σⱼ ρᵢⱼ
        // Trong đó: wᵢ là trọng số, σᵢ là độ lệch chuẩn, ρᵢⱼ là hệ số tương quan
        
        for (symbol_i, asset_i) in assets {
            let weight_i = allocations.get(symbol_i).unwrap_or(&0.0);
            let volatility_i = asset_i.daily_volatility;
            
            for (symbol_j, asset_j) in assets {
                let weight_j = allocations.get(symbol_j).unwrap_or(&0.0);
                let volatility_j = asset_j.daily_volatility;
                
                let key = (symbol_i.clone(), symbol_j.clone());
                let correlation = correlation_matrix.get(&key).unwrap_or(&0.5);
                
                variance += weight_i * weight_j * volatility_i * volatility_j * correlation;
            }
        }
        
        variance.sqrt() // Volatility là căn bậc 2 của phương sai
    }

    // Cập nhật kết quả stress test
    fn update_stress_test_results(&mut self) -> Result<StressTestResults> {
        // Thực hiện các kịch bản stress test
        let equity_crash = self.simulate_scenario("Equity Crash", |returns| {
            returns * 3.0 // Giả định volatility tăng gấp 3
        })?;
        
        let liquidity_crisis = self.simulate_scenario("Liquidity Crisis", |returns| {
            returns * 2.5 - 0.1 // Volatility tăng 2.5 lần và trượt giá 10%
        })?;
        
        let crypto_winter = self.simulate_scenario("Crypto Winter", |returns| {
            returns * 2.0 - 0.2 // Volatility tăng 2 lần và trượt giá 20%
        })?;
        
        let results = StressTestResults {
            scenarios: vec![
                equity_crash,
                liquidity_crisis,
                crypto_winter
            ],
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        self.risk_report.stress_test_results = Some(results.clone());
        
        Ok(results)
    }

    // Mô phỏng một kịch bản stress test
    fn simulate_scenario<F>(&self, name: &str, shock_fn: F) -> Result<StressTestScenario>
    where
        F: Fn(f64) -> f64
    {
        let mut total_loss = 0.0;
        let mut asset_impacts = Vec::new();
        
        for (symbol, asset) in &self.current_portfolio.assets {
            let daily_var = asset.daily_volatility * 1.65; // 95% confidence
            let shocked_var = shock_fn(daily_var);
            
            let loss = shocked_var * asset.value_usd;
            total_loss += loss;
            
            asset_impacts.push(ScenarioAssetImpact {
                token_symbol: symbol.clone(),
                normal_var: daily_var * asset.value_usd,
                stressed_var: shocked_var * asset.value_usd,
                loss_percentage: shocked_var * 100.0,
            });
        }
        
        Ok(StressTestScenario {
            name: name.to_string(),
            total_portfolio_loss: total_loss,
            loss_percentage: if self.current_portfolio.total_value_usd > 0.0 {
                total_loss / self.current_portfolio.total_value_usd * 100.0
            } else {
                0.0
            },
            asset_impacts,
        })
    }

    pub async fn optimize_buy_parameters(&self, token_address: &str, base_amount: String) -> Result<OptimizedTradeParams> {
        // Phân tích mempool và thị trường
        let network_state = self.analyze_current_network_state().await?;
        
        // Thực hiện mô phỏng Monte Carlo
        let simulation_results = self.run_monte_carlo_simulation(
            token_address, 
            &base_amount,
            &network_state,
            1000 // Số lần mô phỏng
        ).await?;
        
        // Tìm chiến lược tối ưu từ kết quả mô phỏng
        let optimal_strategy = self.find_optimal_strategy(&simulation_results);
        
        // Tính toán tham số mua tối ưu
        let optimized_params = self.calculate_optimized_parameters(
            token_address,
            &base_amount,
            &optimal_strategy,
            &network_state
        ).await?;
        
        // Cập nhật vào bộ nhớ cache
        self.save_optimized_strategy_with_params(token_address, &optimal_strategy, &optimized_params).await?;
        
        Ok(optimized_params)
    }

    // Các chiến lược giao dịch khác cũng cần được tối ưu hóa:
    pub async fn optimize_sandwich_parameters(&self, token_address: &str, victim_tx: &PendingSwap) -> Result<SandwichParams> {
        // Tương tự như trên nhưng dành cho sandwich attack
        // ...
    }

    pub async fn optimize_frontrun_parameters(&self, token_address: &str, target_tx: &PendingSwap) -> Result<FrontrunParams> {
        // Tương tự như trên nhưng dành cho frontrun
        // ...
    }

    pub async fn process_ai_recommendation(&mut self, token_address: &str, ai_result: &AIAnalysisResult) -> Result<OptimizedStrategy, Box<dyn std::error::Error>> {
        // Phân tích điều kiện mạng
        let network_state = self.analyze_current_network_state().await?;
        
        // Phân tích các trader khác
        self.analyze_players(&self.chain_adapter).await?;
        
        // Tùy thuộc vào chiến lược được đề xuất, chạy mô phỏng khác nhau
        let strategy = match &ai_result.recommended_strategy {
            TradingStrategy::SandwichAttack => {
                if let Some(opportunity) = self.find_sandwich_opportunity(token_address).await? {
                    self.optimize_sandwich_attack(token_address, &opportunity, &network_state, ai_result.buy_confidence).await?
                } else {
                    return Err("Không tìm thấy cơ hội sandwich phù hợp".into());
                }
            },
            TradingStrategy::MempoolFrontrun => {
                if let Some(target_tx) = self.find_frontrun_target(token_address).await? {
                    self.optimize_frontrun_attack(token_address, &target_tx, &network_state, ai_result.buy_confidence).await?
                } else {
                    return Err("Không tìm thấy giao dịch phù hợp để frontrun".into());
                }
            },
            TradingStrategy::Accumulate | TradingStrategy::LimitedBuy => {
                let amount = self.calculate_optimal_position_size(token_address, ai_result.risk_reward_ratio).await?;
                self.optimize_direct_buy(token_address, amount, &network_state, ai_result.buy_confidence).await?
            },
            TradingStrategy::Monitor => {
                return Err("Không có chiến lược giao dịch được đề xuất".into());
            },
            _ => {
                return Err("Chiến lược không được hỗ trợ".into());
            }
        };
        
        // Lưu chiến lược đã được tối ưu hóa
        self.save_optimized_strategy(token_address, &strategy).await?;
        
        // Thông báo cho TradeManager
        if self.config.auto_execute && strategy.expected_profit > self.config.min_profit_threshold {
            self.send_to_trade_manager(token_address, &strategy).await?;
        }
        
        Ok(strategy)
    }

    async fn optimize_sandwich_attack(&self, token_address: &str, opportunity: &SandwichOpportunity, network_state: &NetworkState, ai_confidence: f64) -> Result<OptimizedStrategy, Box<dyn std::error::Error>> {
        // Tạo các kịch bản khác nhau để mô phỏng
        let mut scenarios = Vec::new();
        
        // Tạo các kịch bản với các gas multiplier khác nhau
        for gas_mult in [1.05, 1.1, 1.15, 1.2, 1.3, 1.4, 1.5].iter() {
            // Và các buy amount khác nhau (% của victim amount)
            for amount_percent in [20, 30, 40, 50, 60].iter() {
                scenarios.push(SandwichScenario {
                    token_address: token_address.to_string(),
                    victim_tx_hash: opportunity.victim_tx_hash.clone(),
                    entry_price: 0.0, // Sẽ được tính toán trong mô phỏng
                    entry_amount: 0.0, // Sẽ được tính toán trong mô phỏng
                    exit_price: 0.0, // Sẽ được tính toán trong mô phỏng
                    price_impact: opportunity.estimated_price_impact,
                    expected_profit: 0.0, // Sẽ được tính toán trong mô phỏng
                    front_gas_multiplier: *gas_mult,
                    back_gas_multiplier: *gas_mult - 0.05,
                    front_amount_percent: *amount_percent as f64,
                    use_flashbots: network_state.congestion_level > 7,
                });
            }
        }
        
        // Mô phỏng Monte Carlo cho mỗi kịch bản
        let mut simulation_results = Vec::new();
        
        for scenario in &scenarios {
            // Ước tính chi phí giao dịch
            let estimated_gas_cost = self.estimate_sandwich_gas_cost(
                scenario.front_gas_multiplier,
                scenario.back_gas_multiplier,
                network_state.base_fee
            ).await?;
            
            // Ước tính lợi nhuận tiềm năng
            let estimated_profit = self.estimate_sandwich_profit(
                token_address,
                opportunity,
                scenario.front_amount_percent,
                estimated_gas_cost
            ).await?;
            
            // Ước tính xác suất thành công
            let success_probability = self.estimate_success_probability(
                scenario.front_gas_multiplier,
                scenario.use_flashbots,
                network_state
            );
            
            // Tính toán lợi nhuận kỳ vọng
            let expected_profit = estimated_profit * success_probability;
            
            simulation_results.push(SimulationResult {
                scenario: scenario.clone(),
                expected_profit,
                estimated_gas_cost,
                success_probability,
                adjusted_score: expected_profit * (1.0 + ai_confidence * 0.2), // Điều chỉnh dựa trên độ tin cậy AI
            });
        }
        
        // Sắp xếp kết quả theo điểm
        simulation_results.sort_by(|a, b| b.adjusted_score.partial_cmp(&a.adjusted_score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Chọn kịch bản tốt nhất
        if let Some(best_result) = simulation_results.first() {
            if best_result.expected_profit > self.config.min_profit_threshold {
                // Tạo chiến lược tối ưu
                let params = SandwichParams {
                    victim_tx_hash: opportunity.victim_tx_hash.clone(),
                    front_run_amount_percent: best_result.scenario.front_amount_percent,
                    front_run_gas_multiplier: best_result.scenario.front_gas_multiplier,
                    back_run_gas_multiplier: best_result.scenario.back_gas_multiplier,
                    use_flashbots: best_result.scenario.use_flashbots,
                };
                
                return Ok(OptimizedStrategy {
                    token_address: token_address.to_string(),
                    strategy_type: StrategyType::Sandwich,
                    params: StrategyParams::Sandwich(params),
                    expected_profit: best_result.expected_profit,
                    success_probability: best_result.success_probability,
                    estimated_gas_cost: best_result.estimated_gas_cost,
                    ai_confidence,
                    timestamp: utils::safe_now(),
                });
            }
        }
        
        Err("Không tìm thấy chiến lược sandwich có lợi".into())
    }

    async fn send_to_trade_manager(&self, token_address: &str, strategy: &OptimizedStrategy) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(trade_manager) = &self.trade_manager {
            let mut trade_manager = trade_manager.lock().await;
            trade_manager.execute_optimized_strategy(strategy).await?;
        }
        
        Ok(())
    }

    pub async fn decide_profit_taking_or_continue_sandwich(&self, token_address: &str, 
                                                          current_position: &TokenPosition) 
                                                          -> Result<ProfitDecision, Box<dyn std::error::Error>> {
        // Phân tích điều kiện thị trường hiện tại
        let market_conditions = self.analyze_current_market_state(token_address).await?;
        
        // Phân tích hoạt động mempool
        let mempool_activity = self.analyze_token_mempool_activity(token_address).await?;
        
        // Phân tích hành vi đối thủ
        let competitors = self.analyze_competitor_activity(token_address).await?;
        
        // Đánh giá vị thế hiện tại
        let position_metrics = self.evaluate_position_metrics(current_position).await?;
        
        // Tạo các kịch bản quyết định
        let mut decision_scenarios = Vec::new();
        
        // 1. Kịch bản chốt lời ngay lập tức
        let take_profit_now = ProfitScenario {
            action: ProfitAction::TakeProfit,
            expected_profit: position_metrics.current_profit_usd,
            probability_success: 1.0, // Chắc chắn khi bán ngay
            risk_factor: 0.0,  // Không có rủi ro khi bán ngay
            time_horizon: 0,   // Ngay lập tức
            score: 0.0,
        };
        decision_scenarios.push(take_profit_now);
        
        // 2. Kịch bản chờ đợt tăng giá tiếp theo (4 giờ)
        let estimated_future_price = self.estimate_future_price(
            token_address, 
            position_metrics.current_price, 
            4 * 3600 // 4 giờ
        ).await?;
        
        let wait_scenario = ProfitScenario {
            action: ProfitAction::HoldForPriceTarget {
                target_price: estimated_future_price * 1.1, // Mục tiêu tăng 10%
                time_limit_seconds: 4 * 3600, // Tối đa 4 giờ
            },
            expected_profit: position_metrics.current_profit_usd * 1.1, // Dự kiến tăng 10%
            probability_success: self.calculate_price_target_probability(
                token_address,
                estimated_future_price * 1.1,
                4 * 3600
            ).await?,
            risk_factor: market_conditions.volatility * 0.5, // Rủi ro tỷ lệ với độ biến động
            time_horizon: 4 * 3600,
            score: 0.0,
        };
        decision_scenarios.push(wait_scenario);
        
        // 3. Kịch bản tiếp tục sandwich với token này
        if mempool_activity.pending_buy_count > 0 && position_metrics.holding_time_seconds < 24 * 3600 {
            let continue_sandwich = ProfitScenario {
                action: ProfitAction::ContinueSandwich {
                    max_additional_buys: mempool_activity.potential_victims.len() as u32,
                    max_time_seconds: 12 * 3600, // Tối đa 12 giờ
                },
                expected_profit: self.estimate_continued_sandwich_profit(
                    token_address,
                    position_metrics.current_profit_usd,
                    mempool_activity.potential_victims.len(),
                    competitors.sandwich_bot_count
                ).await?,
                probability_success: self.calculate_sandwich_success_probability(
                    mempool_activity.potential_victims.len(),
                    competitors.sandwich_bot_count,
                    market_conditions.congestion_level
                ).await?,
                risk_factor: market_conditions.volatility * 0.8 + 
                            (competitors.sandwich_bot_count as f64 * 0.05),
                time_horizon: 12 * 3600,
                score: 0.0,
            };
            decision_scenarios.push(continue_sandwich);
        }
        
        // 4. Kịch bản tiếp tục DCA (nếu token có tiềm năng dài hạn)
        if position_metrics.token_score > 70 && market_conditions.trend == MarketTrend::Uptrend {
            let dca_scenario = ProfitScenario {
                action: ProfitAction::DCABuy {
                    additional_amount_percent: 50, // Tăng thêm 50% vị thế
                    intervals: 3,                   // Chia làm 3 đợt
                    time_frame_seconds: 72 * 3600,  // Trong 3 ngày
                },
                expected_profit: position_metrics.current_profit_usd * 1.5, // Dự kiến tăng 50%
                probability_success: 0.65, // Xác suất thành công trung bình
                risk_factor: market_conditions.volatility * 1.2, // Rủi ro cao hơn do thời gian dài
                time_horizon: 72 * 3600,
                score: 0.0,
            };
            decision_scenarios.push(dca_scenario);
        }
        
        // Điều chỉnh các kịch bản dựa trên dữ liệu lịch sử AI
        self.apply_ai_adjustment_to_scenarios(&mut decision_scenarios, token_address).await?;
        
        // Tính điểm cho từng kịch bản
        for scenario in &mut decision_scenarios {
            let time_factor = match position_metrics.holding_time_seconds {
                t if t < 3600 => 0.8,       // < 1 giờ: ưu tiên tiếp tục nắm giữ
                t if t < 24 * 3600 => 1.0,  // 1-24 giờ: trung lập
                t if t < 72 * 3600 => 1.2,  // 1-3 ngày: ưu tiên chốt lời
                _ => 1.5,                   // > 3 ngày: ưu tiên chốt lời mạnh
            };
            
            // Tính điểm chính cho kịch bản
            scenario.score = (scenario.expected_profit * scenario.probability_success * 
                             (1.0 - scenario.risk_factor * 0.5) * time_factor) / 
                             (1.0 + scenario.time_horizon as f64 / 86400.0); // Điều chỉnh theo thời gian
        }
        
        // Sắp xếp kịch bản theo điểm giảm dần
        decision_scenarios.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Chọn kịch bản tốt nhất
        if let Some(best_scenario) = decision_scenarios.first() {
            let decision = ProfitDecision {
                token_address: token_address.to_string(),
                recommended_action: best_scenario.action.clone(),
                expected_profit: best_scenario.expected_profit,
                reasoning: format!(
                    "Quyết định {} với điểm {:.2}. Lợi nhuận kỳ vọng: ${:.2}, \
                     Xác suất thành công: {:.1}%, Rủi ro: {:.1}%, \
                     Điều kiện thị trường: {}, Hoạt động mempool: {} giao dịch tiềm năng",
                    best_scenario.action.to_string(),
                    best_scenario.score,
                    best_scenario.expected_profit,
                    best_scenario.probability_success * 100.0,
                    best_scenario.risk_factor * 100.0,
                    market_conditions.trend.to_string(),
                    mempool_activity.potential_victims.len()
                ),
                all_scenarios: decision_scenarios.clone(),
                current_token_price: position_metrics.current_price,
                current_profit_usd: position_metrics.current_profit_usd,
                decision_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            };
            
            // Ghi lại quyết định vào lịch sử để phân tích sau này
            self.record_profit_decision(decision.clone()).await?;
            
            return Ok(decision);
        }
        
        Err("Không thể đưa ra quyết định chốt lời".into())
    }

    // Phân tích hoạt động mempool cho một token cụ thể
    async fn analyze_token_mempool_activity(&self, token_address: &str) -> Result<MempoolTokenActivity, Box<dyn std::error::Error>> {
        // Tạo đối tượng activity với giá trị mặc định
        let timestamp = utils::safe_now();
        
        let mut activity = MempoolTokenActivity {
            token_address: token_address.to_string(),
            pending_buy_count: 0,
            pending_sell_count: 0,
            average_buy_size_usd: 0.0,
            average_sell_size_usd: 0.0,
            potential_victims: Vec::new(),
            last_analyzed: timestamp,
        };
        
        // Tránh race condition bằng cách sao lưu mempool_tracker và xử lý an toàn
        if let Some(mempool_tracker) = &self.mempool_tracker {
            // Sử dụng một pattern an toàn hơn với timeout
            match tokio::time::timeout(
                Duration::from_secs(2),
                async {
                    // Thử lấy khóa mà không chặn
                    match mempool_tracker.try_lock() {
                        Ok(tracker) => {
                            // Lấy bản sao dữ liệu để xử lý bên ngoài critical section
                            let pending_swaps = if let Some(swaps) = tracker.pending_swaps.get(token_address) {
                                swaps.clone()
                            } else {
                                Vec::new()
                            };
                            
                            // Trả về dữ liệu và drop lock ngay lập tức
                            Ok(pending_swaps)
                        },
                        Err(_) => {
                            warn!("Không thể lấy mempool_tracker lock trong analyze_token_mempool_activity, đang bận");
                            Ok(Vec::new()) // Trả về danh sách trống nếu không thể lấy lock
                        }
                    }
                }
            ).await {
                Ok(Ok(pending_swaps)) => {
                    // Phân tích dữ liệu ở ngoài critical section
                    let mut buy_count = 0;
                    let mut sell_count = 0;
                    let mut total_buy_amount = 0.0;
                    let mut total_sell_amount = 0.0;
                    
                    for swap in &pending_swaps {
                        if swap.is_buy {
                            buy_count += 1;
                            total_buy_amount += swap.amount_usd;
                            
                            // Thêm vào danh sách potential victims
                            if swap.amount_usd > 100.0 { // Chỉ quan tâm giao dịch > $100
                                activity.potential_victims.push(PotentialVictim {
                                    tx_hash: swap.tx_hash.clone(),
                                    amount_usd: swap.amount_usd,
                                    gas_price: swap.gas_price,
                                    timestamp: swap.timestamp,
                                });
                            }
                        } else {
                            sell_count += 1;
                            total_sell_amount += swap.amount_usd;
                        }
                    }
                    
                    // Cập nhật kết quả
                    activity.pending_buy_count = buy_count;
                    activity.pending_sell_count = sell_count;
                    activity.average_buy_size_usd = if buy_count > 0 { total_buy_amount / buy_count as f64 } else { 0.0 };
                    activity.average_sell_size_usd = if sell_count > 0 { total_sell_amount / sell_count as f64 } else { 0.0 };
                },
                Ok(Err(e)) => {
                    warn!("Lỗi khi xử lý mempool data: {}", e);
                },
                Err(_) => {
                    warn!("Timeout khi lấy mempool data cho token {}", token_address);
                }
            }
        }
        
        Ok(activity)
    }

    // Phân tích hành vi đối thủ
    async fn analyze_competitor_activity(&self, token_address: &str) -> Result<CompetitorAnalysis, Box<dyn std::error::Error>> {
        // Lấy timestamp an toàn
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => {
                warn!("Lỗi khi lấy thời gian hệ thống trong analyze_competitor_activity");
                0 // Giá trị mặc định
            }
        };
        
        let mut analysis = CompetitorAnalysis {
            token_address: token_address.to_string(),
            active_mev_bots: 0,
            sandwich_bot_count: 0,
            frontrun_bot_count: 0,
            arbitrage_bot_count: 0,
            average_gas_multiplier: 1.0,
            competitors_aggression: 0.0,
            last_analyzed: timestamp,
        };
        
        // Sao chép dữ liệu để tránh race condition và không cần giữ lock trong quá trình xử lý
        let players_snapshot: HashMap<String, NetworkPlayer>;
        let recent_blocks_snapshot: Vec<BlockInfo>;
        
        // Clone players với timeout để tránh deadlock
        players_snapshot = self.players.clone();
        
        // Clone recent_blocks với xử lý lỗi phù hợp
        recent_blocks_snapshot = match &self.recent_blocks {
            Some(blocks) => blocks.clone(),
            None => {
                debug!("Không có dữ liệu recent_blocks cho token {}", token_address);
                return Ok(analysis);
            }
        };
        
        // Phân tích các block gần đây để xác định MEV bot
        let mut mev_bot_addresses = HashSet::new();
        let mut sandwich_bots = HashSet::new();
        let mut frontrun_bots = HashSet::new();
        let mut arbitrage_bots = HashSet::new();
        let mut total_gas_multiplier = 0.0;
        let mut gas_multiplier_count = 0;
        
        // Tối ưu: Lấy các tx_hash cần xử lý ra khỏi các block
        let tx_hash_vec: Vec<_> = recent_blocks_snapshot.iter()
            .flat_map(|block| block.successful_mev_txs.iter().cloned())
            .collect();
        
        // Xử lý đồng thời các giao dịch nhưng giới hạn số lượng task đồng thời
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10)); // Giới hạn 10 task đồng thời
        let (tx_send, mut tx_recv) = tokio::sync::mpsc::channel(100);
        
        let mut tasks = Vec::new();
        
        for tx_hash in tx_hash_vec {
            let tx_hash_str = format!("{:?}", tx_hash);
            let players_clone = players_snapshot.clone();
            let tx_send = tx_send.clone();
            let semaphore_clone = semaphore.clone();
            
            // Spawn task có kiểm soát đồng thời
            let task = tokio::spawn(async move {
                // Lấy permit để giới hạn số lượng task đồng thời
                let _permit = match semaphore_clone.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        warn!("Không thể lấy permit từ semaphore: {}", e);
                        return;
                    }
                };
                
                if let Some(player) = players_clone.get(&tx_hash_str) {
                    // Phân tích và trả kết quả
                    let strategy = match player.estimated_gas_strategy {
                        GasStrategy::Conservative => 0,
                        GasStrategy::Moderate => 1,
                        GasStrategy::Aggressive => 2,
                        GasStrategy::VeryAggressive => 3,
                        _ => -1,
                    };
                    
                    if let Err(e) = tx_send.send((player.clone(), strategy)).await {
                        warn!("Không thể gửi kết quả phân tích: {}", e);
                    }
                }
                // permit tự động drop khi kết thúc scope
            });
            
            tasks.push(task);
        }
        
        // Đóng kênh gửi để signal hoàn thành sau khi tất cả các task đã được spawn
        drop(tx_send);
        
        // Đợi tất cả các task hoàn thành với timeout để tránh treo
        match tokio::time::timeout(Duration::from_secs(5), futures::future::join_all(tasks)).await {
            Ok(_) => {
                debug!("Tất cả các task phân tích hoàn thành");
            },
            Err(_) => {
                warn!("Timeout khi đợi các task phân tích hoàn thành");
            }
        }
        
        // Thu thập kết quả
        while let Some((player, strategy_type)) = tx_recv.recv().await {
            // Chỉ xử lý nếu giao dịch gần đây (7 ngày)
            let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_secs(),
                Err(_) => timestamp, // Sử dụng timestamp đã tạo trước đó nếu có lỗi
            };
            
            if now.saturating_sub(player.last_seen) < 7 * 24 * 60 * 60 { // Sử dụng saturating_sub để tránh overflow
                if !player.recent_transactions.is_empty() {
                    mev_bot_addresses.insert(player.recent_transactions[0].tx_hash);
                    
                    // Phân loại bot theo chiến lược
                    match strategy_type {
                        0 | 1 => { 
                            // Conservative hoặc Moderate thường là arbitrage
                            arbitrage_bots.insert(player.recent_transactions[0].tx_hash);
                        },
                        2 => {
                            // Aggressive thường là frontrun 
                            frontrun_bots.insert(player.recent_transactions[0].tx_hash);
                        },
                        3 => {
                            // VeryAggressive thường là sandwich 
                            sandwich_bots.insert(player.recent_transactions[0].tx_hash);
                        },
                        _ => {}
                    }
                    
                    // Tính trung bình gas multiplier an toàn
                    if !player.recent_transactions.is_empty() {
                        let avg_gas = player.recent_transactions.iter()
                            .filter(|tx| tx.gas_price > U256::zero())
                            .map(|tx| {
                                // Chuyển đổi an toàn từ U256 sang f64
                                let gas_u128 = tx.gas_price.as_u128();
                                if gas_u128 > u64::MAX as u128 {
                                    warn!("Gas price vượt quá giới hạn u64: {}", gas_u128);
                                    0.0
                                } else {
                                    gas_u128 as f64
                                }
                            })
                            .sum::<f64>() / player.recent_transactions.len().max(1) as f64;
                        
                        if !avg_gas.is_nan() && !avg_gas.is_infinite() {
                            total_gas_multiplier += avg_gas;
                            gas_multiplier_count += 1;
                        }
                    }
                }
            }
        }
        
        // Cập nhật kết quả phân tích
        analysis.active_mev_bots = mev_bot_addresses.len();
        analysis.sandwich_bot_count = sandwich_bots.len();
        analysis.frontrun_bot_count = frontrun_bots.len();
        analysis.arbitrage_bot_count = arbitrage_bots.len();
        
        if gas_multiplier_count > 0 {
            analysis.average_gas_multiplier = total_gas_multiplier / gas_multiplier_count as f64;
        }
        
        // Tính mức độ cạnh tranh
        let total_bots = analysis.active_mev_bots.max(1);
        let sandwich_percentage = analysis.sandwich_bot_count as f64 / total_bots as f64;
        let frontrun_percentage = analysis.frontrun_bot_count as f64 / total_bots as f64;
        
        // Cạnh tranh cao hơn khi có nhiều bot sandwich và frontrun
        analysis.competitors_aggression = 
            (sandwich_percentage * 0.7 + frontrun_percentage * 0.3) * // Trọng số sandwich cao hơn
            (1.0 + (analysis.average_gas_multiplier / 100.0).min(5.0)) * // Hệ số nhân dựa trên gas
            (1.0 + (analysis.active_mev_bots as f64 / 10.0).min(1.0)); // Hệ số nhân dựa trên số lượng bot
        
        Ok(analysis)
    }

    // Áp dụng điều chỉnh AI vào các kịch bản
    async fn apply_ai_adjustment_to_scenarios(&self, scenarios: &mut Vec<ProfitScenario>, token_address: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ai_module) = &self.ai_module {
            let ai = ai_module.lock().await;
            
            // Lấy phân tích AI mới nhất cho token
            if let Some(analysis) = ai.get_latest_token_analysis(token_address) {
                for scenario in scenarios.iter_mut() {
                    match &scenario.action {
                        ProfitAction::TakeProfit => {
                            if analysis.price_change_prediction < 0.0 {
                                // Nếu AI dự đoán giá giảm, tăng điểm cho việc chốt lời
                                scenario.probability_success *= 1.1;
                                scenario.expected_profit *= 1.05;
                            }
                        },
                        ProfitAction::HoldForPriceTarget { .. } => {
                            if analysis.price_change_prediction > 0.0 {
                                // Nếu AI dự đoán giá tăng, tăng điểm cho việc nắm giữ
                                scenario.probability_success *= 1.0 + (analysis.price_change_prediction.min(30.0) / 100.0);
                                scenario.expected_profit *= 1.0 + (analysis.price_change_prediction.min(30.0) / 200.0);
                            } else {
                                // Nếu AI dự đoán giá giảm, giảm điểm cho việc nắm giữ
                                scenario.probability_success *= 0.9;
                                scenario.risk_factor *= 1.1;
                            }
                        },
                        ProfitAction::ContinueSandwich { .. } => {
                            if analysis.prediction_type == "sandwich_opportunity" {
                                // Nếu AI phát hiện cơ hội sandwich, tăng điểm
                                scenario.probability_success *= 1.0 + (analysis.buy_confidence.min(0.95) - 0.5).max(0.0);
                                scenario.expected_profit *= 1.0 + (analysis.buy_confidence.min(0.95) - 0.5).max(0.0) / 2.0;
                            }
                        },
                        ProfitAction::DCABuy { .. } => {
                            if analysis.prediction_type == "pump_potential" {
                                // Nếu AI phát hiện tiềm năng tăng giá, tăng điểm cho DCA
                                scenario.probability_success *= 1.0 + (analysis.buy_confidence.min(0.95) - 0.5).max(0.0);
                                scenario.expected_profit *= 1.0 + (analysis.buy_confidence.min(0.95) - 0.5).max(0.0);
                                scenario.risk_factor *= 0.9;
                            }
                        },
                    }
                }
            }
        }
        
        Ok(())
    }

    // Tính xác suất đạt mục tiêu giá
    async fn calculate_price_target_probability(&self, token_address: &str, target_price: f64, time_horizon_seconds: u64) -> Result<f64, Box<dyn std::error::Error>> {
        // Phân tích lịch sử giá gần đây
        let price_history = self.chain_adapter.get_token_price_history(token_address, 48).await?; // 48 giờ gần nhất
        
        if price_history.is_empty() {
            return Ok(0.5); // Mặc định nếu không có dữ liệu
        }
        
        // Tính toán độ biến động lịch sử
        let mut price_changes = Vec::new();
        for i in 1..price_history.len() {
            let previous = price_history[i-1].1;
            let current = price_history[i].1;
            if previous > 0.0 {
                let percent_change = (current - previous) / previous;
                price_changes.push(percent_change);
            }
        }
        
        if price_changes.is_empty() {
            return Ok(0.5); // Mặc định nếu không có dữ liệu thay đổi
        }
        
        // Tính độ lệch chuẩn và trung bình
        let mean: f64 = price_changes.iter().sum::<f64>() / price_changes.len() as f64;
        let variance: f64 = price_changes.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / price_changes.len() as f64;
        let std_dev = variance.sqrt();
        
        // Tính số giờ từ thời gian hiện tại
        let hours = time_horizon_seconds as f64 / 3600.0;
        
        // Sử dụng mô hình phân phối chuẩn để ước tính xác suất
        let current_price = price_history.last().unwrap().1;
        let percent_change_needed = (target_price / current_price) - 1.0;
        
        // Điều chỉnh theo căn thời gian (theo mô hình Wiener)
        let adjusted_std_dev = std_dev * (hours / 24.0).sqrt();
        
        // Tính z-score cho mục tiêu
        let z_score = (percent_change_needed - mean * (hours / 24.0)) / adjusted_std_dev;
        
        // Tính xác suất từ z-score (sử dụng phân phối chuẩn)
        let probability = 1.0 - StatNormal::new(0.0, 1.0)
            .map_err(|_| "Lỗi khi tạo phân phối chuẩn")?
            .cdf(z_score);
        
        Ok(probability)
    }

    // Ước tính lợi nhuận nếu tiếp tục sandwich
    async fn estimate_continued_sandwich_profit(&self, token_address: &str, current_profit: f64, 
                                              victim_count: usize, competitor_count: usize) 
                                              -> Result<f64, Box<dyn std::error::Error>> {
        if victim_count == 0 {
            return Ok(current_profit); // Không có victim mới
        }
        
        // Phân tích lịch sử sandwich gần đây
        let mut recent_profit_multipliers = Vec::new();
        
        if let Some(performance_tracker) = &self.performance_tracker {
            let tracker = performance_tracker.lock().await;
            if let Some(token_performance) = tracker.get_token_performance(token_address) {
                // Lấy lịch sử giao dịch sandwich gần đây
                let sandwich_results = token_performance.filter_by_strategy(StrategyType::Sandwich);
                
                for result in sandwich_results {
                    if result.success && result.profit_usd > 0.0 {
                        let multiplier = result.profit_usd / result.initial_investment_usd;
                        recent_profit_multipliers.push(multiplier);
                    }
                }
            }
        }
        
        // Xác định multiplier trung bình
        let avg_multiplier = if !recent_profit_multipliers.is_empty() {
            recent_profit_multipliers.iter().sum::<f64>() / recent_profit_multipliers.len() as f64
        } else {
            0.05 // Mặc định 5% lợi nhuận nếu không có dữ liệu
        };
        
        // Điều chỉnh theo số lượng competitor
        let competitor_factor = 1.0 / (1.0 + competitor_count as f64 * 0.2);
        
        // Ước tính lợi nhuận bổ sung
        let additional_profit = current_profit * avg_multiplier * competitor_factor * 
                               (victim_count as f64).min(3.0); // Giới hạn tối đa 3 victim
        
        // Tổng lợi nhuận kỳ vọng
        let total_expected_profit = current_profit + additional_profit;
        
        Ok(total_expected_profit)
    }

    // Phương thức thực thi quyết định lợi nhuận
    pub async fn execute_profit_decision(&self, decision: &ProfitDecision) -> Result<ExecutionResult, Box<dyn std::error::Error>> {
        if let Some(trade_manager) = &self.trade_manager {
            let mut trade_manager = trade_manager.lock().await;
            
            match &decision.recommended_action {
                ProfitAction::TakeProfit => {
                    // Thực hiện bán toàn bộ token để chốt lời
                    let sell_result = trade_manager.sell_all_token(&decision.token_address).await?;
                    
                    return Ok(ExecutionResult {
                        action_executed: ProfitAction::TakeProfit,
                        success: sell_result.success,
                        transaction_hash: Some(sell_result.tx_hash),
                        profit_usd: sell_result.profit_usd,
                        execution_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        error_message: None,
                    });
                },
                
                ProfitAction::HoldForPriceTarget { target_price, time_limit_seconds } => {
                    // Thiết lập đơn hàng giới hạn tự động
                    let order_id = trade_manager.create_limit_order(
                        &decision.token_address,
                        OrderType::SellLimit,
                        *target_price,
                        100, // 100% vị thế
                        *time_limit_seconds
                    ).await?;
                    
                    return Ok(ExecutionResult {
                        action_executed: decision.recommended_action.clone(),
                        success: true,
                        transaction_hash: None, // Chưa có giao dịch
                        profit_usd: 0.0, // Chưa thực hiện
                        execution_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        error_message: None,
                    });
                },
                
                ProfitAction::ContinueSandwich { max_additional_buys, max_time_seconds } => {
                    // Kích hoạt chế độ sandwich tự động
                    trade_manager.enable_auto_sandwich(
                        &decision.token_address,
                        *max_additional_buys,
                        *max_time_seconds
                    ).await?;
                    
                    return Ok(ExecutionResult {
                        action_executed: decision.recommended_action.clone(),
                        success: true,
                        transaction_hash: None, // Chưa có giao dịch
                        profit_usd: 0.0, // Chưa thực hiện
                        execution_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        error_message: None,
                    });
                },
                
                ProfitAction::DCABuy { additional_amount_percent, intervals, time_frame_seconds } => {
                    // Tạo lịch trình DCA
                    let current_position = trade_manager.get_token_position(&decision.token_address)?;
                    let additional_amount = current_position.position_value_usd * (*additional_amount_percent as f64 / 100.0);
                    
                    let dca_schedule = trade_manager.create_dca_schedule(
                        &decision.token_address,
                        additional_amount,
                        *intervals,
                        *time_frame_seconds
                    ).await?;
                    
                    return Ok(ExecutionResult {
                        action_executed: decision.recommended_action.clone(),
                        success: true,
                        transaction_hash: None, // Chưa có giao dịch
                        profit_usd: 0.0, // Chưa thực hiện
                        execution_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        error_message: None,
                    });
                },
            }
        }
        
        Err("Không thể thực thi quyết định: TradeManager không có sẵn".into())
    }

    pub async fn analyze_current_network_state(&self) -> Result<NetworkState> {
        Ok(NetworkState {
            gas_price: 50_000_000_000,
            congestion_level: 5,
            block_time: 12.0,
            pending_tx_count: 100,
            base_fee: ethers::types::U256::from(20_000_000_000u64),
        })
    }

    /// Phát hiện giao dịch MEV dựa trên heuristics
    pub fn detect_mev_transaction(&self, tx: &Transaction) -> bool {
        // Kiểm tra trước cấu hình
        if !self.config.mev_detection_enabled {
            return false; // Bỏ qua phát hiện nếu không được cấu hình
        }

        // Điểm đánh giá MEV
        let mut mev_score = 0;
        
        // Kiểm tra trước tiên nếu giao dịch là một giao dịch MEV đã biết
        let known_mev = self.known_mev_transactions.read().unwrap_or_else(|_| {
            warn!("Không thể lấy read lock cho known_mev_transactions");
            Vec::new().into()
        });
        
        if known_mev.contains(&tx.hash) {
            return true;
        }
        
        // Danh sách các địa chỉ MEV bot đã biết
        const KNOWN_MEV_BOTS: [&str; 5] = [
            "0x00000000003b3cc22af3ae1eac0440bcee416b40",
            "0x000000000dfde7deaf24138722987c9a6991e2d4",
            "0x000000a52a03835517e9d193b3c27626e1bc96b1",
            "0xb4a81261b16b92af0b9f7c4a83f1e885132d81e4",
            "0xae2ebf7d5efe0e3c1a2082a1e9fd63912c42c2f5"
        ];
        
        // Kiểm tra nếu giao dịch đến từ MEV bot đã biết
        if let Some(from) = tx.from {
            let from_str = format!("{:?}", from).to_lowercase();
            for mev_bot in &KNOWN_MEV_BOTS {
                if from_str.contains(mev_bot) {
                    mev_score += 20; // Cộng điểm cao nếu là bot đã biết
                    break;
                }
            }
        }

        // Kiểm tra giao dịch sandwich
        if let Some(ref mempool) = self.mempool_tracker {
            if let Ok(opportunities) = mempool.sandwich_opportunities.try_read() {
                for opportunity in opportunities.iter() {
                    // Nếu giao dịch nằm trong cơ hội sandwich
                    if opportunity.related_transactions.contains(&tx.hash) {
                        mev_score += 10;
                        break;
                    }
                }
            } else {
                warn!("Không thể lấy read lock cho sandwich_opportunities");
            }
        }

        // Kiểm tra frontrunning (dựa trên gas price cao bất thường)
        let avg_gas_price = if let Ok(stats) = self.chain_stats.try_read() {
            stats.average_gas_price
        } else {
            warn!("Không thể lấy read lock cho chain_stats");
            // Sử dụng giá gas mặc định nếu không thể lấy dữ liệu
            U256::from(50_000_000_000u64) // 50 gwei
        };
        
        if let Some(gas_price) = tx.gas_price {
            if gas_price > avg_gas_price * 3 {
                mev_score += 15; // Gas cao gấp 3 lần trung bình
            } else if gas_price > avg_gas_price * 2 {
                mev_score += 10; // Gas cao gấp 2 lần trung bình
            } else if gas_price > avg_gas_price * 5 / 3 {
                mev_score += 5; // Gas cao gấp 1.67 lần trung bình
            }
        }

        // Kiểm tra giao dịch arbitrage
        if let Some(ref mempool) = self.mempool_tracker {
            if let Ok(opportunities) = mempool.arbitrage_opportunities.try_read() {
                for opportunity in opportunities.iter() {
                    if opportunity.transaction_hash == tx.hash {
                        mev_score += 10;
                        break;
                    }
                }
            } else {
                warn!("Không thể lấy read lock cho arbitrage_opportunities");
            }
        }
        
        // Kiểm tra input data đặc trưng của MEV
        if let Some(input) = &tx.input {
            let input_str = hex::encode(input.to_vec());
            
            // Kiểm tra các mẫu đặc trưng trong dữ liệu input
            // Ví dụ: sandwitch attack thường có nhiều lệnh swap liên tiếp
            if input_str.contains("0x38ed1739") && input_str.contains("0x8803dbee") {
                mev_score += 5; // Có thể là sandwich attack
            }
            
            // Kiểm tra độ dài input (MEV transaction thường có input phức tạp)
            if input.len() > 1000 {
                mev_score += 5;
            }
        }
        
        // Kiểm tra nếu giao dịch có liên quan đến các giao dịch khác trong mempool
        if let Some(ref mempool) = self.mempool_tracker {
            if let Ok(pending_txs) = mempool.transactions.try_read() {
                let mut related_count = 0;
                
                for (_, tx_data) in pending_txs.iter() {
                    if tx_data.related_transactions.contains(&tx.hash) {
                        related_count += 1;
                    }
                }
                
                if related_count >= 3 {
                    mev_score += 10; // Nhiều giao dịch liên quan
                } else if related_count >= 1 {
                    mev_score += 5; // Ít nhất một giao dịch liên quan
                }
            } else {
                warn!("Không thể lấy read lock cho pending transactions");
            }
        }

        // Quyết định dựa trên tổng điểm
        let is_mev = mev_score >= 15; // Ngưỡng phát hiện MEV

        // Nếu là MEV, lưu vào danh sách đã biết
        if is_mev {
            if let Ok(mut known_mevs) = self.known_mev_transactions.try_write() {
                known_mevs.insert(tx.hash);
                
                // Giới hạn kích thước danh sách để tránh tràn bộ nhớ
                if known_mevs.len() > 1000 {
                    // Xóa các mục cũ nhất (chúng ta giả định iterator theo thứ tự chèn)
                    while known_mevs.len() > 800 {
                        if let Some(oldest) = known_mevs.iter().next().cloned() {
                            known_mevs.remove(&oldest);
                        } else {
                            break;
                        }
                    }
                }
            } else {
                warn!("Không thể lấy write lock cho known_mev_transactions");
            }
        }

        debug!("Phát hiện MEV: {}, Điểm: {}, Hash: {:?}", is_mev, mev_score, tx.hash);
        is_mev
    }
}

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

// MarketTrend enum để biểu diễn xu hướng thị trường
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketTrend {
    Uptrend,
    Downtrend,
    Sideways,
}

// Điều kiện thị trường
#[derive(Debug, Clone)]
pub struct MarketConditions {
    pub trend: MarketTrend,
    pub volatility: f64,
    pub liquidity: f64,
    pub competition: f64,
    pub gas_price: f64,
}

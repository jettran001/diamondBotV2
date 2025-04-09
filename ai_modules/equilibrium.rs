// External imports
use ethers::prelude::*;
use ethers::providers::{Http, Provider, Middleware};
use ethers::types::{U256, H256, Address};

// Standard library imports
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use std::sync::{Mutex, RwLock};

// Internal imports
use crate::trade::trade_logic::GameConfig;

// Third party imports
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use log::{info, warn, debug, error};
use rand::prelude::*;
use serde::{Serialize, Deserialize};

/// Một người chơi trong hệ thống game theory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    /// Địa chỉ của người chơi
    pub address: Address,
    /// Chiến lược hiện tại
    pub strategy: Strategy,
    /// Lịch sử hành động
    pub action_history: Vec<Action>,
    /// Điểm lợi ích (utility)
    pub utility: f64,
}

/// Chiến lược mà người chơi có thể sử dụng
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Strategy {
    /// Luôn cộng tác (Cooperate)
    AlwaysCooperate,
    /// Luôn phản bội (Defect)
    AlwaysDefect,
    /// Tit-for-Tat: Bắt đầu hợp tác, sau đó làm theo hành động gần nhất của đối thủ
    TitForTat,
    /// Phản ứng ngẫu nhiên
    Random,
    /// Tối ưu hóa lợi nhuận (Profit Maximizing)
    ProfitMaximizing,
    /// Chiến lược tùy chỉnh
    Custom(String),
}

/// Hành động của người chơi
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Action {
    /// Hợp tác
    Cooperate,
    /// Phản bội
    Defect,
    /// Không hành động
    NoAction,
}

/// Kết quả của một ván game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameOutcome {
    /// Thành công hay thất bại
    pub success: bool,
    /// Lợi nhuận (có thể âm nếu thất bại)
    pub profit: f64,
    /// Chi phí gas
    pub gas_cost: f64,
    /// Hành động của các người chơi
    pub actions: HashMap<Address, Action>,
    /// Lợi ích của các người chơi
    pub utilities: HashMap<Address, f64>,
}

/// Cấu trúc phân tích cân bằng Nash
#[derive(Debug)]
pub struct EquilibriumAnalyzer {
    /// Danh sách người chơi
    players: HashMap<Address, Player>,
    /// Ma trận thanh toán (payoff matrix)
    payoff_matrix: Vec<Vec<f64>>,
    /// Cấu hình
    config: GameConfig,
    /// Provider kết nối blockchain
    provider: Arc<Provider<Http>>,
    /// RNG
    rng: Arc<Mutex<ThreadRng>>,
}

impl EquilibriumAnalyzer {
    /// Tạo một bộ phân tích cân bằng Nash mới
    pub fn new(provider: Arc<Provider<Http>>, config: GameConfig) -> Self {
        Self {
            players: HashMap::new(),
            payoff_matrix: Vec::new(),
            config,
            provider,
            rng: Arc::new(Mutex::new(rand::thread_rng())),
        }
    }

    /// Thêm người chơi vào hệ thống
    pub fn add_player(&mut self, address: Address, strategy: Strategy) -> &mut Self {
        let player = Player {
            address,
            strategy,
            action_history: Vec::new(),
            utility: 0.0,
        };
        
        self.players.insert(address, player);
        self
    }

    /// Xóa người chơi khỏi hệ thống
    pub fn remove_player(&mut self, address: &Address) -> Result<()> {
        if self.players.remove(address).is_none() {
            return Err(anyhow!("Player not found"));
        }
        Ok(())
    }

    /// Khởi tạo ma trận thanh toán
    pub fn initialize_payoff_matrix(&mut self) -> Result<()> {
        let player_count = self.players.len();
        if player_count == 0 {
            return Err(anyhow!("No players in the system"));
        }
        
        // Khởi tạo ma trận rỗng
        let mut matrix = Vec::with_capacity(player_count);
        for _ in 0..player_count {
            matrix.push(vec![0.0; player_count]);
        }
        
        // Đặt giá trị cho ma trận, example: Prisoner's dilemma
        // Có thể thay đổi tùy theo yêu cầu cụ thể
        if player_count == 2 {
            // Cả hai hợp tác
            matrix[0][0] = 3.0; // Player 1 utility
            matrix[1][1] = 3.0; // Player 2 utility
            
            // Player 1 phản bội, Player 2 hợp tác
            matrix[0][1] = 5.0; // Player 1 utility
            matrix[1][0] = 0.0; // Player 2 utility
            
            // Player 1 hợp tác, Player 2 phản bội
            matrix[0][1] = 0.0; // Player 1 utility
            matrix[1][0] = 5.0; // Player 2 utility
            
            // Cả hai phản bội
            matrix[0][1] = 1.0; // Player 1 utility
            matrix[1][0] = 1.0; // Player 2 utility
        } else {
            // Ma trận N-người chơi
            for i in 0..player_count {
                for j in 0..player_count {
                    if i == j {
                        matrix[i][j] = 2.0; // Giá trị mặc định
                    } else {
                        matrix[i][j] = 1.0; // Giá trị mặc định
                    }
                }
            }
        }
        
        self.payoff_matrix = matrix;
        Ok(())
    }

    /// Dự đoán hành động của người chơi
    fn predict_action(&self, player: &Player, opponent_histories: &HashMap<Address, Vec<Action>>) -> Action {
        match player.strategy {
            Strategy::AlwaysCooperate => Action::Cooperate,
            Strategy::AlwaysDefect => Action::Defect,
            Strategy::TitForTat => {
                // Lấy hành động gần nhất của đối thủ
                let mut last_opponent_action = Action::Cooperate; // Mặc định là hợp tác
                
                for (_, history) in opponent_histories {
                    if let Some(last_action) = history.last() {
                        if *last_action == Action::Defect {
                            last_opponent_action = Action::Defect;
                            break;
                        }
                    }
                }
                
                last_opponent_action
            },
            Strategy::Random => {
                let mut rng = self.rng.lock().unwrap();
                if rng.gen::<f64>() < 0.5 {
                    Action::Cooperate
                } else {
                    Action::Defect
                }
            },
            Strategy::ProfitMaximizing => {
                // Phân tích lợi nhuận dự kiến từ mỗi hành động
                // Đơn giản hóa: Phản bội nếu có nhiều người chơi đã phản bội trong lịch sử
                let mut defection_count = 0;
                let mut total_actions = 0;
                
                for (_, history) in opponent_histories {
                    for action in history {
                        if *action == Action::Defect {
                            defection_count += 1;
                        }
                        total_actions += 1;
                    }
                }
                
                if total_actions == 0 || (defection_count as f64) / (total_actions as f64) < 0.3 {
                    Action::Cooperate
                } else {
                    Action::Defect
                }
            },
            Strategy::Custom(_) => {
                // Xử lý chiến lược tùy chỉnh
                Action::Cooperate
            },
        }
    }

    /// Tính toán lợi ích (utility) cho người chơi
    fn calculate_utility(&self, player_index: usize, actions: &HashMap<Address, Action>) -> f64 {
        let player_count = self.players.len();
        if player_count <= 1 || player_index >= player_count {
            return 0.0;
        }
        
        let mut utility = 0.0;
        
        // Đơn giản hóa: Tính điểm dựa trên số người hợp tác
        let cooperate_count = actions.values().filter(|&a| *a == Action::Cooperate).count();
        
        // Lấy hành động của người chơi hiện tại
        let player_address = self.players.keys().nth(player_index).unwrap();
        let player_action = actions.get(player_address).unwrap_or(&Action::NoAction);
        
        match player_action {
            Action::Cooperate => {
                // Nếu hợp tác, lợi ích = số người hợp tác * 2
                utility = (cooperate_count as f64) * 2.0;
            },
            Action::Defect => {
                // Nếu phản bội, lợi ích = số người hợp tác * 3
                utility = (cooperate_count as f64) * 3.0;
                
                // Nhưng nếu quá nhiều người phản bội, lợi ích giảm
                let defect_count = actions.len() - cooperate_count;
                if defect_count > 1 {
                    utility -= (defect_count as f64) * 1.5;
                }
            },
            Action::NoAction => {
                // Không có lợi ích nếu không hành động
            }
        }
        
        utility
    }

    /// Chạy một lần mô phỏng game
    pub async fn run_game(&self) -> Result<GameOutcome> {
        if self.players.is_empty() {
            return Err(anyhow!("No players in the system"));
        }
        
        // Thu thập lịch sử hành động của mỗi người chơi
        let mut opponent_histories: HashMap<Address, Vec<Action>> = HashMap::new();
        for (address, player) in &self.players {
            opponent_histories.insert(*address, player.action_history.clone());
        }
        
        // Dự đoán hành động của mỗi người chơi
        let mut actions: HashMap<Address, Action> = HashMap::new();
        for (address, player) in &self.players {
            let action = self.predict_action(player, &opponent_histories);
            actions.insert(*address, action);
        }
        
        // Tính toán lợi ích cho mỗi người chơi
        let mut utilities: HashMap<Address, f64> = HashMap::new();
        for (i, (address, _)) in self.players.iter().enumerate() {
            let utility = self.calculate_utility(i, &actions);
            utilities.insert(*address, utility);
        }
        
        // Tính toán lợi nhuận tổng thể
        let total_utility: f64 = utilities.values().sum();
        let average_utility = total_utility / (self.players.len() as f64);
        
        // Kiểm tra thành công
        let success = average_utility > self.config.min_utility_threshold;
        
        // Tính chi phí gas (giả định)
        let gas_cost = self.config.base_gas_cost * (self.players.len() as f64);
        
        // Tính lợi nhuận
        let profit = if success {
            total_utility - gas_cost
        } else {
            -gas_cost
        };
        
        // Tạo kết quả game
        let outcome = GameOutcome {
            success,
            profit,
            gas_cost,
            actions,
            utilities,
        };
        
        Ok(outcome)
    }

    /// Tìm cân bằng Nash
    pub async fn find_nash_equilibrium(&self) -> Result<Vec<(Address, Strategy)>> {
        if self.players.is_empty() {
            return Err(anyhow!("No players in the system"));
        }
        
        // Đơn giản hóa: Đối với Prisoner's dilemma, cân bằng Nash là tất cả phản bội
        let mut equilibrium = Vec::new();
        for (address, _) in &self.players {
            equilibrium.push((*address, Strategy::AlwaysDefect));
        }
        
        Ok(equilibrium)
    }

    /// Tìm chiến lược tối ưu Pareto
    pub async fn find_pareto_optimal(&self) -> Result<Vec<(Address, Strategy)>> {
        if self.players.is_empty() {
            return Err(anyhow!("No players in the system"));
        }
        
        // Đơn giản hóa: Đối với Prisoner's dilemma, tối ưu Pareto là tất cả hợp tác
        let mut optimal = Vec::new();
        for (address, _) in &self.players {
            optimal.push((*address, Strategy::AlwaysCooperate));
        }
        
        Ok(optimal)
    }

    /// Đề xuất chiến lược tối ưu
    pub async fn suggest_optimal_strategy(&self, address: &Address) -> Result<Strategy> {
        if !self.players.contains_key(address) {
            return Err(anyhow!("Player not found"));
        }
        
        // Đơn giản hóa: Đề xuất TitForTat trong hầu hết các trường hợp
        // Đây là chiến lược cân bằng tốt trong nhiều tình huống
        Ok(Strategy::TitForTat)
    }
}

/// Trait để phân tích cân bằng trong game theory
#[async_trait]
pub trait EquilibriumAnalysis: Send + Sync {
    /// Phân tích các chiến lược và đưa ra đề xuất
    async fn analyze_equilibrium(&self, token_address: &str, players: Vec<Address>) 
        -> Result<GameOutcome, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Đề xuất chiến lược tối ưu cho một người chơi cụ thể
    async fn suggest_strategy(&self, token_address: &str, player: &Address)
        -> Result<Strategy, Box<dyn std::error::Error + Send + Sync>>;
}

/// Implement EquilibriumAnalysis cho EquilibriumAnalyzer
#[async_trait]
impl EquilibriumAnalysis for EquilibriumAnalyzer {
    async fn analyze_equilibrium(&self, token_address: &str, players: Vec<Address>) 
        -> Result<GameOutcome, Box<dyn std::error::Error + Send + Sync>> {
        
        let result = self.run_game().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
        Ok(result)
    }
    
    async fn suggest_strategy(&self, token_address: &str, player: &Address)
        -> Result<Strategy, Box<dyn std::error::Error + Send + Sync>> {
        
        let strategy = self.suggest_optimal_strategy(player).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            
        Ok(strategy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_equilibrium_analyzer() {
        // Khởi tạo provider
        let provider = Arc::new(Provider::<Http>::try_from("http://localhost:8545").unwrap());
        
        // Khởi tạo config
        let config = GameConfig {
            min_utility_threshold: 2.0,
            base_gas_cost: 0.1,
            // Thêm các trường khác nếu cần
        };
        
        // Khởi tạo analyzer
        let mut analyzer = EquilibriumAnalyzer::new(provider, config);
        
        // Thêm người chơi
        let player1 = "0x1111111111111111111111111111111111111111".parse::<Address>().unwrap();
        let player2 = "0x2222222222222222222222222222222222222222".parse::<Address>().unwrap();
        
        analyzer.add_player(player1, Strategy::TitForTat)
                .add_player(player2, Strategy::AlwaysCooperate);
        
        // Khởi tạo ma trận
        analyzer.initialize_payoff_matrix().unwrap();
        
        // Chạy game
        let outcome = analyzer.run_game().await.unwrap();
        
        // Kiểm tra kết quả
        assert!(outcome.success);
        assert!(outcome.profit >= 0.0);
    }
} 
// External imports
use ethers::types as ethers_types;

// Standard library imports
use std::time::Duration;
use std::fmt::{Display, Formatter};

// Third party imports
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

// Internal modules
mod block;
mod chain;
mod transaction;
mod user;
mod wallet;

// Re-exports
pub use block::*;
pub use chain::*;
pub use transaction::*;
pub use user::*;
pub use wallet::*;

/// Thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Địa chỉ token
    pub address: ethers_types::Address,
    /// Tên token
    pub name: String,
    /// Ký hiệu token
    pub symbol: String,
    /// Số thập phân
    pub decimals: u8,
    /// Tổng cung
    pub total_supply: ethers_types::U256,
    /// Giá hiện tại (USD)
    pub current_price: f64,
}

/// Thông tin cặp giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairInfo {
    /// Địa chỉ cặp
    pub address: ethers_types::Address,
    /// Token 0
    pub token0: TokenInfo,
    /// Token 1
    pub token1: TokenInfo,
    /// Tỷ giá
    pub rate: f64,
    /// Thanh khoản
    pub liquidity: ethers_types::U256,
}

/// Thông tin giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeInfo {
    /// Hash giao dịch
    pub tx_hash: ethers_types::H256,
    /// Token mua
    pub buy_token: TokenInfo,
    /// Token bán
    pub sell_token: TokenInfo,
    /// Số lượng mua
    pub buy_amount: ethers_types::U256,
    /// Số lượng bán
    pub sell_amount: ethers_types::U256,
    /// Thời gian giao dịch
    pub timestamp: DateTime<Utc>,
}

/// Thông tin đơn hàng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderInfo {
    /// ID đơn hàng
    pub id: String,
    /// Token mua
    pub buy_token: TokenInfo,
    /// Token bán
    pub sell_token: TokenInfo,
    /// Số lượng mua
    pub buy_amount: ethers_types::U256,
    /// Số lượng bán
    pub sell_amount: ethers_types::U256,
    /// Trạng thái đơn hàng
    pub status: OrderStatus,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
}

/// Trạng thái đơn hàng
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Đang chờ
    Pending,
    /// Đã khớp một phần
    PartiallyFilled,
    /// Đã khớp hoàn toàn
    Filled,
    /// Đã hủy
    Cancelled,
    /// Đã hết hạn
    Expired,
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test User
    #[test]
    fn test_user() {
        let user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );
        assert_eq!(user.username, "test_user");
        assert_eq!(user.email, "test@example.com");
        assert_eq!(user.role, UserRole::User);
    }

    /// Test Wallet
    #[test]
    fn test_wallet() {
        let wallet = Wallet::new(
            ethers_types::Address::random(),
            WalletType::Standard,
        );
        assert_eq!(wallet.wallet_type, WalletType::Standard);
        assert_eq!(wallet.status, WalletStatus::Active);
    }

    /// Test Transaction
    #[test]
    fn test_transaction() {
        let tx = Transaction::new(
            ethers_types::H256::random(),
            ethers_types::Address::random(),
            ethers_types::Address::random(),
            ethers_types::U256::from(1000000000000000000u64),
        );
        assert_eq!(tx.status, TransactionStatus::Pending);
    }

    /// Test Block
    #[test]
    fn test_block() {
        let block = Block::new(
            1,
            ethers_types::H256::random(),
            ethers_types::H256::random(),
            vec![],
        );
        assert_eq!(block.number, 1);
        assert_eq!(block.status, BlockStatus::Pending);
    }

    /// Test Chain
    #[test]
    fn test_chain() {
        let chain = Chain::new(
            "Ethereum".to_string(),
            1,
            "http://localhost:8545".to_string(),
        );
        assert_eq!(chain.name, "Ethereum");
        assert_eq!(chain.chain_id, 1);
        assert_eq!(chain.status, ChainStatus::Active);
    }
} 
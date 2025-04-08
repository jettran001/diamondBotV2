// External imports
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::LookupMap,
    json_types::U128,
    env, log, near_bindgen, AccountId, Balance, PanicOnDefault, Promise,
    require, PromiseOrValue, ext_contract,
    serde::{Deserialize, Serialize},
};

// Standard library imports
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    fmt::{self, Display, Formatter},
    error::Error,
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::{timeout, sleep};

/// Interface cho Diamond Token
#[ext_contract(ext_diamond_token)]
pub trait DiamondToken: Send + Sync + 'static {
    /// Mint token cho account
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - Account ID nhận token
    /// * `amount` - Số lượng token cần mint
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise cho việc mint token
    fn mint(&mut self, account_id: AccountId, amount: U128) -> Promise;
    
    /// Lấy số dư token của account
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - Account ID cần kiểm tra
    /// 
    /// # Returns
    /// 
    /// * `U128` - Số dư token
    fn ft_balance_of(&self, account_id: AccountId) -> U128;
}

/// Interface cho Mining Callbacks
#[ext_contract(ext_self)]
pub trait MiningCallbacks: Send + Sync + 'static {
    /// Callback khi claim reward thành công
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - Account ID của user
    /// * `amount` - Số lượng token được claim
    fn claim_reward_callback(&mut self, user_id: AccountId, amount: U128);
}

/// Struct quản lý mining Diamond Token
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct DiamondMining {
    /// Account ID của owner
    pub owner_id: AccountId,
    /// Contract ID của Diamond Token
    pub diamond_token: AccountId,
    /// Tỷ lệ phần thưởng mỗi block/epoch
    pub reward_rate: U128,
    /// Block/epoch cuối cùng tính phần thưởng
    pub last_reward_block: u64,
    /// Rewards của mỗi user
    pub rewards: LookupMap<AccountId, U128>,
    /// Lần cuối mỗi user cập nhật
    pub last_update: LookupMap<AccountId, u64>,
    /// Thời gian cập nhật cuối cùng
    last_update_time: u64,
}

#[near_bindgen]
impl DiamondMining {
    /// Khởi tạo contract mới
    /// 
    /// # Arguments
    /// 
    /// * `diamond_token` - Contract ID của Diamond Token
    /// 
    /// # Returns
    /// 
    /// * `Self` - Instance mới của DiamondMining
    #[init]
    pub fn new(diamond_token: AccountId) -> Self {
        let owner_id = env::predecessor_account_id();
        Self {
            owner_id,
            diamond_token,
            reward_rate: U128(10_000_000_000_000_000_000), // 10 DMD mỗi block (18 decimals)
            last_reward_block: env::block_height(),
            rewards: LookupMap::new(b"r".to_vec()),
            last_update: LookupMap::new(b"u".to_vec()),
            last_update_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Cập nhật phần thưởng cho user
    pub fn update_rewards(&mut self) {
        let user_id = env::predecessor_account_id();
        let current_block = env::block_height();
        
        // Lấy block cuối cùng mà user cập nhật
        let last_update = self.last_update.get(&user_id).unwrap_or(self.last_reward_block);
        
        // Tính số block đã qua
        let blocks_passed = current_block - last_update;
        
        if blocks_passed > 0 {
            // Tính phần thưởng
            let total_reward = U128(blocks_passed as u128 * self.reward_rate.0);
            
            // Cập nhật phần thưởng cho user
            let current_reward = self.rewards.get(&user_id).unwrap_or(U128(0));
            let new_reward = U128(current_reward.0 + total_reward.0);
            
            self.rewards.insert(&user_id, &new_reward);
            self.last_update.insert(&user_id, &current_block);
            self.last_update_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            log!("User {} updated rewards: +{} DMD", user_id, total_reward.0);
        }
    }

    /// Yêu cầu phần thưởng
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise cho việc claim reward
    #[payable]
    pub fn claim_rewards(&mut self) -> Promise {
        let user_id = env::predecessor_account_id();
        
        // Cập nhật phần thưởng trước
        self.update_rewards();
        
        // Lấy phần thưởng hiện tại
        let reward = self.rewards.get(&user_id).unwrap_or(U128(0));
        require!(reward.0 > 0, "No rewards to claim");
        
        // Reset phần thưởng
        self.rewards.insert(&user_id, &U128(0));
        
        // Gọi hàm mint trên Diamond Token
        ext_diamond_token::ext(self.diamond_token.clone())
            .with_attached_deposit(1)
            .with_static_gas(env::prepaid_gas() / 3)
            .mint(
                user_id.clone(),
                reward
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(env::prepaid_gas() / 3)
                    .claim_reward_callback(user_id, reward)
            )
    }

    /// Callback khi claim reward thành công
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - Account ID của user
    /// * `amount` - Số lượng token được claim
    #[private]
    pub fn claim_reward_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            log!("User {} claimed reward: {} DMD", user_id, amount.0);
        } else {
            log!("Reward claim failed for user {}", user_id);
            // Khôi phục phần thưởng nếu mint thất bại
            let current_reward = self.rewards.get(&user_id).unwrap_or(U128(0));
            self.rewards.insert(&user_id, &U128(current_reward.0 + amount.0));
        }
    }

    /// Lấy phần thưởng hiện tại của user
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - Account ID của user
    /// 
    /// # Returns
    /// 
    /// * `U128` - Số lượng token thưởng
    pub fn get_pending_reward(&self, account_id: AccountId) -> U128 {
        let current_reward = self.rewards.get(&account_id).unwrap_or(U128(0));
        
        // Tính thêm phần thưởng từ lần cập nhật cuối đến hiện tại
        let current_block = env::block_height();
        let last_update = self.last_update.get(&account_id).unwrap_or(self.last_reward_block);
        let blocks_passed = current_block - last_update;
        
        if blocks_passed > 0 {
            let additional_reward = U128(blocks_passed as u128 * self.reward_rate.0);
            return U128(current_reward.0 + additional_reward.0);
        }
        
        current_reward
    }

    /// Thay đổi tỷ lệ phần thưởng (chỉ owner)
    /// 
    /// # Arguments
    /// 
    /// * `reward_rate` - Tỷ lệ phần thưởng mới
    #[payable]
    pub fn set_reward_rate(&mut self, reward_rate: U128) {
        self.assert_owner();
        self.reward_rate = reward_rate;
        log!("Reward rate updated to {} DMD per block", reward_rate.0);
    }

    /// Force cập nhật phần thưởng cho tất cả các user (chỉ owner)
    #[payable]
    pub fn force_update_global_reward(&mut self) {
        self.assert_owner();
        self.last_reward_block = env::block_height();
        log!("Global reward block updated to {}", self.last_reward_block);
    }

    /// Cấu hình token contract (chỉ owner)
    /// 
    /// # Arguments
    /// 
    /// * `diamond_token` - Contract ID mới của Diamond Token
    #[payable]
    pub fn set_diamond_token(&mut self, diamond_token: AccountId) {
        self.assert_owner();
        self.diamond_token = diamond_token;
        log!("Diamond Token contract updated to {}", self.diamond_token);
    }

    /// Chuyển quyền owner
    /// 
    /// # Arguments
    /// 
    /// * `new_owner` - Account ID của owner mới
    #[payable]
    pub fn transfer_ownership(&mut self, new_owner: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner;
        log!("Ownership transferred to {}", new_owner);
    }

    /// Kiểm tra quyền owner
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this method"
        );
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_context, VMContextBuilder};
    use near_sdk::MockedBlockchain;
    use near_sdk::{testing_env, VMContext};

    /// Test khởi tạo contract
    #[test]
    fn test_new() {
        let context = get_context("alice".to_string(), 1);
        testing_env!(context);
        
        let contract = DiamondMining::new("diamond_token".to_string());
        
        assert_eq!(contract.owner_id, "alice".to_string());
        assert_eq!(contract.diamond_token, "diamond_token".to_string());
        assert_eq!(contract.reward_rate.0, 10_000_000_000_000_000_000);
        assert_eq!(contract.last_reward_block, 1);
    }

    /// Test cập nhật phần thưởng
    #[test]
    fn test_update_rewards() {
        let mut context = get_context("alice".to_string(), 1);
        testing_env!(context.clone());
        
        let mut contract = DiamondMining::new("diamond_token".to_string());
        
        // Cập nhật block height
        context.block_height = 10;
        testing_env!(context.clone());
        
        contract.update_rewards();
        
        let reward = contract.get_pending_reward("alice".to_string());
        assert_eq!(reward.0, 90_000_000_000_000_000_000); // 9 blocks * 10 DMD
    }
}

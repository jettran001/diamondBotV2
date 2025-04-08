use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, log, near_bindgen, AccountId, Balance, PanicOnDefault, Promise,
    require, PromiseOrValue, ext_contract,
};
use near_sdk::serde::{Deserialize, Serialize};

// External imports
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
#[ext_contract(ext_ft)]
pub trait FungibleToken: Send + Sync + 'static {
    /// Chuyển token
    /// 
    /// # Arguments
    /// 
    /// * `receiver_id` - ID người nhận
    /// * `amount` - Số lượng token
    /// * `memo` - Ghi chú
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);

    /// Chuyển token và gọi callback
    /// 
    /// # Arguments
    /// 
    /// * `receiver_id` - ID người nhận
    /// * `amount` - Số lượng token
    /// * `memo` - Ghi chú
    /// * `msg` - Thông điệp callback
    /// 
    /// # Returns
    /// 
    /// * `PromiseOrValue<U128>` - Kết quả chuyển token
    fn ft_transfer_call(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>, msg: String) -> PromiseOrValue<U128>;

    /// Lấy số dư token
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - ID tài khoản
    /// 
    /// # Returns
    /// 
    /// * `U128` - Số dư token
    fn ft_balance_of(&self, account_id: AccountId) -> U128;
}

/// Interface cho Farming callbacks
#[ext_contract(ext_self)]
pub trait FarmingCallbacks: Send + Sync + 'static {
    /// Callback khi stake token
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    fn stake_callback(&mut self, user_id: AccountId, amount: U128);

    /// Callback khi unstake token
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    fn unstake_callback(&mut self, user_id: AccountId, amount: U128);

    /// Callback khi claim reward
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    fn claim_callback(&mut self, user_id: AccountId, amount: U128);
}

/// Thông tin stake của người dùng
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct Stake {
    /// Số lượng token đã stake
    pub amount: U128,
    /// Thời gian bắt đầu stake
    pub start_time: u64,
    /// Thời gian claim cuối cùng
    pub last_claim: u64,
    /// Thời gian cập nhật cuối cùng
    pub last_update: u64,
}

/// Hợp đồng Farming
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Farming {
    /// ID chủ sở hữu
    pub owner_id: AccountId,
    /// Token staking (Diamond Token - DMD)
    pub staking_token: AccountId,
    /// Tổng số token đã stake
    pub total_staked: U128,
    /// Tỷ lệ phần thưởng (1 DMD/giây cho mỗi 100 DMD staked)
    pub reward_rate: U128,
    /// Stake của mỗi người dùng
    pub stakes: LookupMap<AccountId, Stake>,
    /// Thời gian khóa (30 ngày)
    pub lock_period: u64,
    /// Thời gian cập nhật cuối cùng
    pub last_update: u64,
}

/// Thời gian khóa mặc định (30 ngày)
const LOCK_PERIOD: u64 = 30 * 24 * 60 * 60 * 1_000_000_000; // 30 ngày (nanoseconds)

#[near_bindgen]
impl Farming {
    /// Khởi tạo hợp đồng Farming mới
    /// 
    /// # Arguments
    /// 
    /// * `staking_token` - ID token staking
    /// 
    /// # Returns
    /// 
    /// * `Self` - Instance mới của Farming
    #[init]
    pub fn new(staking_token: AccountId) -> Self {
        let owner_id = env::predecessor_account_id();
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            owner_id,
            staking_token,
            total_staked: U128(0),
            reward_rate: U128(1_000_000_000_000_000_000), // 1 DMD (18 decimals)
            stakes: LookupMap::new(b"s".to_vec()),
            lock_period: LOCK_PERIOD,
            last_update: current_time,
        }
    }

    /// Stake token
    /// 
    /// # Arguments
    /// 
    /// * `amount` - Số lượng token
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise cho việc stake
    #[payable]
    pub fn stake(&mut self, amount: U128) -> Promise {
        require!(amount.0 > 0, "Amount must be greater than 0");
        let user_id = env::predecessor_account_id();
        
        // Gọi ft_transfer_call trên Diamond Token để chuyển token vào farming contract
        ext_ft::ext(self.staking_token.clone())
            .with_attached_deposit(1)
            .with_static_gas(env::prepaid_gas() / 3)
            .ft_transfer_call(
                env::current_account_id(),
                amount,
                None,
                format!("{{\"action\": \"stake\", \"user_id\": \"{}\"}}", user_id)
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(env::prepaid_gas() / 3)
                    .stake_callback(user_id, amount)
            )
    }

    /// Callback khi stake thành công
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    #[private]
    pub fn stake_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            // Nếu user đã có stake, claim rewards trước
            if let Some(mut user_stake) = self.stakes.get(&user_id) {
                let reward = self.calculate_reward(&user_stake);
                if reward.0 > 0 {
                    log!("User {} claimed reward: {}", user_id, reward.0);
                    user_stake.last_claim = env::block_timestamp();
                    // Xử lý reward (gửi token về cho user)
                    // (Sẽ triển khai ở hàm claim_rewards)
                }
                
                // Cập nhật stake mới
                user_stake.amount = U128(user_stake.amount.0 + amount.0);
                user_stake.last_update = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                self.stakes.insert(&user_id, &user_stake);
            } else {
                // Tạo stake mới
                let stake = Stake {
                    amount,
                    start_time: env::block_timestamp(),
                    last_claim: env::block_timestamp(),
                    last_update: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                self.stakes.insert(&user_id, &stake);
            }
            
            // Cập nhật tổng số token đã stake
            self.total_staked = U128(self.total_staked.0 + amount.0);
            self.last_update = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            log!("User {} staked {} DMD", user_id, amount.0);
        } else {
            log!("Stake transaction failed for user {}", user_id);
        }
    }

    /// Unstake token
    /// 
    /// # Arguments
    /// 
    /// * `amount` - Số lượng token
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise cho việc unstake
    #[payable]
    pub fn unstake(&mut self, amount: U128) -> Promise {
        let user_id = env::predecessor_account_id();
        let stake = self.stakes.get(&user_id).expect("No stake found");
        
        require!(stake.amount.0 >= amount.0, "Insufficient staked amount");
        
        // Kiểm tra thời gian khóa
        let current_time = env::block_timestamp();
        require!(
            current_time >= stake.start_time + self.lock_period,
            "Stake is still locked"
        );
        
        // Claim rewards trước khi unstake
        self.claim_rewards();
        
        // Chuyển token về cho user
        self.total_staked = U128(self.total_staked.0 - amount.0);
        
        ext_ft::ext(self.staking_token.clone())
            .with_attached_deposit(1)
            .with_static_gas(env::prepaid_gas() / 3)
            .ft_transfer(
                user_id.clone(),
                amount,
                Some("Unstake from Farming".to_string())
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(env::prepaid_gas() / 3)
                    .unstake_callback(user_id, amount)
            )
    }

    /// Callback khi unstake thành công
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    #[private]
    pub fn unstake_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            let mut stake = self.stakes.get(&user_id).expect("No stake found");
            stake.amount = U128(stake.amount.0 - amount.0);
            stake.last_update = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            if stake.amount.0 == 0 {
                self.stakes.remove(&user_id);
            } else {
                self.stakes.insert(&user_id, &stake);
            }
            
            self.last_update = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            log!("User {} unstaked {} DMD", user_id, amount.0);
        } else {
            log!("Unstake transaction failed for user {}", user_id);
            // Khôi phục total_staked nếu unstake thất bại
            self.total_staked = U128(self.total_staked.0 + amount.0);
        }
    }

    /// Claim rewards
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise cho việc claim rewards
    #[payable]
    pub fn claim_rewards(&mut self) -> Promise {
        let user_id = env::predecessor_account_id();
        let stake = self.stakes.get(&user_id).expect("No stake found");
        
        let reward = self.calculate_reward(&stake);
        require!(reward.0 > 0, "No rewards to claim");
        
        // Cập nhật thời gian claim cuối cùng
        let mut stake = stake;
        stake.last_claim = env::block_timestamp();
        stake.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.stakes.insert(&user_id, &stake);
        
        // Gửi reward về cho user
        ext_ft::ext(self.staking_token.clone())
            .with_attached_deposit(1)
            .with_static_gas(env::prepaid_gas() / 3)
            .ft_transfer(
                user_id.clone(),
                reward,
                Some(format!("Claim reward {} DMD", reward.0))
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(env::prepaid_gas() / 3)
                    .claim_callback(user_id, reward)
            )
    }

    /// Callback khi claim rewards thành công
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID người dùng
    /// * `amount` - Số lượng token
    #[private]
    pub fn claim_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            log!("User {} claimed reward: {} DMD", user_id, amount.0);
        } else {
            log!("Reward claim failed for user {}", user_id);
            // Khôi phục last_claim nếu claim thất bại
            if let Some(mut stake) = self.stakes.get(&user_id) {
                stake.last_claim = stake.last_update;
                self.stakes.insert(&user_id, &stake);
            }
        }
    }

    /// Tính phần thưởng dựa trên thời gian và số lượng stake
    /// 
    /// # Arguments
    /// 
    /// * `stake` - Thông tin stake
    /// 
    /// # Returns
    /// 
    /// * `U128` - Số lượng reward
    fn calculate_reward(&self, stake: &Stake) -> U128 {
        let time_elapsed = env::block_timestamp() - stake.last_claim;
        
        // Công thức: amount * reward_rate * time_elapsed / (100 * 10^18)
        // Tương đương với 1 DMD mỗi giây cho mỗi 100 DMD staked
        let reward = stake.amount.0 * self.reward_rate.0 * time_elapsed as u128 / (100 * 10u128.pow(18));
        
        U128(reward)
    }

    /// Lấy thông tin stake của user
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - ID tài khoản
    /// 
    /// # Returns
    /// 
    /// * `Option<Stake>` - Thông tin stake nếu có
    pub fn get_stake(&self, account_id: AccountId) -> Option<Stake> {
        self.stakes.get(&account_id)
    }

    /// Lấy phần thưởng có thể claim
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - ID tài khoản
    /// 
    /// # Returns
    /// 
    /// * `U128` - Số lượng reward có thể claim
    pub fn get_pending_reward(&self, account_id: AccountId) -> U128 {
        if let Some(stake) = self.stakes.get(&account_id) {
            self.calculate_reward(&stake)
        } else {
            U128(0)
        }
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
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        log!("Reward rate updated to {} DMD per second", reward_rate.0);
    }

    /// Thay đổi thời gian khóa (chỉ owner)
    /// 
    /// # Arguments
    /// 
    /// * `lock_period` - Thời gian khóa mới
    #[payable]
    pub fn set_lock_period(&mut self, lock_period: u64) {
        self.assert_owner();
        self.lock_period = lock_period;
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        log!("Lock period updated to {} nanoseconds", lock_period);
    }

    /// Chuyển quyền sở hữu (chỉ owner)
    /// 
    /// # Arguments
    /// 
    /// * `new_owner` - ID chủ sở hữu mới
    #[payable]
    pub fn transfer_ownership(&mut self, new_owner: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner;
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        log!("Ownership transferred to {}", new_owner);
    }

    /// Kiểm tra quyền sở hữu
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this function"
        );
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_context, VMContextBuilder};
    use near_sdk::testing_env;

    /// Tạo context cho test
    fn get_context(predecessor_account_id: AccountId) -> VMContext {
        VMContextBuilder::new()
            .predecessor_account_id(predecessor_account_id)
            .build()
    }

    /// Test khởi tạo hợp đồng
    #[test]
    fn test_new() {
        let context = get_context("alice.near".parse().unwrap());
        testing_env!(context);
        
        let contract = Farming::new("diamond.near".parse().unwrap());
        assert_eq!(contract.owner_id, "alice.near".parse().unwrap());
        assert_eq!(contract.staking_token, "diamond.near".parse().unwrap());
        assert_eq!(contract.total_staked.0, 0);
        assert_eq!(contract.reward_rate.0, 1_000_000_000_000_000_000);
        assert_eq!(contract.lock_period, LOCK_PERIOD);
    }

    /// Test tính toán reward
    #[test]
    fn test_calculate_reward() {
        let context = get_context("alice.near".parse().unwrap());
        testing_env!(context);
        
        let contract = Farming::new("diamond.near".parse().unwrap());
        let stake = Stake {
            amount: U128(100 * 10u128.pow(18)), // 100 DMD
            start_time: 0,
            last_claim: 0,
            last_update: 0,
        };
        
        // 1 giây = 1_000_000_000 nanoseconds
        let reward = contract.calculate_reward(&stake);
        assert_eq!(reward.0, 1_000_000_000_000_000_000); // 1 DMD
    }
}

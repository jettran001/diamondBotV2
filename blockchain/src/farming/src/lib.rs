use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, log, near_bindgen, AccountId, Balance, PanicOnDefault, Promise,
    require, PromiseOrValue, ext_contract,
};
use near_sdk::serde::{Deserialize, Serialize};

// Định nghĩa interface cho Diamond Token
#[ext_contract(ext_ft)]
pub trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    fn ft_transfer_call(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>, msg: String) -> PromiseOrValue<U128>;
    fn ft_balance_of(&self, account_id: AccountId) -> U128;
}

#[ext_contract(ext_self)]
pub trait FarmingCallbacks {
    fn stake_callback(&mut self, user_id: AccountId, amount: U128);
    fn unstake_callback(&mut self, user_id: AccountId, amount: U128);
    fn claim_callback(&mut self, user_id: AccountId, amount: U128);
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct Stake {
    pub amount: U128,
    pub start_time: u64,
    pub last_claim: u64,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Farming {
    pub owner_id: AccountId,
    pub staking_token: AccountId,      // Diamond Token (DMD)
    pub total_staked: U128,            // Tổng số token đã stake
    pub reward_rate: U128,             // Tỷ lệ phần thưởng (1 DMD/giây cho mỗi 100 DMD staked)
    pub stakes: LookupMap<AccountId, Stake>, // Stake của mỗi user
    pub lock_period: u64,              // Thời gian khóa (30 ngày)
}

const LOCK_PERIOD: u64 = 30 * 24 * 60 * 60 * 1_000_000_000; // 30 ngày (nanoseconds)

#[near_bindgen]
impl Farming {
    #[init]
    pub fn new(staking_token: AccountId) -> Self {
        let owner_id = env::predecessor_account_id();
        Self {
            owner_id,
            staking_token,
            total_staked: U128(0),
            reward_rate: U128(1_000_000_000_000_000_000), // 1 DMD (18 decimals)
            stakes: LookupMap::new(b"s".to_vec()),
            lock_period: LOCK_PERIOD,
        }
    }

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
                self.stakes.insert(&user_id, &user_stake);
            } else {
                // Tạo stake mới
                let stake = Stake {
                    amount,
                    start_time: env::block_timestamp(),
                    last_claim: env::block_timestamp(),
                };
                self.stakes.insert(&user_id, &stake);
            }
            
            // Cập nhật tổng số token đã stake
            self.total_staked = U128(self.total_staked.0 + amount.0);
            log!("User {} staked {} DMD", user_id, amount.0);
        } else {
            log!("Stake transaction failed for user {}", user_id);
        }
    }

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

    #[private]
    pub fn unstake_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            // Cập nhật stake của user
            if let Some(mut stake) = self.stakes.get(&user_id) {
                stake.amount = U128(stake.amount.0 - amount.0);
                if stake.amount.0 == 0 {
                    self.stakes.remove(&user_id);
                } else {
                    self.stakes.insert(&user_id, &stake);
                }
                log!("User {} unstaked {} DMD", user_id, amount.0);
            }
        } else {
            log!("Unstake transaction failed for user {}", user_id);
            // Khôi phục total_staked
            self.total_staked = U128(self.total_staked.0 + amount.0);
        }
    }

    #[payable]
    pub fn claim_rewards(&mut self) -> Promise {
        let user_id = env::predecessor_account_id();
        if let Some(mut stake) = self.stakes.get(&user_id) {
            let reward = self.calculate_reward(&stake);
            require!(reward.0 > 0, "No rewards to claim");
            
            // Cập nhật thời gian claim cuối cùng
            stake.last_claim = env::block_timestamp();
            self.stakes.insert(&user_id, &stake);
            
            // Chuyển phần thưởng về cho user
            ext_ft::ext(self.staking_token.clone())
                .with_attached_deposit(1)
                .with_static_gas(env::prepaid_gas() / 3)
                .ft_transfer(
                    user_id.clone(),
                    reward,
                    Some("Reward from Farming".to_string())
                )
                .then(
                    ext_self::ext(env::current_account_id())
                        .with_static_gas(env::prepaid_gas() / 3)
                        .claim_callback(user_id, reward)
                )
        } else {
            env::panic_str("No stake found");
        }
    }

    #[private]
    pub fn claim_callback(&mut self, user_id: AccountId, amount: U128) {
        if env::promise_result(0) == near_sdk::PromiseResult::Successful {
            log!("User {} claimed reward: {}", user_id, amount.0);
        } else {
            log!("Claim reward failed for user {}", user_id);
            // Khôi phục last_claim time
            if let Some(mut stake) = self.stakes.get(&user_id) {
                stake.last_claim = env::block_timestamp() - (amount.0 * 100 * 10u128.pow(18) / self.reward_rate.0) as u64;
                self.stakes.insert(&user_id, &stake);
            }
        }
    }

    // Tính phần thưởng dựa trên thời gian và số lượng stake
    fn calculate_reward(&self, stake: &Stake) -> U128 {
        let time_elapsed = env::block_timestamp() - stake.last_claim;
        
        // Công thức: amount * reward_rate * time_elapsed / (100 * 10^18)
        // Tương đương với 1 DMD mỗi giây cho mỗi 100 DMD staked
        let reward = stake.amount.0 * self.reward_rate.0 * time_elapsed as u128 / (100 * 10u128.pow(18));
        
        U128(reward)
    }

    // Lấy thông tin stake của user
    pub fn get_stake(&self, account_id: AccountId) -> Option<Stake> {
        self.stakes.get(&account_id)
    }

    // Lấy phần thưởng có thể claim
    pub fn get_pending_reward(&self, account_id: AccountId) -> U128 {
        if let Some(stake) = self.stakes.get(&account_id) {
            self.calculate_reward(&stake)
        } else {
            U128(0)
        }
    }

    // Thay đổi tỷ lệ phần thưởng (chỉ owner)
    #[payable]
    pub fn set_reward_rate(&mut self, reward_rate: U128) {
        self.assert_owner();
        self.reward_rate = reward_rate;
        log!("Reward rate updated to {}", reward_rate.0);
    }

    // Thay đổi thời gian khóa (chỉ owner)
    #[payable]
    pub fn set_lock_period(&mut self, lock_period: u64) {
        self.assert_owner();
        self.lock_period = lock_period;
        log!("Lock period updated to {} seconds", lock_period / 1_000_000_000);
    }

    // Chuyển quyền owner
    #[payable]
    pub fn transfer_ownership(&mut self, new_owner: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner;
        log!("Ownership transferred to {}", self.owner_id);
    }

    // Kiểm tra owner
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this method"
        );
    }
}

// Triển khai ft_on_transfer cho NEP-141
#[near_bindgen]
impl Farming {
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> U128 {
        // Xử lý khi nhận token từ ft_transfer_call
        let parsed_msg: serde_json::Value = serde_json::from_str(&msg).expect("Invalid msg format");
        
        if let Some(action) = parsed_msg["action"].as_str() {
            match action {
                "stake" => {
                    // Xử lý stake
                    if let Some(user_id_str) = parsed_msg["user_id"].as_str() {
                        let user_id: AccountId = user_id_str.parse().expect("Invalid account ID in msg");
                        
                        // Update stake
                        if let Some(mut user_stake) = self.stakes.get(&user_id) {
                            // Xử lý phần thưởng hiện tại nếu có
                            let reward = self.calculate_reward(&user_stake);
                            if reward.0 > 0 {
                                log!("User {} has pending reward: {}", user_id, reward.0);
                                user_stake.last_claim = env::block_timestamp();
                            }
                            
                            user_stake.amount = U128(user_stake.amount.0 + amount.0);
                            self.stakes.insert(&user_id, &user_stake);
                        } else {
                            // Tạo stake mới
                            let stake = Stake {
                                amount,
                                start_time: env::block_timestamp(),
                                last_claim: env::block_timestamp(),
                            };
                            self.stakes.insert(&user_id, &stake);
                        }
                        
                        // Cập nhật tổng số token đã stake
                        self.total_staked = U128(self.total_staked.0 + amount.0);
                        log!("User {} staked {} DMD via ft_transfer_call", user_id, amount.0);
                        
                        // Không trả lại token
                        return U128(0);
                    }
                },
                _ => {
                    env::panic_str("Unknown action");
                }
            }
        }
        
        // Mặc định trả lại tất cả tokens
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, VMContext};

    fn get_context(predecessor_account_id: AccountId) -> VMContext {
        let mut builder = VMContextBuilder::new();
        builder.predecessor_account_id(predecessor_account_id);
        builder.current_account_id(accounts(0));
        builder.block_timestamp(1_600_000_000_000_000_000);
        builder.build()
    }

    #[test]
    fn test_new() {
        let context = get_context(accounts(1));
        testing_env!(context);
        
        let contract = Farming::new(accounts(2));
        assert_eq!(contract.owner_id, accounts(1));
        assert_eq!(contract.staking_token, accounts(2));
        assert_eq!(contract.total_staked.0, 0);
        assert_eq!(contract.lock_period, LOCK_PERIOD);
    }

    #[test]
    fn test_calculate_reward() {
        let context = get_context(accounts(1));
        testing_env!(context);
        
        let contract = Farming::new(accounts(2));
        
        let stake = Stake {
            amount: U128(100_000_000_000_000_000_000), // 100 DMD
            start_time: 1_600_000_000_000_000_000,
            last_claim: 1_600_000_000_000_000_000,
        };
        
        // Fake time lapse
        let mut ctx = get_context(accounts(1));
        ctx.block_timestamp = 1_600_000_010_000_000_000; // +10 seconds
        testing_env!(ctx);
        
        // Reward should be 10 DMD (1 DMD/s for 100 DMD staked for 10s)
        let reward = contract.calculate_reward(&stake);
        assert_eq!(reward.0, 10_000_000_000_000_000_000); // 10 DMD
    }
}

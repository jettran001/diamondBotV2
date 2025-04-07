use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, log, near_bindgen, AccountId, Balance, PanicOnDefault, Promise,
    require, PromiseOrValue, ext_contract,
};
use near_sdk::serde::{Deserialize, Serialize};

// Định nghĩa interface cho Diamond Token
#[ext_contract(ext_diamond_token)]
pub trait DiamondToken {
    fn mint(&mut self, account_id: AccountId, amount: U128) -> Promise;
    fn ft_balance_of(&self, account_id: AccountId) -> U128;
}

#[ext_contract(ext_self)]
pub trait MiningCallbacks {
    fn claim_reward_callback(&mut self, user_id: AccountId, amount: U128);
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct DiamondMining {
    pub owner_id: AccountId,
    pub diamond_token: AccountId,      // Diamond Token contract
    pub reward_rate: U128,             // Phần thưởng mỗi block/epoch
    pub last_reward_block: u64,        // Block/epoch cuối cùng tính phần thưởng
    pub rewards: LookupMap<AccountId, U128>, // Rewards của mỗi user
    pub last_update: LookupMap<AccountId, u64>, // Lần cuối mỗi user cập nhật
}

#[near_bindgen]
impl DiamondMining {
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
        }
    }

    // Cập nhật phần thưởng cho user
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
            
            log!("User {} updated rewards: +{} DMD", user_id, total_reward.0);
        }
    }

    // Yêu cầu phần thưởng
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

    // Lấy phần thưởng hiện tại của user
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

    // Thay đổi tỷ lệ phần thưởng (chỉ owner)
    #[payable]
    pub fn set_reward_rate(&mut self, reward_rate: U128) {
        self.assert_owner();
        self.reward_rate = reward_rate;
        log!("Reward rate updated to {} DMD per block", reward_rate.0);
    }

    // Force cập nhật phần thưởng cho tất cả các user (chỉ owner)
    #[payable]
    pub fn force_update_global_reward(&mut self) {
        self.assert_owner();
        self.last_reward_block = env::block_height();
        log!("Global reward block updated to {}", self.last_reward_block);
    }

    // Cấu hình token contract (chỉ owner)
    #[payable]
    pub fn set_diamond_token(&mut self, diamond_token: AccountId) {
        self.assert_owner();
        self.diamond_token = diamond_token;
        log!("Diamond Token contract updated to {}", self.diamond_token);
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

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, VMContext};

    fn get_context(predecessor_account_id: AccountId, block_height: u64) -> VMContext {
        let mut builder = VMContextBuilder::new();
        builder.predecessor_account_id(predecessor_account_id);
        builder.current_account_id(accounts(0));
        builder.block_index(block_height);
        builder.build()
    }

    #[test]
    fn test_new() {
        let context = get_context(accounts(1), 100);
        testing_env!(context);
        
        let contract = DiamondMining::new(accounts(2));
        assert_eq!(contract.owner_id, accounts(1));
        assert_eq!(contract.diamond_token, accounts(2));
        assert_eq!(contract.reward_rate.0, 10_000_000_000_000_000_000);
        assert_eq!(contract.last_reward_block, 100);
    }

    #[test]
    fn test_update_rewards() {
        let user = accounts(1);
        let token = accounts(2);
        
        // Khởi tạo ở block 100
        let context = get_context(user.clone(), 100);
        testing_env!(context);
        
        let mut contract = DiamondMining::new(token);
        
        // Đi tới block 110
        let context = get_context(user.clone(), 110);
        testing_env!(context);
        
        contract.update_rewards();
        
        // Phần thưởng cho 10 block = 10 blocks * 10 DMD = 100 DMD
        let reward = contract.get_pending_reward(user.clone());
        assert_eq!(reward.0, 100_000_000_000_000_000_000);
        
        // Đi tiếp tới block 115
        let context = get_context(user.clone(), 115);
        testing_env!(context);
        
        // Phần thưởng thêm cho 5 block = 5 blocks * 10 DMD = 50 DMD
        let reward = contract.get_pending_reward(user.clone());
        assert_eq!(reward.0, 150_000_000_000_000_000_000);
    }
}

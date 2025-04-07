use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, AccountId, Balance, BorshStorageKey, require};
use near_sdk::serde::{Deserialize, Serialize};

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub struct ChainSupply {
    pub chain_id: u16,
    pub chain_name: String,
    pub supply: U128,
    pub last_updated: u64,
}

#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    SupplyByChain,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct SupplyTracker {
    // Chain ID -> Supply
    supplies: LookupMap<u16, ChainSupply>,
    // Chain ID của NEAR Protocol
    near_chain_id: u16,
    // Tổng cung tối đa
    max_supply: Balance,
    // Owner có quyền cập nhật
    owner_id: AccountId,
}

#[near_bindgen]
impl SupplyTracker {
    pub fn new(storage_key: Vec<u8>, near_chain_id: u16, max_supply: Balance, owner_id: AccountId) -> Self {
        Self {
            supplies: LookupMap::new(storage_key),
            near_chain_id,
            max_supply,
            owner_id,
        }
    }

    // Cập nhật số lượng token trên một chain
    pub fn update_chain_supply(&mut self, chain_id: u16, chain_name: String, supply: Balance) -> bool {
        let current_account_id = env::predecessor_account_id();
        
        // Chỉ có owner mới có quyền cập nhật
        if current_account_id != self.owner_id {
            return false;
        }
        
        let chain_supply = ChainSupply {
            chain_id,
            chain_name,
            supply: U128(supply),
            last_updated: env::block_timestamp(),
        };
        
        self.supplies.insert(&chain_id, &chain_supply);
        true
    }
    
    // Kiểm tra xem oracle có được phép cập nhật không
    pub fn is_authorized_oracle(&self, account_id: &AccountId) -> bool {
        account_id == &self.owner_id
    }

    // Lấy tổng cung của token trên tất cả các chain
    pub fn get_total_circulating_supply(&self) -> Balance {
        let mut total: Balance = 0;
        for (_, supply) in self.supplies.iter() {
            total += supply.supply.0;
        }
        total
    }

    // Kiểm tra xem có thể mint thêm token hay không
    pub fn can_mint_additional(&self, amount: Balance) -> bool {
        let current_total = self.get_total_circulating_supply();
        current_total + amount <= self.max_supply
    }

    // Lấy số lượng token trên một chain cụ thể
    pub fn get_chain_supply(&self, chain_id: u16) -> Option<ChainSupply> {
        self.supplies.get(&chain_id)
    }
    
    // Lấy danh sách tất cả chuỗi và số lượng token
    pub fn get_all_chain_supplies(&self) -> Vec<ChainSupply> {
        self.supplies.iter().map(|(_, supply)| supply).collect()
    }
    
    // Đặt chain ID của NEAR
    pub fn set_near_chain_id(&mut self, near_chain_id: u16) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can update NEAR chain ID"
        );
        self.near_chain_id = near_chain_id;
    }
    
    // Lấy chain ID của NEAR
    pub fn get_near_chain_id(&self) -> u16 {
        self.near_chain_id
    }
    
    // Lấy tổng cung tối đa
    pub fn get_max_supply(&self) -> U128 {
        U128(self.max_supply)
    }
    
    // Cập nhật tổng cung tối đa (nếu cần)
    pub fn set_max_supply(&mut self, max_supply: U128) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can update max supply"
        );
        self.max_supply = max_supply.0;
    }
    
    // Chuyển quyền sở hữu
    pub fn transfer_ownership(&mut self, new_owner_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can transfer ownership"
        );
        self.owner_id = new_owner_id;
    }
}

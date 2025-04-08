use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::FungibleToken;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedMap};
use near_sdk::json_types::U128;
use near_sdk::{
    env, log, AccountId, Balance, BorshStorageKey, NearToken, PanicOnDefault, Promise, PromiseOrValue,
};
use near_sdk::{near_bindgen, require};

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

mod bridge;
mod supply_tracker;
mod module_manager;

use bridge::{BridgeAdapter, ChainConfig, LayerZeroParams, lz_send, SupportedChain};
use supply_tracker::{SupplyTracker, ChainSupply};
use module_manager::{ModuleManager, Module, ModuleStatus};

/// Token Diamond chính của hệ thống
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, Clone)]
pub struct DiamondToken {
    /// Fungible token contract
    pub token: FungibleToken,
    /// Metadata của token
    pub metadata: LazyOption<FungibleTokenMetadata>,
    /// ID của chủ sở hữu
    pub owner_id: AccountId,
    /// Tổng cung tối đa
    pub max_supply: Balance,
    /// Cấu hình cho các chain
    pub chain_configs: LookupMap<u16, ChainConfig>,
    /// Endpoint của LayerZero
    pub lz_endpoint: AccountId,
    /// Theo dõi nguồn cung
    pub supply_tracker: SupplyTracker,
    /// ID của chain NEAR
    pub near_chain_id: u16,
    /// Quản lý các module
    pub module_manager: ModuleManager,
    /// Thời gian cập nhật cuối cùng
    pub last_update: u64,
}

/// Các hằng số
const MAX_SUPPLY: Balance = 1_000_000_000_000_000_000_000_000_000; // 1B tokens (18 decimals)
const INITIAL_SUPPLY: Balance = 250_000_000_000_000_000_000_000_000; // 250M tokens (18 decimals)

/// Storage keys cho contract
#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
    FungibleToken,
    Metadata,
    ChainConfigs,
    SupplyTracker,
    ModuleManager,
}

#[near_bindgen]
impl DiamondToken {
    /// Khởi tạo token Diamond mới
    /// 
    /// # Arguments
    /// 
    /// * `owner_id` - ID của chủ sở hữu
    /// * `lz_endpoint` - Endpoint của LayerZero
    /// * `near_chain_id` - ID của chain NEAR
    /// 
    /// # Returns
    /// 
    /// * `Self` - Instance mới của DiamondToken
    #[init]
    pub fn new(owner_id: AccountId, lz_endpoint: AccountId, near_chain_id: u16) -> Self {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        let metadata = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Diamond Token".to_string(),
            symbol: "DMD".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 18,
        };
        
        let supply_tracker = SupplyTracker::new(
            StorageKey::SupplyTracker.try_to_vec().unwrap(), 
            near_chain_id, 
            MAX_SUPPLY,
            owner_id.clone()
        );
        
        let module_manager = ModuleManager::new(
            StorageKey::ModuleManager.try_to_vec().unwrap(),
            owner_id.clone(),
            None,
        );
        
        let mut this = Self {
            token: FungibleToken::new(StorageKey::FungibleToken),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
            owner_id: owner_id.clone(),
            max_supply: MAX_SUPPLY,
            chain_configs: LookupMap::new(StorageKey::ChainConfigs),
            lz_endpoint,
            supply_tracker,
            near_chain_id,
            module_manager,
            last_update: current_time,
        };
        
        this.token.internal_register_account(&owner_id);
        this.token.internal_deposit(&owner_id, INITIAL_SUPPLY);
        
        this.supply_tracker.update_chain_supply(
            near_chain_id, 
            "NEAR Protocol".to_string(), 
            INITIAL_SUPPLY
        );
        
        log!("Minted {} DMD tokens to {}", INITIAL_SUPPLY, owner_id);
        
        this
    }
    
    /// Chuyển token
    /// 
    /// # Arguments
    /// 
    /// * `receiver_id` - ID người nhận
    /// * `amount` - Số lượng token
    /// * `memo` - Ghi chú
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        
        self.token.internal_transfer(&sender_id, &receiver_id, amount, memo);
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Mint token mới
    /// 
    /// # Arguments
    /// 
    /// * `account_id` - ID tài khoản nhận
    /// * `amount` - Số lượng token
    #[payable]
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        self.assert_owner();
        let amount: Balance = amount.into();
        let current_supply = self.ft_total_supply().0;
        
        require!(
            current_supply + amount <= self.max_supply,
            "Cannot mint more than max supply"
        );
        
        let total_circulating = self.supply_tracker.get_total_circulating_supply();
        require!(
            total_circulating + amount <= self.max_supply,
            "Cannot mint more than global max supply"
        );
        
        self.token.internal_deposit(&account_id, amount);
        
        if let Some(mut chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                chain_supply.supply.0 + amount
            );
        }
        
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        log!("Minted {} DMD tokens to {}", amount, account_id);
    }
    
    /// Burn token
    /// 
    /// # Arguments
    /// 
    /// * `amount` - Số lượng token
    #[payable]
    pub fn burn(&mut self, amount: U128) {
        let account_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        
        self.token.internal_withdraw(&account_id, amount);
        
        if let Some(mut chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            let new_supply = chain_supply.supply.0 - amount;
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                new_supply
            );
        }
        
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        log!("Burned {} DMD tokens from {}", amount, account_id);
    }
    
    /// Kiểm tra quyền sở hữu
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this method"
        );
    }
    
    /// Lấy tổng cung tối đa
    pub fn get_max_supply(&self) -> U128 {
        self.max_supply.into()
    }
    
    /// Lấy ID chủ sở hữu
    pub fn get_owner(&self) -> AccountId {
        self.owner_id.clone()
    }
    
    /// Cập nhật LayerZero endpoint
    /// 
    /// # Arguments
    /// 
    /// * `lz_endpoint` - Endpoint mới
    #[payable]
    pub fn set_lz_endpoint(&mut self, lz_endpoint: AccountId) {
        self.assert_owner();
        self.lz_endpoint = lz_endpoint;
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Lấy tổng cung lưu thông toàn cầu
    pub fn get_global_circulating_supply(&self) -> U128 {
        U128(self.supply_tracker.get_total_circulating_supply())
    }
    
    /// Lấy tổng cung lưu thông trên NEAR
    pub fn get_near_circulating_supply(&self) -> U128 {
        if let Some(chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            chain_supply.supply
        } else {
            U128(0)
        }
    }
    
    /// Thêm admin cho module
    /// 
    /// # Arguments
    /// 
    /// * `admin_id` - ID của admin
    /// 
    /// # Returns
    /// 
    /// * `bool` - Kết quả thêm admin
    #[payable]
    pub fn add_module_admin(&mut self, admin_id: AccountId) -> bool {
        self.assert_owner();
        let result = self.module_manager.add_admin(admin_id);
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        result
    }
    
    /// Thêm module mới
    /// 
    /// # Arguments
    /// 
    /// * `module` - Module cần thêm
    /// 
    /// # Returns
    /// 
    /// * `bool` - Kết quả thêm module
    #[payable]
    pub fn add_module(&mut self, module: Module) -> bool {
        self.assert_owner();
        let result = self.module_manager.add_module(module);
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        result
    }
    
    /// Lấy danh sách module đang hoạt động
    pub fn get_active_modules(&self) -> Vec<Module> {
        self.module_manager.get_active_modules()
    }
    
    /// Chuyển quyền sở hữu
    /// 
    /// # Arguments
    /// 
    /// * `new_owner_id` - ID chủ sở hữu mới
    #[payable]
    pub fn transfer_ownership(&mut self, new_owner_id: AccountId) {
        self.assert_owner();
        
        self.supply_tracker.transfer_ownership(new_owner_id.clone());
        self.module_manager.transfer_ownership(new_owner_id.clone());
        
        self.owner_id = new_owner_id;
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Gọi hàm của module
    /// 
    /// # Arguments
    /// 
    /// * `module_name` - Tên module
    /// * `function_name` - Tên hàm
    /// * `args` - Tham số
    /// 
    /// # Returns
    /// 
    /// * `Promise` - Promise thực thi
    #[payable]
    pub fn call_module_function(
        &self, 
        module_name: String, 
        function_name: String, 
        args: String
    ) -> Promise {
        self.assert_owner();
        
        let deposit = env::attached_deposit();
        let gas = env::prepaid_gas() - env::used_gas() - Gas(30_000_000_000_000);
        
        self.module_manager.call_module_function(
            &module_name,
            &function_name,
            args,
            deposit,
            gas.0
        )
    }
}

impl BridgeAdapter for DiamondToken {
    fn bridge_out(&mut self, to: String, amount: U128, lz_params: LayerZeroParams) -> Promise {
        let sender_id = env::predecessor_account_id();
        let amount_u128: Balance = amount.into();
        
        let chain_config = self.chain_configs.get(&lz_params.dest_chain_id)
            .expect("Chain not supported");
            
        require!(chain_config.enabled, "Bridge to this chain is disabled");
        
        let fee_amount = self.estimate_bridge_fee(lz_params.dest_chain_id, amount).0;
        
        require!(
            lz_params.fees.0 >= fee_amount,
            format!("Insufficient bridge fee. Required: {}", fee_amount)
        );
        
        self.token.internal_withdraw(&sender_id, amount_u128);
        
        if let Some(chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            let new_supply = chain_supply.supply.0 - amount_u128;
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                new_supply
            );
        }
        
        log!("Burned {} DMD tokens from {} for bridge", amount_u128, sender_id);
        
        let payload = near_sdk::serde_json::to_vec(&serde_json::json!({
            "from": sender_id.to_string(),
            "to": to,
            "amount": amount,
        })).unwrap();
        
        let dest_address = chain_config.remote_bridge_address.as_bytes().to_vec();
        
        lz_send(
            self.lz_endpoint.clone(),
            lz_params.dest_chain_id,
            dest_address,
            payload,
            sender_id,
            lz_params.adapter_params,
            lz_params.fees.0
        )
    }
    
    fn bridge_in(&mut self, from_chain_id: u16, sender: String, receiver_id: AccountId, amount: U128) -> PromiseOrValue<bool> {
        require!(
            env::predecessor_account_id() == self.lz_endpoint,
            "Only LZ endpoint can call this method"
        );
        
        let chain_config = self.chain_configs.get(&from_chain_id)
            .expect("Chain not supported");
            
        require!(chain_config.enabled, "Bridge from this chain is disabled");
        
        let amount_u128: Balance = amount.into();
        
        let current_supply = self.ft_total_supply().0;
        require!(
            current_supply + amount_u128 <= self.max_supply,
            "Bridge would exceed max supply"
        );
        
        let total_circulating = self.supply_tracker.get_total_circulating_supply();
        require!(
            total_circulating + amount_u128 <= self.max_supply,
            "Bridge would exceed global max supply"
        );
        
        if !self.token.accounts.contains_key(&receiver_id) {
            self.token.internal_register_account(&receiver_id);
        }
        
        self.token.internal_deposit(&receiver_id, amount_u128);
        
        if let Some(chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            let new_supply = chain_supply.supply.0 + amount_u128;
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                new_supply
            );
        }
        
        log!("Bridged in {} DMD tokens to {} from chain {} (sender: {})",
             amount_u128, receiver_id, from_chain_id, sender);
             
        PromiseOrValue::Value(true)
    }
    
    fn set_chain_config(&mut self, chain_id: u16, config: ChainConfig) {
        self.assert_owner();
        self.chain_configs.insert(&chain_id, &config);
        
        if self.supply_tracker.get_chain_supply(chain_id).is_none() {
            self.supply_tracker.update_chain_supply(
                chain_id,
                config.chain_name,
                0
            );
        }
        
        log!("Chain config updated for chain ID: {}", chain_id);
    }
    
    fn get_chain_config(&self, chain_id: u16) -> Option<ChainConfig> {
        self.chain_configs.get(&chain_id)
    }
    
    fn estimate_bridge_fee(&self, dest_chain_id: u16, amount: U128) -> U128 {
        let chain_config = match self.chain_configs.get(&dest_chain_id) {
            Some(config) => config,
            None => return U128(0),
        };
        
        let amount_u128: Balance = amount.into();
        let fee = (amount_u128 * chain_config.fee_basis_points as u128) / 10000;
        
        let fee = if fee < chain_config.min_fee.0 {
            chain_config.min_fee.0
        } else if fee > chain_config.max_fee.0 {
            chain_config.max_fee.0
        } else {
            fee
        };
        
        U128(fee)
    }
    
    fn get_total_circulating_supply(&self) -> U128 {
        U128(self.supply_tracker.get_total_circulating_supply())
    }
    
    fn get_chain_supplies(&self) -> Vec<(u16, String, U128)> {
        self.supply_tracker.get_all_chain_supplies()
            .into_iter()
            .map(|supply| (supply.chain_id, supply.chain_name, supply.supply))
            .collect()
    }
    
    fn update_remote_chain_supply(&mut self, chain_id: u16, supply: U128) -> bool {
        let caller = env::predecessor_account_id();
        let is_authorized = self.owner_id == caller 
            || self.chain_configs.get(&chain_id)
                .and_then(|config| config.supply_oracle)
                .map_or(false, |oracle| oracle == caller);
                
        if !is_authorized {
            return false;
        }
        
        if let Some(chain_config) = self.chain_configs.get(&chain_id) {
            self.supply_tracker.update_chain_supply(
                chain_id,
                chain_config.chain_name,
                supply.0
            );
            true
        } else {
            false
        }
    }
    
    fn set_supply_oracle(&mut self, chain_id: u16, oracle: AccountId) -> bool {
        self.assert_owner();
        
        if let Some(mut config) = self.chain_configs.get(&chain_id) {
            config.supply_oracle = Some(oracle);
            self.chain_configs.insert(&chain_id, &config);
            true
        } else {
            false
        }
    }
}

near_contract_standards::impl_fungible_token_core!(DiamondToken, token);
near_contract_standards::impl_fungible_token_storage!(DiamondToken, token);

#[near_bindgen]
impl FungibleTokenMetadataProvider for DiamondToken {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_context, VMContextBuilder};
    use near_sdk::{testing_env, AccountId, Balance, Gas};

    /// Test khởi tạo DiamondToken
    #[test]
    fn test_new() {
        let owner_id = AccountId::new_unchecked("owner.testnet".to_string());
        let lz_endpoint = AccountId::new_unchecked("lz.testnet".to_string());
        let near_chain_id = 1;
        
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let token = DiamondToken::new(owner_id.clone(), lz_endpoint.clone(), near_chain_id);
        
        assert_eq!(token.owner_id, owner_id);
        assert_eq!(token.lz_endpoint, lz_endpoint);
        assert_eq!(token.near_chain_id, near_chain_id);
        assert_eq!(token.max_supply, MAX_SUPPLY);
        assert_eq!(token.ft_total_supply().0, INITIAL_SUPPLY);
        assert!(token.last_update > 0);
    }

    /// Test chuyển token
    #[test]
    fn test_transfer() {
        let owner_id = AccountId::new_unchecked("owner.testnet".to_string());
        let lz_endpoint = AccountId::new_unchecked("lz.testnet".to_string());
        let near_chain_id = 1;
        let receiver_id = AccountId::new_unchecked("receiver.testnet".to_string());
        
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let mut token = DiamondToken::new(owner_id.clone(), lz_endpoint.clone(), near_chain_id);
        
        let amount = U128(100_000_000_000_000_000_000_000_000); // 100M tokens
        
        token.ft_transfer(receiver_id.clone(), amount, None);
        
        assert_eq!(token.ft_balance_of(owner_id).0, INITIAL_SUPPLY - amount.0);
        assert_eq!(token.ft_balance_of(receiver_id).0, amount.0);
        assert!(token.last_update > 0);
    }

    /// Test cấu hình bridge
    #[test]
    fn test_bridge_config() {
        let owner_id = AccountId::new_unchecked("owner.testnet".to_string());
        let lz_endpoint = AccountId::new_unchecked("lz.testnet".to_string());
        let near_chain_id = 1;
        let new_lz_endpoint = AccountId::new_unchecked("new_lz.testnet".to_string());
        
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let mut token = DiamondToken::new(owner_id.clone(), lz_endpoint.clone(), near_chain_id);
        
        token.set_lz_endpoint(new_lz_endpoint.clone());
        
        assert_eq!(token.lz_endpoint, new_lz_endpoint);
        assert!(token.last_update > 0);
    }

    /// Test tổng cung toàn cầu
    #[test]
    fn test_global_supply() {
        let owner_id = AccountId::new_unchecked("owner.testnet".to_string());
        let lz_endpoint = AccountId::new_unchecked("lz.testnet".to_string());
        let near_chain_id = 1;
        
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let token = DiamondToken::new(owner_id.clone(), lz_endpoint.clone(), near_chain_id);
        
        assert_eq!(token.get_global_circulating_supply().0, INITIAL_SUPPLY);
        assert_eq!(token.get_near_circulating_supply().0, INITIAL_SUPPLY);
    }
}

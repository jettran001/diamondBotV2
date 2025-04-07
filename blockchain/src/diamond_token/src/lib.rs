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

mod bridge;
mod supply_tracker;
mod module_manager;

use bridge::{BridgeAdapter, ChainConfig, LayerZeroParams, lz_send, SupportedChain};
use supply_tracker::{SupplyTracker, ChainSupply};
use module_manager::{ModuleManager, Module, ModuleStatus};

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct DiamondToken {
    token: FungibleToken,
    metadata: LazyOption<FungibleTokenMetadata>,
    owner_id: AccountId,
    max_supply: Balance,
    // Quản lý bridge
    chain_configs: LookupMap<u16, ChainConfig>,
    lz_endpoint: AccountId,
    // Theo dõi tổng cung trên các chain
    supply_tracker: SupplyTracker,
    // Near chain ID
    near_chain_id: u16,
    // Quản lý module mở rộng
    module_manager: ModuleManager,
}

const MAX_SUPPLY: Balance = 1_000_000_000_000_000_000_000_000_000; // 1B tokens (18 decimals)
const INITIAL_SUPPLY: Balance = 250_000_000_000_000_000_000_000_000; // 250M tokens (18 decimals)

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
    #[init]
    pub fn new(owner_id: AccountId, lz_endpoint: AccountId, near_chain_id: u16) -> Self {
        let metadata = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Diamond Token".to_string(),
            symbol: "DMD".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 18,
        };
        
        // Khởi tạo SupplyTracker
        let supply_tracker = SupplyTracker::new(
            StorageKey::SupplyTracker.try_to_vec().unwrap(), 
            near_chain_id, 
            MAX_SUPPLY,
            owner_id.clone()
        );
        
        // Khởi tạo ModuleManager
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
        };
        
        // Mint initial supply to owner
        this.token.internal_register_account(&owner_id);
        this.token.internal_deposit(&owner_id, INITIAL_SUPPLY);
        
        // Cập nhật tổng cung trên NEAR
        this.supply_tracker.update_chain_supply(
            near_chain_id, 
            "NEAR Protocol".to_string(), 
            INITIAL_SUPPLY
        );
        
        // Log mint event
        log!("Minted {} DMD tokens to {}", INITIAL_SUPPLY, owner_id);
        
        this
    }
    
    // Chuyển token không có thuế
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        
        // Chuyển trực tiếp không tính thuế
        self.token.internal_transfer(&sender_id, &receiver_id, amount, memo);
    }
    
    // Mint thêm token (chỉ owner)
    #[payable]
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        self.assert_owner();
        let amount: Balance = amount.into();
        let current_supply = self.ft_total_supply().0;
        
        require!(
            current_supply + amount <= self.max_supply,
            "Cannot mint more than max supply"
        );
        
        // Kiểm tra với tổng cung trên tất cả các chain
        let total_circulating = self.supply_tracker.get_total_circulating_supply();
        require!(
            total_circulating + amount <= self.max_supply,
            "Cannot mint more than global max supply"
        );
        
        self.token.internal_deposit(&account_id, amount);
        
        // Cập nhật tổng cung trên NEAR
        if let Some(mut chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                chain_supply.supply.0 + amount
            );
        }
        
        log!("Minted {} DMD tokens to {}", amount, account_id);
    }
    
    // Đốt token
    #[payable]
    pub fn burn(&mut self, amount: U128) {
        let account_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        
        self.token.internal_withdraw(&account_id, amount);
        
        // Cập nhật tổng cung trên NEAR
        if let Some(mut chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            let new_supply = chain_supply.supply.0 - amount;
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                new_supply
            );
        }
        
        log!("Burned {} DMD tokens from {}", amount, account_id);
    }
    
    // Kiểm tra người gọi có phải là owner không
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this method"
        );
    }
    
    // Lấy tổng cung tối đa
    pub fn get_max_supply(&self) -> U128 {
        self.max_supply.into()
    }
    
    // Lấy thông tin owner
    pub fn get_owner(&self) -> AccountId {
        self.owner_id.clone()
    }
    
    // Thay đổi LayerZero endpoint
    #[payable]
    pub fn set_lz_endpoint(&mut self, lz_endpoint: AccountId) {
        self.assert_owner();
        self.lz_endpoint = lz_endpoint;
    }
    
    // Lấy tổng cung trên tất cả các chain
    pub fn get_global_circulating_supply(&self) -> U128 {
        U128(self.supply_tracker.get_total_circulating_supply())
    }
    
    // Lấy tổng cung trên NEAR
    pub fn get_near_circulating_supply(&self) -> U128 {
        if let Some(chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            chain_supply.supply
        } else {
            U128(0)
        }
    }
    
    // Thêm admin cho module manager
    #[payable]
    pub fn add_module_admin(&mut self, admin_id: AccountId) -> bool {
        self.assert_owner();
        self.module_manager.add_admin(admin_id)
    }
    
    // Thêm module mới
    #[payable]
    pub fn add_module(&mut self, module: Module) -> bool {
        self.assert_owner();
        self.module_manager.add_module(module)
    }
    
    // Lấy tất cả module đang hoạt động
    pub fn get_active_modules(&self) -> Vec<Module> {
        self.module_manager.get_active_modules()
    }
    
    // Chuyển quyền sở hữu
    #[payable]
    pub fn transfer_ownership(&mut self, new_owner_id: AccountId) {
        self.assert_owner();
        
        // Cập nhật owner trong các module
        self.supply_tracker.transfer_ownership(new_owner_id.clone());
        self.module_manager.transfer_ownership(new_owner_id.clone());
        
        self.owner_id = new_owner_id;
    }
    
    // Gọi hàm từ module
    #[payable]
    pub fn call_module_function(
        &self, 
        module_name: String, 
        function_name: String, 
        args: String
    ) -> Promise {
        self.assert_owner();
        
        let deposit = env::attached_deposit();
        let gas = env::prepaid_gas() - env::used_gas() - Gas(30_000_000_000_000); // Giữ lại gas dự phòng
        
        self.module_manager.call_module_function(
            &module_name,
            &function_name,
            args,
            deposit,
            gas.0
        )
    }
}

// Triển khai BridgeAdapter cho DiamondToken
#[near_bindgen]
impl BridgeAdapter for DiamondToken {
    /// Gửi token từ NEAR đến chuỗi khác thông qua LayerZero
    fn bridge_out(&mut self, to: String, amount: U128, lz_params: LayerZeroParams) -> Promise {
        let sender_id = env::predecessor_account_id();
        let amount_u128: Balance = amount.into();
        
        // Kiểm tra cấu hình chuỗi đích
        let chain_config = self.chain_configs.get(&lz_params.dest_chain_id)
            .expect("Chain not supported");
            
        require!(chain_config.enabled, "Bridge to this chain is disabled");
        
        // Tính phí bridge
        let fee_amount = self.estimate_bridge_fee(lz_params.dest_chain_id, amount).0;
        
        // Kiểm tra phí gửi đủ
        require!(
            lz_params.fees.0 >= fee_amount,
            format!("Insufficient bridge fee. Required: {}", fee_amount)
        );
        
        // Đốt token trên chain nguồn
        self.token.internal_withdraw(&sender_id, amount_u128);
        
        // Cập nhật tổng cung trên NEAR
        if let Some(chain_supply) = self.supply_tracker.get_chain_supply(self.near_chain_id) {
            let new_supply = chain_supply.supply.0 - amount_u128;
            self.supply_tracker.update_chain_supply(
                self.near_chain_id,
                chain_supply.chain_name,
                new_supply
            );
        }
        
        log!("Burned {} DMD tokens from {} for bridge", amount_u128, sender_id);
        
        // Chuẩn bị payload để gửi qua LayerZero
        let payload = near_sdk::serde_json::to_vec(&serde_json::json!({
            "from": sender_id.to_string(),
            "to": to,
            "amount": amount,
        })).unwrap();
        
        // Chuyển đổi địa chỉ đích sang bytes
        let dest_address = chain_config.remote_bridge_address.as_bytes().to_vec();
        
        // Gọi hàm lz_send để gửi message cross-chain
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
    
    /// Nhận token từ chuỗi khác
    fn bridge_in(&mut self, from_chain_id: u16, sender: String, receiver_id: AccountId, amount: U128) -> PromiseOrValue<bool> {
        // Chỉ cho phép LayerZero endpoint gọi hàm này
        require!(
            env::predecessor_account_id() == self.lz_endpoint,
            "Only LZ endpoint can call this method"
        );
        
        // Kiểm tra chuỗi nguồn được hỗ trợ
        let chain_config = self.chain_configs.get(&from_chain_id)
            .expect("Chain not supported");
            
        require!(chain_config.enabled, "Bridge from this chain is disabled");
        
        // Mint token trên chain đích
        let amount_u128: Balance = amount.into();
        
        // Đảm bảo tổng cung không vượt quá giới hạn
        let current_supply = self.ft_total_supply().0;
        require!(
            current_supply + amount_u128 <= self.max_supply,
            "Bridge would exceed max supply"
        );
        
        // Kiểm tra với tổng cung trên tất cả các chain
        let total_circulating = self.supply_tracker.get_total_circulating_supply();
        require!(
            total_circulating + amount_u128 <= self.max_supply,
            "Bridge would exceed global max supply"
        );
        
        // Đăng ký tài khoản nếu chưa có
        if !self.token.accounts.contains_key(&receiver_id) {
            self.token.internal_register_account(&receiver_id);
        }
        
        // Mint token cho người nhận
        self.token.internal_deposit(&receiver_id, amount_u128);
        
        // Cập nhật tổng cung trên NEAR
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
    
    /// Thiết lập cấu hình bridge cho một chuỗi
    fn set_chain_config(&mut self, chain_id: u16, config: ChainConfig) {
        self.assert_owner();
        self.chain_configs.insert(&chain_id, &config);
        
        // Đảm bảo chain cũng được thêm vào supply tracker
        if self.supply_tracker.get_chain_supply(chain_id).is_none() {
            self.supply_tracker.update_chain_supply(
                chain_id,
                config.chain_name,
                0 // Ban đầu không có token trên chain mới
            );
        }
        
        log!("Chain config updated for chain ID: {}", chain_id);
    }
    
    /// Lấy cấu hình bridge cho một chuỗi
    fn get_chain_config(&self, chain_id: u16) -> Option<ChainConfig> {
        self.chain_configs.get(&chain_id)
    }
    
    /// Tính phí bridge cho một giao dịch
    fn estimate_bridge_fee(&self, dest_chain_id: u16, amount: U128) -> U128 {
        let chain_config = match self.chain_configs.get(&dest_chain_id) {
            Some(config) => config,
            None => return U128(0),
        };
        
        // Tính phí dựa trên số lượng token và fee_basis_points
        let amount_u128: Balance = amount.into();
        let fee = (amount_u128 * chain_config.fee_basis_points as u128) / 10000;
        
        // Đảm bảo phí nằm trong khoảng min_fee và max_fee
        let fee = if fee < chain_config.min_fee.0 {
            chain_config.min_fee.0
        } else if fee > chain_config.max_fee.0 {
            chain_config.max_fee.0
        } else {
            fee
        };
        
        U128(fee)
    }
    
    /// Lấy tổng cung trên tất cả các chain
    fn get_total_circulating_supply(&self) -> U128 {
        U128(self.supply_tracker.get_total_circulating_supply())
    }
    
    /// Lấy số lượng token trên từng chain
    fn get_chain_supplies(&self) -> Vec<(u16, String, U128)> {
        self.supply_tracker.get_all_chain_supplies()
            .into_iter()
            .map(|supply| (supply.chain_id, supply.chain_name, supply.supply))
            .collect()
    }
    
    /// Cập nhật thông tin tổng cung từ chain khác
    fn update_remote_chain_supply(&mut self, chain_id: u16, supply: U128) -> bool {
        // Phải là owner hoặc oracle được ủy quyền
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
    
    /// Thiết lập oracle cho một chuỗi
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

// Triển khai các hàm tiêu chuẩn của NEP-141 Fungible Token
near_contract_standards::impl_fungible_token_core!(DiamondToken, token);
near_contract_standards::impl_fungible_token_storage!(DiamondToken, token);

#[near_bindgen]
impl FungibleTokenMetadataProvider for DiamondToken {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::{testing_env, VMContext};
    
    fn get_context(predecessor_account_id: AccountId) -> VMContext {
        VMContextBuilder::new()
            .predecessor_account_id(predecessor_account_id)
            .build()
    }
    
    const NEAR_CHAIN_ID: u16 = 1313161554; // NEAR mainnet chain ID
    
    #[test]
    fn test_new() {
        let owner = accounts(0);
        let lz_endpoint = accounts(1);
        let context = get_context(owner.clone());
        testing_env!(context);
        
        let contract = DiamondToken::new(owner.clone(), lz_endpoint, NEAR_CHAIN_ID);
        assert_eq!(contract.ft_balance_of(owner.clone()).0, INITIAL_SUPPLY);
        assert_eq!(contract.get_owner(), owner);
        assert_eq!(contract.get_global_circulating_supply().0, INITIAL_SUPPLY);
    }
    
    #[test]
    fn test_transfer() {
        let owner = accounts(0);
        let user = accounts(1);
        let lz_endpoint = accounts(4);
        let context = get_context(owner.clone());
        testing_env!(context);
        
        let mut contract = DiamondToken::new(owner.clone(), lz_endpoint, NEAR_CHAIN_ID);
        
        // Đăng ký tài khoản mới
        contract.storage_deposit(Some(user.clone()), None);
        
        // Chuyển token từ owner đến user
        let transfer_amount: Balance = 1_000_000_000_000_000_000_000_000; // 1M tokens
        contract.ft_transfer(user.clone(), transfer_amount.into(), None);
        
        // Kiểm tra số dư
        assert_eq!(contract.ft_balance_of(user.clone()).0, transfer_amount);
        
        // Chuyển token giữa các người dùng không có thuế
        let context = get_context(user.clone());
        testing_env!(context);
        
        // Đăng ký tài khoản người nhận
        contract.storage_deposit(Some(accounts(3)), None);
        
        let user_transfer = 100_000_000_000_000_000_000_000; // 100k tokens
        contract.ft_transfer(accounts(3), user_transfer.into(), None);
        
        // Kiểm tra số dư
        assert_eq!(contract.ft_balance_of(accounts(3)).0, user_transfer);
        assert_eq!(contract.ft_balance_of(user.clone()).0, transfer_amount - user_transfer);
    }
    
    #[test]
    fn test_bridge_config() {
        let owner = accounts(0);
        let lz_endpoint = accounts(1);
        let context = get_context(owner.clone());
        testing_env!(context);
        
        let mut contract = DiamondToken::new(owner.clone(), lz_endpoint, NEAR_CHAIN_ID);
        
        // Thêm cấu hình cho Ethereum (chain ID 1)
        let eth_config = ChainConfig {
            remote_chain_id: 1,
            remote_bridge_address: "0x1234567890123456789012345678901234567890".to_string(),
            chain_name: "Ethereum".to_string(),
            enabled: true,
            fee_basis_points: 50, // 0.5%
            min_fee: U128(1_000_000_000_000_000_000), // 1 token tối thiểu
            max_fee: U128(100_000_000_000_000_000_000), // 100 token tối đa
            supply_oracle: None,
        };
        
        contract.set_chain_config(1, eth_config.clone());
        
        // Kiểm tra cấu hình đã được thiết lập
        let stored_config = contract.get_chain_config(1).unwrap();
        assert_eq!(stored_config.remote_chain_id, eth_config.remote_chain_id);
        assert_eq!(stored_config.remote_bridge_address, eth_config.remote_bridge_address);
        assert_eq!(stored_config.enabled, eth_config.enabled);
        
        // Kiểm tra tính phí
        let bridge_amount = U128(1_000_000_000_000_000_000_000); // 1000 tokens
        let expected_fee = U128(5_000_000_000_000_000_000); // 5 tokens (0.5%)
        assert_eq!(contract.estimate_bridge_fee(1, bridge_amount), expected_fee);
    }
    
    #[test]
    fn test_global_supply() {
        let owner = accounts(0);
        let lz_endpoint = accounts(1);
        let eth_oracle = accounts(2);
        let context = get_context(owner.clone());
        testing_env!(context);
        
        let mut contract = DiamondToken::new(owner.clone(), lz_endpoint, NEAR_CHAIN_ID);
        
        // Ban đầu chỉ có supply trên NEAR
        assert_eq!(contract.get_global_circulating_supply().0, INITIAL_SUPPLY);
        
        // Thêm chain Ethereum với oracle
        let eth_config = ChainConfig {
            remote_chain_id: 1,
            remote_bridge_address: "0x1234567890123456789012345678901234567890".to_string(),
            chain_name: "Ethereum".to_string(),
            enabled: true,
            fee_basis_points: 50,
            min_fee: U128(1_000_000_000_000_000_000),
            max_fee: U128(100_000_000_000_000_000_000),
            supply_oracle: Some(eth_oracle.clone()),
        };
        
        contract.set_chain_config(1, eth_config.clone());
        
        // Đặt context là oracle
        let context = get_context(eth_oracle.clone());
        testing_env!(context);
        
        // Cập nhật supply trên Ethereum
        let eth_supply = 50_000_000_000_000_000_000_000_000; // 50M tokens
        assert!(contract.update_remote_chain_supply(1, U128(eth_supply)));
        
        // Tổng cung toàn cầu bao gồm cả token trên Ethereum
        assert_eq!(contract.get_global_circulating_supply().0, INITIAL_SUPPLY + eth_supply);
        
        // Kiểm tra danh sách các chain
        let supplies = contract.get_chain_supplies();
        assert_eq!(supplies.len(), 2);
        
        let near_supply = supplies.iter().find(|(id, _, _)| *id == NEAR_CHAIN_ID).unwrap().2;
        let eth_supply_from_list = supplies.iter().find(|(id, _, _)| *id == 1).unwrap().2;
        
        assert_eq!(near_supply.0, INITIAL_SUPPLY);
        assert_eq!(eth_supply_from_list.0, eth_supply);
    }
}

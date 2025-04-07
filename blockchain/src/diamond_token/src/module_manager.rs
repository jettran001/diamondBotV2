use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap, Vector};
use near_sdk::{env, near_bindgen, AccountId, Promise, PromiseResult, require};
use near_sdk::serde::{Deserialize, Serialize};

/// Trạng thái của module
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum ModuleStatus {
    Active,
    Paused,
    Deprecated,
}

/// Thông tin về một module
#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct Module {
    pub name: String,
    pub contract_id: AccountId,
    pub version: String,
    pub status: ModuleStatus,
    pub added_timestamp: u64,
    pub last_updated: u64,
    pub metadata: String, // JSON string với thông tin bổ sung
}

/// Quản lý các module mở rộng
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct ModuleManager {
    // Các module hiện tại (name -> Module)
    modules: UnorderedMap<String, Module>,
    // Danh sách các khoá module theo thứ tự
    module_keys: Vector<String>,
    // Các module được ủy quyền gọi hàm của token
    authorized_callers: LookupMap<AccountId, Vec<String>>,
    // Owner của system
    owner_id: AccountId,
    // Danh sách các quản trị viên
    admins: Vec<AccountId>,
}

impl ModuleManager {
    pub fn new(
        storage_prefix: Vec<u8>,
        owner_id: AccountId,
        init_admins: Option<Vec<AccountId>>,
    ) -> Self {
        let storage_id = env::storage_usage();
        let mut prefix = storage_prefix.clone();
        
        let mut admins = Vec::new();
        if let Some(admin_list) = init_admins {
            admins.extend(admin_list);
        }
        // Đảm bảo owner luôn là admin
        if !admins.contains(&owner_id) {
            admins.push(owner_id.clone());
        }
        
        let module_prefix = [&prefix[..], b"_modules"].concat();
        let keys_prefix = [&prefix[..], b"_modulekeys"].concat();
        let callers_prefix = [&prefix[..], b"_callers"].concat();
        
        Self {
            modules: UnorderedMap::new(module_prefix),
            module_keys: Vector::new(keys_prefix),
            authorized_callers: LookupMap::new(callers_prefix),
            owner_id,
            admins,
        }
    }
    
    /// Thêm một module mới
    pub fn add_module(&mut self, module: Module) -> bool {
        self.assert_admin();
        
        if self.modules.get(&module.name).is_some() {
            return false; // Module đã tồn tại
        }
        
        self.module_keys.push(&module.name);
        self.modules.insert(&module.name, &module);
        true
    }
    # Thêm module farming
near call <diamond_token_id> add_module '{
  "module": {
    "name": "farming",
    "contract_id": "<farming_contract_id>",
    "version": "1.0.0",
    "status": "Active",
    "added_timestamp": 0,
    "last_updated": 0,
    "metadata": "{\"description\":\"Diamond Farming Module\"}"
  }
}' --accountId <owner_account_id> --depositYocto 1
    /// Cập nhật một module
    pub fn update_module(&mut self, name: &str, updated_module: Module) -> bool {
        self.assert_admin();
        
        if self.modules.get(name).is_none() {
            return false; // Module không tồn tại
        }
        
        self.modules.insert(name, &updated_module);
        true
    }
    
    /// Cập nhật trạng thái module
    pub fn update_module_status(&mut self, name: &str, status: ModuleStatus) -> bool {
        self.assert_admin();
        
        if let Some(mut module) = self.modules.get(name) {
            module.status = status;
            module.last_updated = env::block_timestamp();
            self.modules.insert(name, &module);
            true
        } else {
            false
        }
    }
    
    /// Xóa một module
    pub fn remove_module(&mut self, name: &str) -> bool {
        self.assert_admin();
        
        if self.modules.remove(name).is_none() {
            return false;
        }
        
        // Xóa khỏi danh sách key
        let mut new_keys = Vec::new();
        for i in 0..self.module_keys.len() {
            let key = self.module_keys.get(i).unwrap();
            if key != name {
                new_keys.push(key);
            }
        }
        
        // Tạo lại danh sách key
        self.module_keys.clear();
        for key in new_keys {
            self.module_keys.push(&key);
        }
        
        true
    }
    
    /// Thêm quyền gọi hàm cho một contract
    pub fn authorize_caller(&mut self, contract_id: AccountId, permissions: Vec<String>) -> bool {
        self.assert_admin();
        
        let mut current_permissions = self.authorized_callers
            .get(&contract_id)
            .unwrap_or_else(|| Vec::new());
            
        // Thêm các quyền mới
        for perm in permissions {
            if !current_permissions.contains(&perm) {
                current_permissions.push(perm);
            }
        }
        
        self.authorized_callers.insert(&contract_id, &current_permissions);
        true
    }
    
    /// Thu hồi quyền của một contract
    pub fn revoke_caller(&mut self, contract_id: &AccountId, permissions: Option<Vec<String>>) -> bool {
        self.assert_admin();
        
        if let Some(perms) = permissions {
            // Thu hồi quyền cụ thể
            if let Some(mut current_permissions) = self.authorized_callers.get(contract_id) {
                current_permissions.retain(|p| !perms.contains(p));
                
                if current_permissions.is_empty() {
                    self.authorized_callers.remove(contract_id);
                } else {
                    self.authorized_callers.insert(contract_id, &current_permissions);
                }
                true
            } else {
                false
            }
        } else {
            // Thu hồi tất cả quyền
            self.authorized_callers.remove(contract_id).is_some()
        }
    }
    
    /// Kiểm tra xem contract có quyền thực hiện hành động không
    pub fn has_permission(&self, contract_id: &AccountId, permission: &str) -> bool {
        if self.admins.contains(contract_id) {
            return true;
        }
        
        if let Some(permissions) = self.authorized_callers.get(contract_id) {
            return permissions.contains(&permission.to_string());
        }
        
        false
    }
    
    /// Lấy tất cả module đang hoạt động
    pub fn get_active_modules(&self) -> Vec<Module> {
        let mut result = Vec::new();
        
        for i in 0..self.module_keys.len() {
            let key = self.module_keys.get(i).unwrap();
            let module = self.modules.get(&key).unwrap();
            
            if module.status == ModuleStatus::Active {
                result.push(module);
            }
        }
        
        result
    }
    
    /// Lấy tất cả module
    pub fn get_all_modules(&self) -> Vec<Module> {
        let mut result = Vec::new();
        
        for i in 0..self.module_keys.len() {
            let key = self.module_keys.get(i).unwrap();
            let module = self.modules.get(&key).unwrap();
            result.push(module);
        }
        
        result
    }
    
    /// Gọi một hàm từ module từ xa
    pub fn call_module_function(
        &self,
        module_name: &str,
        function_name: &str,
        args: String,
        deposit: u128,
        gas: u64
    ) -> Promise {
        self.assert_admin();
        
        let module = self.modules.get(module_name)
            .expect("Module not found");
            
        if module.status != ModuleStatus::Active {
            env::panic_str("Module is not active");
        }
        
        Promise::new(module.contract_id)
            .function_call(
                function_name.to_string(),
                args.into_bytes(),
                deposit,
                gas
            )
    }
    
    /// Thêm admin mới
    pub fn add_admin(&mut self, admin_id: AccountId) -> bool {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can add admins"
        );
        
        if !self.admins.contains(&admin_id) {
            self.admins.push(admin_id);
            true
        } else {
            false
        }
    }
    
    /// Xóa admin
    pub fn remove_admin(&mut self, admin_id: &AccountId) -> bool {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can remove admins"
        );
        
        require!(
            admin_id != &self.owner_id,
            "Cannot remove owner from admins"
        );
        
        let initial_len = self.admins.len();
        self.admins.retain(|id| id != admin_id);
        
        initial_len != self.admins.len()
    }
    
    /// Kiểm tra xem người gọi hiện tại có phải là admin
    pub fn is_admin(&self, account_id: &AccountId) -> bool {
        self.admins.contains(account_id)
    }
    
    /// Lấy danh sách tất cả admin
    pub fn get_admins(&self) -> Vec<AccountId> {
        self.admins.clone()
    }
    
    /// Kiểm tra người gọi là admin hoặc owner
    fn assert_admin(&self) {
        let caller = env::predecessor_account_id();
        require!(
            self.is_admin(&caller),
            "Only admin can perform this action"
        );
    }
    
    /// Chuyển quyền sở hữu
    pub fn transfer_ownership(&mut self, new_owner_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can transfer ownership"
        );
        
        // Thêm owner mới vào danh sách admin nếu chưa có
        if !self.admins.contains(&new_owner_id) {
            self.admins.push(new_owner_id.clone());
        }
        
        self.owner_id = new_owner_id;
    }
} 
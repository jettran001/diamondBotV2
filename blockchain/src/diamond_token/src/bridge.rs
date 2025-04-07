use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, AccountId, Balance, PanicOnDefault, PromiseOrValue,
    require, PromiseResult, Promise, Gas,
};
use near_sdk::serde::{Deserialize, Serialize};

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct LayerZeroParams {
    pub dest_chain_id: u16,
    pub adapter_params: Vec<u8>,
    pub fees: U128,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct ChainConfig {
    pub remote_chain_id: u16,
    pub remote_bridge_address: String,
    pub chain_name: String,
    pub enabled: bool,
    pub fee_basis_points: u32,
    pub min_fee: U128,
    pub max_fee: U128,
    pub supply_oracle: Option<AccountId>, // Oracle có khả năng cập nhật supply
}

/// Trait để tích hợp Diamond Token với bridge
#[near_bindgen]
pub trait BridgeAdapter {
    /// Gửi token từ NEAR đến chuỗi khác
    fn bridge_out(&mut self, to: String, amount: U128, lz_params: LayerZeroParams) -> Promise;
    
    /// Nhận token từ chuỗi khác
    fn bridge_in(&mut self, from_chain_id: u16, sender: String, receiver_id: AccountId, amount: U128) -> PromiseOrValue<bool>;
    
    /// Thiết lập cấu hình bridge cho một chuỗi
    fn set_chain_config(&mut self, chain_id: u16, config: ChainConfig);
    
    /// Lấy cấu hình bridge cho một chuỗi
    fn get_chain_config(&self, chain_id: u16) -> Option<ChainConfig>;
    
    /// Tính phí bridge cho một giao dịch
    fn estimate_bridge_fee(&self, dest_chain_id: u16, amount: U128) -> U128;
    
    /// Thêm hàm mới: Lấy tổng cung trên tất cả các chain
    fn get_total_circulating_supply(&self) -> U128;
    
    /// Thêm hàm mới: Lấy số lượng token trên từng chain
    fn get_chain_supplies(&self) -> Vec<(u16, String, U128)>;
    
    /// Thêm hàm mới: Cập nhật thông tin tổng cung từ chain khác
    fn update_remote_chain_supply(&mut self, chain_id: u16, supply: U128) -> bool;
    
    /// Thêm hàm mới: Thiết lập oracle cho một chuỗi
    fn set_supply_oracle(&mut self, chain_id: u16, oracle: AccountId) -> bool;
}

/// Triển khai MPC Controller
/// MPC (Multi-Party Computation) cho phép quản lý bridge an toàn
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct MPCController {
    /// Các địa chỉ được uỷ quyền ký giao dịch
    pub authorized_signers: Vec<AccountId>,
    /// Số chữ ký cần thiết để xác nhận một giao dịch
    pub threshold: u8,
    /// Các giao dịch đang chờ xử lý
    pub pending_transactions: Vec<MPCTransaction>,
    /// Danh sách chain được hỗ trợ
    pub supported_chains: Vec<SupportedChain>,
    /// Owner ID
    pub owner_id: AccountId,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct SupportedChain {
    pub chain_id: u16,
    pub chain_name: String,
    pub bridge_address: String,
    pub enabled: bool,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct MPCTransaction {
    pub tx_id: String,
    pub from_chain: u16,
    pub to_chain: u16,
    pub sender: String,
    pub receiver: String,
    pub amount: U128,
    pub signatures: Vec<(AccountId, Vec<u8>)>,
    pub completed: bool,
    pub timestamp: u64,
    pub memo: Option<String>,
}

#[near_bindgen]
impl MPCController {
    #[init]
    pub fn new(authorized_signers: Vec<AccountId>, threshold: u8, owner_id: AccountId) -> Self {
        require!(
            threshold > 0 && threshold as usize <= authorized_signers.len(),
            "Invalid threshold"
        );
        
        Self {
            authorized_signers,
            threshold,
            pending_transactions: Vec::new(),
            supported_chains: Vec::new(),
            owner_id,
        }
    }
    
    /// Thêm chain mới
    pub fn add_supported_chain(&mut self, chain: SupportedChain) -> bool {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can add supported chains"
        );
        
        // Kiểm tra chain đã tồn tại chưa
        if self.supported_chains.iter().any(|c| c.chain_id == chain.chain_id) {
            return false;
        }
        
        self.supported_chains.push(chain);
        true
    }
    
    /// Cập nhật chain
    pub fn update_supported_chain(&mut self, chain_id: u16, chain: SupportedChain) -> bool {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can update supported chains"
        );
        
        for i in 0..self.supported_chains.len() {
            if self.supported_chains[i].chain_id == chain_id {
                self.supported_chains[i] = chain;
                return true;
            }
        }
        
        false
    }
    
    /// Kiểm tra chain hỗ trợ
    pub fn is_chain_supported(&self, chain_id: u16) -> bool {
        self.supported_chains.iter().any(|c| c.chain_id == chain_id && c.enabled)
    }
    
    /// Lấy danh sách chain được hỗ trợ
    pub fn get_supported_chains(&self) -> Vec<SupportedChain> {
        self.supported_chains.clone()
    }
    
    /// Tạo một giao dịch bridge mới
    pub fn create_transaction(
        &mut self,
        tx_id: String,
        from_chain: u16,
        to_chain: u16,
        sender: String,
        receiver: String,
        amount: U128,
        memo: Option<String>,
    ) -> PromiseOrValue<bool> {
        // Kiểm tra xem giao dịch đã tồn tại chưa
        require!(
            !self.pending_transactions.iter().any(|tx| tx.tx_id == tx_id),
            "Transaction already exists"
        );
        
        // Kiểm tra chain được hỗ trợ
        require!(
            self.is_chain_supported(from_chain) && self.is_chain_supported(to_chain),
            "Chain not supported"
        );
        
        // Tạo giao dịch mới
        let transaction = MPCTransaction {
            tx_id,
            from_chain,
            to_chain,
            sender,
            receiver,
            amount,
            signatures: Vec::new(),
            completed: false,
            timestamp: env::block_timestamp(),
            memo,
        };
        
        self.pending_transactions.push(transaction);
        PromiseOrValue::Value(true)
    }
    
    /// Ký một giao dịch
    pub fn sign_transaction(&mut self, tx_id: String, signature: Vec<u8>) -> PromiseOrValue<bool> {
        let signer_id = env::predecessor_account_id();
        
        // Kiểm tra xem người ký có được uỷ quyền không
        require!(
            self.authorized_signers.contains(&signer_id),
            "Signer not authorized"
        );
        
        // Tìm giao dịch
        for tx in &mut self.pending_transactions {
            if tx.tx_id == tx_id && !tx.completed {
                // Kiểm tra người ký chưa ký giao dịch này
                if !tx.signatures.iter().any(|(id, _)| id == &signer_id) {
                    tx.signatures.push((signer_id, signature));
                    
                    // Kiểm tra xem đã đủ chữ ký chưa
                    if tx.signatures.len() >= self.threshold as usize {
                        tx.completed = true;
                        // Thực hiện bridge (trong triển khai thực tế sẽ gọi smart contract bridge)
                        return PromiseOrValue::Value(true);
                    }
                    
                    return PromiseOrValue::Value(true);
                } else {
                    return PromiseOrValue::Value(false);
                }
            }
        }
        
        PromiseOrValue::Value(false)
    }
    
    /// Lấy thông tin giao dịch
    pub fn get_transaction(&self, tx_id: String) -> Option<MPCTransaction> {
        self.pending_transactions
            .iter()
            .find(|tx| tx.tx_id == tx_id)
            .cloned()
    }
    
    /// Lấy tất cả giao dịch đang chờ
    pub fn get_pending_transactions(&self) -> Vec<MPCTransaction> {
        self.pending_transactions
            .iter()
            .filter(|tx| !tx.completed)
            .cloned()
            .collect()
    }
    
    /// Lấy tất cả giao dịch hoàn thành
    pub fn get_completed_transactions(&self, limit: u32) -> Vec<MPCTransaction> {
        self.pending_transactions
            .iter()
            .filter(|tx| tx.completed)
            .rev()
            .take(limit as usize)
            .cloned()
            .collect()
    }
    
    /// Thêm người ký được uỷ quyền
    pub fn add_authorized_signer(&mut self, signer_id: AccountId) -> bool {
        // Chỉ owner mới có thể thêm người ký
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can add signers"
        );
        
        if !self.authorized_signers.contains(&signer_id) {
            self.authorized_signers.push(signer_id);
            return true;
        }
        
        false
    }
    
    /// Xóa người ký
    pub fn remove_authorized_signer(&mut self, signer_id: &AccountId) -> bool {
        // Chỉ owner mới có thể xóa người ký
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can remove signers"
        );
        
        let initial_len = self.authorized_signers.len();
        self.authorized_signers.retain(|id| id != signer_id);
        
        // Đảm bảo số lượng người ký còn lại >= threshold
        require!(
            self.authorized_signers.len() >= self.threshold as usize,
            "Cannot remove signer: would make threshold unachievable"
        );
        
        initial_len != self.authorized_signers.len()
    }
    
    /// Thay đổi threshold
    pub fn set_threshold(&mut self, threshold: u8) -> bool {
        // Chỉ owner mới có thể thay đổi threshold
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can change threshold"
        );
        
        require!(
            threshold > 0 && threshold as usize <= self.authorized_signers.len(),
            "Invalid threshold"
        );
        
        self.threshold = threshold;
        true
    }
    
    /// Lấy danh sách người ký được ủy quyền
    pub fn get_authorized_signers(&self) -> Vec<AccountId> {
        self.authorized_signers.clone()
    }
    
    /// Lấy threshold hiện tại
    pub fn get_threshold(&self) -> u8 {
        self.threshold
    }
    
    /// Chuyển quyền sở hữu
    pub fn transfer_ownership(&mut self, new_owner_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can transfer ownership"
        );
        
        self.owner_id = new_owner_id;
    }
}

// Khai báo layerzero_lz_receiver từ xa
#[derive(BorshDeserialize, BorshSerialize)]
pub struct ExternalLayerZero {
    pub account_id: AccountId,
}

/// Gọi hàm LayerZero để gửi message cross-chain
pub fn lz_send(
    lz_endpoint: AccountId,
    dest_chain_id: u16,
    dest_address: Vec<u8>,
    payload: Vec<u8>,
    refund_address: AccountId,
    adapter_params: Vec<u8>,
    fees: Balance,
) -> Promise {
    // Trong triển khai thực tế, đây sẽ là lời gọi đến LayerZero Endpoint contract
    Promise::new(lz_endpoint)
        .function_call(
            "lz_send".to_string(),
            near_sdk::serde_json::to_vec(&serde_json::json!({
                "dest_chain_id": dest_chain_id,
                "dest_address": dest_address,
                "payload": payload,
                "refund_address": refund_address,
                "adapter_params": adapter_params
            }))
            .unwrap(),
            fees as u128,
            Gas(5_000_000_000_000)
        )
} 
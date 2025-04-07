use ethers::prelude::*;
use ethers::types::{Address, U256, H256};
use std::sync::Arc;
use std::collections::HashMap;
use std::str::FromStr;
use anyhow::Result;
use crate::wallet::WalletManager;

type PoolId = u64;

#[derive(Debug, Clone)]
pub struct FarmData {
    pub pool_id: PoolId,
    pub stake_token: Address,
    pub reward_token: Address,
    pub apr: f64,
    pub tvl: U256,
    pub total_staked: U256,
    pub rewards_per_block: U256,
    pub last_updated: u64,
    pub end_block: Option<u64>,
}

pub struct FarmManager {
    contract_address: Address,
    provider: Arc<Provider<Http>>,
    farm_data: HashMap<PoolId, FarmData>,
}

impl FarmManager {
    pub fn new(address: Address, provider: Arc<Provider<Http>>) -> Self {
        Self {
            contract_address: address,
            provider,
            farm_data: HashMap::new(),
        }
    }
    
    pub async fn stake(&self, wallet: &WalletManager, 
                     pool_id: u64, 
                     amount: U256) -> Result<H256> {
        // Stake token vào pool
        // Lấy địa chỉ wallet - cần thay thế hàm get_default_wallet bằng cách khác
        // Giả sử wallet đầu tiên trong danh sách là wallet mặc định
        let wallets = wallet.list_wallets()?;
        if wallets.is_empty() {
            return Err(anyhow::anyhow!("Không tìm thấy ví nào"));
        }
        
        let wallet_info = &wallets[0];
        
        // Truy cập ví từ WalletManager - cần sử dụng phương thức get_wallet
        let wallet_addr = Address::from_str(&wallet_info.address)?;
        let wallet_signer = wallet.get_wallet(wallet_addr)?
            .with_chain_id(wallet_info.chain_id);
        
        // Tạo provider với signer
        let provider_with_signer = SignerMiddleware::new(
            self.provider.clone(),
            wallet_signer
        );
        
        // Tạo contract interface
        let farm_contract = ethers::contract::Contract::new(
            self.contract_address,
            // ABI của farm contract (đây chỉ là ví dụ)
            ethers::abi::parse_abi(&[
                "function deposit(uint256 _pid, uint256 _amount) external"
            ])?,
            Arc::new(provider_with_signer)
        );
        
        // Gọi hàm deposit để stake token
        let tx = farm_contract
            .method::<_, ()>("deposit", (pool_id, amount))?
            .send()
            .await?
            .await?;
        
        // Trả về hash của transaction
        match tx {
            Some(receipt) => Ok(receipt.transaction_hash),
            None => Err(anyhow::anyhow!("Không nhận được transaction receipt"))
        }
    }
    
    pub async fn unstake(&self, wallet: &WalletManager, 
                       pool_id: u64, 
                       amount: U256) -> Result<H256> {
        // Unstake token từ pool
        // Lấy địa chỉ wallet - cần thay thế hàm get_default_wallet bằng cách khác
        // Giả sử wallet đầu tiên trong danh sách là wallet mặc định
        let wallets = wallet.list_wallets()?;
        if wallets.is_empty() {
            return Err(anyhow::anyhow!("Không tìm thấy ví nào"));
        }
        
        let wallet_info = &wallets[0];
        
        // Truy cập ví từ WalletManager - cần sử dụng phương thức get_wallet
        let wallet_addr = Address::from_str(&wallet_info.address)?;
        let wallet_signer = wallet.get_wallet(wallet_addr)?
            .with_chain_id(wallet_info.chain_id);
        
        // Tạo provider với signer
        let provider_with_signer = SignerMiddleware::new(
            self.provider.clone(),
            wallet_signer
        );
        
        // Tạo contract interface
        let farm_contract = ethers::contract::Contract::new(
            self.contract_address,
            // ABI của farm contract (đây chỉ là ví dụ)
            ethers::abi::parse_abi(&[
                "function withdraw(uint256 _pid, uint256 _amount) external"
            ])?,
            Arc::new(provider_with_signer)
        );
        
        // Gọi hàm withdraw để unstake token
        let tx = farm_contract
            .method::<_, ()>("withdraw", (pool_id, amount))?
            .send()
            .await?
            .await?;
        
        // Trả về hash của transaction
        match tx {
            Some(receipt) => Ok(receipt.transaction_hash),
            None => Err(anyhow::anyhow!("Không nhận được transaction receipt"))
        }
    }
    
    pub async fn claim_rewards(&self, wallet: &WalletManager, 
                            pool_id: u64) -> Result<H256> {
        // Nhận phần thưởng - hầu hết các farm, unstake với amount=0 sẽ nhận phần thưởng
        self.unstake(wallet, pool_id, U256::zero()).await
    }
    
    pub async fn get_apr(&self, pool_id: u64) -> Result<f64> {
        // Lấy APR của pool
        if let Some(farm) = self.farm_data.get(&pool_id) {
            Ok(farm.apr)
        } else {
            // Fetch data from contract
            let farm_data = self.update_pool_data_internal(pool_id).await?;
            Ok(farm_data.apr)
        }
    }
    
    pub async fn get_tvl(&self, pool_id: u64) -> Result<U256> {
        // Lấy tổng giá trị bị khóa (TVL)
        if let Some(farm) = self.farm_data.get(&pool_id) {
            Ok(farm.tvl)
        } else {
            // Fetch data from contract
            let farm_data = self.update_pool_data_internal(pool_id).await?;
            Ok(farm_data.tvl)
        }
    }
    
    pub async fn update_pool_data(&mut self, pool_id: u64) -> Result<FarmData> {
        // Cập nhật dữ liệu pool từ contract
        let farm_data = self.update_pool_data_internal(pool_id).await?;
        
        // Cập nhật cache
        self.farm_data.insert(pool_id, farm_data.clone());
        
        Ok(farm_data)
    }
    
    // Phương thức internal để fetch dữ liệu từ contract
    async fn update_pool_data_internal(&self, pool_id: u64) -> Result<FarmData> {
        // Tạo contract interface
        let farm_contract = ethers::contract::Contract::new(
            self.contract_address,
            // ABI của farm contract (đây chỉ là ví dụ)
            ethers::abi::parse_abi(&[
                "function poolInfo(uint256 _pid) external view returns (address lpToken, uint256 allocPoint, uint256 lastRewardBlock, uint256 accRewardPerShare)",
                "function rewardToken() external view returns (address)",
                "function totalStaked(uint256 _pid) external view returns (uint256)"
            ])?,
            self.provider.clone()
        );
        
        // Gọi hàm poolInfo để lấy thông tin pool
        let (lp_token, alloc_point, _last_reward_block, _): (Address, U256, U256, U256) = farm_contract
            .method("poolInfo", pool_id)?
            .call()
            .await?;
        
        // Lấy token thưởng
        let reward_token: Address = farm_contract
            .method::<_, Address>("rewardToken", ())?
            .call()
            .await?;
        
        // Lấy tổng số token đã stake
        let total_staked: U256 = farm_contract
            .method::<_, U256>("totalStaked", pool_id)?
            .call()
            .await?;
        
        // Lấy block hiện tại
        let current_block = self.provider.get_block_number().await?;
        
        // Tính toán APR dựa trên phân bổ điểm và block reward
        // Đây chỉ là ví dụ đơn giản, trong thực tế cần tính toán dựa trên giá token
        let apr = calculate_apr(alloc_point, total_staked);
        
        // Ví dụ về cách tính TVL, cần biết giá của token stake
        let tvl = total_staked; // Trong thực tế: total_staked * token_price
        
        // Tạo dữ liệu farm
        let farm_data = FarmData {
            pool_id,
            stake_token: lp_token,
            reward_token,
            apr,
            tvl,
            total_staked,
            rewards_per_block: U256::from(1_000_000_000_000_000_000u128), // Giả định
            last_updated: current_block.as_u64(),
            end_block: None // Một số farm không có ngày kết thúc
        };
        
        Ok(farm_data)
    }
    
    pub async fn get_user_stake(&self, wallet: &WalletManager, pool_id: u64) -> Result<U256> {
        // Lấy số lượng token đã stake của user
        // Lấy địa chỉ wallet - cần thay thế hàm get_default_wallet bằng cách khác
        // Giả sử wallet đầu tiên trong danh sách là wallet mặc định
        let wallets = wallet.list_wallets()?;
        if wallets.is_empty() {
            return Err(anyhow::anyhow!("Không tìm thấy ví nào"));
        }
        
        let wallet_info = &wallets[0];
        let user_address = Address::from_str(&wallet_info.address)?;
        
        // Tạo contract interface
        let farm_contract = ethers::contract::Contract::new(
            self.contract_address,
            // ABI của farm contract (đây chỉ là ví dụ)
            ethers::abi::parse_abi(&[
                "function userInfo(uint256 _pid, address _user) external view returns (uint256 amount, uint256 rewardDebt)"
            ])?,
            self.provider.clone()
        );
        
        // Gọi hàm userInfo để lấy thông tin stake của user
        let (amount, _): (U256, U256) = farm_contract
            .method("userInfo", (pool_id, user_address))?
            .call()
            .await?;
        
        Ok(amount)
    }
    
    pub async fn get_pending_rewards(&self, wallet: &WalletManager, pool_id: u64) -> Result<U256> {
        // Lấy số lượng phần thưởng đang chờ nhận
        // Lấy địa chỉ wallet - cần thay thế hàm get_default_wallet bằng cách khác
        // Giả sử wallet đầu tiên trong danh sách là wallet mặc định
        let wallets = wallet.list_wallets()?;
        if wallets.is_empty() {
            return Err(anyhow::anyhow!("Không tìm thấy ví nào"));
        }
        
        let wallet_info = &wallets[0];
        let user_address = Address::from_str(&wallet_info.address)?;
        
        // Tạo contract interface
        let farm_contract = ethers::contract::Contract::new(
            self.contract_address,
            // ABI của farm contract (đây chỉ là ví dụ)
            ethers::abi::parse_abi(&[
                "function pendingReward(uint256 _pid, address _user) external view returns (uint256)"
            ])?,
            self.provider.clone()
        );
        
        // Gọi hàm pendingReward để lấy phần thưởng chờ nhận
        let pending: U256 = farm_contract
            .method("pendingReward", (pool_id, user_address))?
            .call()
            .await?;
        
        Ok(pending)
    }
}

// Hàm phụ trợ để tính APR dựa trên phân bổ điểm và tổng số token đã stake
fn calculate_apr(alloc_point: U256, total_staked: U256) -> f64 {
    // Đây chỉ là ví dụ đơn giản, trong thực tế cần tính toán chi tiết hơn
    if total_staked.is_zero() {
        return 0.0;
    }
    
    // Giả định rằng mỗi block có 1 token reward, và có 2_102_400 block mỗi năm (6500 mỗi ngày)
    const BLOCKS_PER_YEAR: u64 = 2_102_400;
    
    // Giả định giá token reward và stake là 1:1
    let rewards_per_year = alloc_point.as_u128() as f64 * BLOCKS_PER_YEAR as f64;
    (rewards_per_year / total_staked.as_u128() as f64) * 100.0
}

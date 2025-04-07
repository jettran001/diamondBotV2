use ethers::types::{Address, U256};
use anyhow::{Result, anyhow};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use tracing::{info, warn, error};
use tokio::sync::{RwLock as TokioRwLock, Mutex};
use ethers::providers::{Provider, Http, Middleware};
use log::{debug, info, warn, error};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::cmp;
use tracing::{info, warn};
use crate::chain_adapters::interfaces::ChainError;

/// Quản lý nonce để tránh duplicate transaction
pub struct NonceManager {
    /// Lưu trữ nonce cho mỗi địa chỉ ví
    nonces: RwLock<HashMap<Address, Mutex<U256>>>,
    /// Provider để lấy nonce từ blockchain khi cần
    provider: Arc<Provider<Http>>,
    /// Thời gian cache nonce tối đa (sau thời gian này sẽ lấy lại từ chain)
    cache_duration: Duration,
    /// Lưu thời điểm cập nhật nonce gần nhất
    last_updates: RwLock<HashMap<Address, Instant>>,
    /// Lock để đảm bảo đồng bộ giữa các thread khi cập nhật nonce
    global_lock: Mutex<()>,
}

impl NonceManager {
    /// Tạo NonceManager mới
    pub fn new(provider: Arc<Provider<Http>>, cache_seconds: u64) -> Self {
        Self {
            nonces: RwLock::new(HashMap::new()),
            provider,
            cache_duration: Duration::from_secs(cache_seconds),
            last_updates: RwLock::new(HashMap::new()),
            global_lock: Mutex::new(()),
        }
    }
    
    /// Lấy nonce tiếp theo cho địa chỉ và tự động tăng
    pub async fn get_next_nonce(&self, address: Address) -> Result<U256> {
        // Lock global để tránh race condition giữa nhiều thread
        let _guard = self.global_lock.lock().await;
        
        // Kiểm tra xem đã có nonce cho địa chỉ này chưa
        let mut nonces = self.nonces.write().await;
        let mut last_updates = self.last_updates.write().await;
        
        // Nếu chưa có nonce hoặc cache đã hết hạn, lấy từ blockchain
        let should_refresh = match last_updates.get(&address) {
            Some(time) => time.elapsed() > self.cache_duration,
            None => true,
        };
        
        if !nonces.contains_key(&address) || should_refresh {
            // Lấy nonce hiện tại từ blockchain
            let chain_nonce = self.get_nonce_from_chain(address).await?;
            debug!("Lấy nonce từ blockchain cho địa chỉ {}: {}", address, chain_nonce);
            
            // Thêm nonce vào cache
            nonces.insert(address, Mutex::new(chain_nonce));
            last_updates.insert(address, Instant::now());
        }
        
        // Lấy mutex cho nonce của địa chỉ
        let nonce_mutex = nonces.get(&address).unwrap().clone();
        
        // Giải phóng lock write để các thread khác có thể đọc
        drop(nonces);
        drop(last_updates);
        
        // Lock nonce của địa chỉ này và tăng giá trị
        let mut nonce = nonce_mutex.lock().await;
        let current_nonce = *nonce;
        *nonce = *nonce + 1;
        
        Ok(current_nonce)
    }
    
    /// Cập nhật nonce cụ thể cho địa chỉ (sau khi biết giao dịch đã được xác nhận)
    pub async fn update_nonce(&self, address: Address, new_nonce: U256) -> Result<()> {
        let _guard = self.global_lock.lock().await;
        
        let mut nonces = self.nonces.write().await;
        let mut last_updates = self.last_updates.write().await;
        
        // Tạo mới hoặc cập nhật nonce hiện có
        if let Some(nonce_mutex) = nonces.get(&address) {
            let mut nonce = nonce_mutex.lock().await;
            *nonce = cmp::max(*nonce, new_nonce);
        } else {
            nonces.insert(address, Mutex::new(new_nonce));
        }
        
        // Cập nhật thời gian cập nhật
        last_updates.insert(address, Instant::now());
        
        Ok(())
    }
    
    /// Reset nonce cho địa chỉ (lấy lại từ blockchain)
    pub async fn reset_nonce(&self, address: Address) -> Result<U256> {
        let _guard = self.global_lock.lock().await;
        
        // Lấy nonce từ blockchain
        let chain_nonce = self.get_nonce_from_chain(address).await?;
        
        // Cập nhật lại trong cache
        let mut nonces = self.nonces.write().await;
        let mut last_updates = self.last_updates.write().await;
        
        if let Some(nonce_mutex) = nonces.get(&address) {
            let mut nonce = nonce_mutex.lock().await;
            *nonce = chain_nonce;
        } else {
            nonces.insert(address, Mutex::new(chain_nonce));
        }
        
        last_updates.insert(address, Instant::now());
        
        Ok(chain_nonce)
    }
    
    /// Lấy nonce từ blockchain
    async fn get_nonce_from_chain(&self, address: Address) -> Result<U256> {
        match self.provider.get_transaction_count(address, None).await {
            Ok(nonce) => Ok(nonce),
            Err(err) => {
                error!("Lỗi khi lấy nonce từ blockchain: {}", err);
                Err(anyhow!("Không thể lấy nonce: {}", err))
            }
        }
    }
    
    /// Kiểm tra xem nonce đã tồn tại trong cache chưa
    pub async fn has_cached_nonce(&self, address: Address) -> bool {
        let nonces = self.nonces.read().await;
        nonces.contains_key(&address)
    }
    
    /// Xóa cache nonce cho tất cả địa chỉ đã quá hạn
    pub async fn cleanup_cache(&self) {
        let _guard = self.global_lock.lock().await;
        
        let mut nonces = self.nonces.write().await;
        let mut last_updates = self.last_updates.write().await;
        
        // Lọc ra các địa chỉ có cache đã hết hạn
        let expired_addresses: Vec<Address> = last_updates
            .iter()
            .filter(|(_, time)| time.elapsed() > self.cache_duration)
            .map(|(addr, _)| *addr)
            .collect();
        
        // Xóa các địa chỉ hết hạn khỏi cache
        for addr in &expired_addresses {
            nonces.remove(addr);
            last_updates.remove(addr);
        }
        
        if !expired_addresses.is_empty() {
            debug!("Đã xóa {} cache nonce hết hạn", expired_addresses.len());
        }
    }
} 
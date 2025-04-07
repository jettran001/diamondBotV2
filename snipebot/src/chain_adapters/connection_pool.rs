// Standard library imports
use std::{
    sync::{Arc, RwLock},
    collections::HashMap,
    time::Duration,
    future::Future,
    sync::Weak,
};

// Internal imports
use crate::chain_adapters::{
    interfaces::ChainError,
    retry_policy::{RetryPolicy, RetryPolicyEnum},
};

// Third party imports
use anyhow::{Result, anyhow};
use tracing::{info, warn, debug};
use ethers::providers::{Middleware, Provider};
use serde::{Serialize, Deserialize};
use metrics::{counter, gauge};
use tokio::sync::Semaphore;
use serde_json;
use crate::cache::{Cache, JSONCache, Cacheable};

/// Trạng thái của một RPC endpoint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointStatus {
    /// Đang hoạt động tốt
    Healthy,
    /// Có vấn đề nhỏ (độ trễ cao, đôi khi timeout)
    Degraded,
    /// Không thể kết nối
    Down,
}

/// Thông tin chi tiết về một RPC endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    /// URL của endpoint
    pub url: String,
    /// Trạng thái hiện tại
    pub status: EndpointStatus,
    /// Độ trễ trung bình (ms)
    pub avg_latency: f64,
    /// Thời gian gần nhất kiểm tra (Unix timestamp)
    pub last_check: u64,
    /// Tỷ lệ lỗi (0.0 - 1.0)
    pub error_rate: f64,
    /// Số lần sử dụng thành công
    pub success_count: u64,
    /// Số lần lỗi
    pub error_count: u64,
    /// Thứ tự ưu tiên (thấp hơn = ưu tiên cao hơn)
    pub priority: u32,
    /// Giới hạn số lượng request đồng thời
    pub concurrent_limit: usize,
}

impl EndpointInfo {
    /// Tạo mới thông tin endpoint
    pub fn new(url: &str, priority: u32, concurrent_limit: usize) -> Self {
        Self {
            url: url.to_string(),
            status: EndpointStatus::Healthy,
            avg_latency: 0.0,
            last_check: 0,
            error_rate: 0.0,
            success_count: 0,
            error_count: 0,
            priority,
            concurrent_limit,
        }
    }
    
    /// Cập nhật thông tin sau một request thành công
    pub fn record_success(&mut self, latency_ms: f64) {
        self.success_count += 1;
        
        // Cập nhật độ trễ trung bình
        let total_requests = self.success_count + self.error_count;
        self.avg_latency = ((self.avg_latency * (total_requests - 1) as f64) + latency_ms) / total_requests as f64;
        
        // Cập nhật tỷ lệ lỗi
        self.error_rate = self.error_count as f64 / total_requests as f64;
        
        // Cập nhật thời gian kiểm tra
        self.last_check = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Cập nhật trạng thái nếu cần
        if self.status == EndpointStatus::Degraded && self.error_rate < 0.1 {
            self.status = EndpointStatus::Healthy;
            info!("Endpoint {} status changed to Healthy", self.url);
        }
    }
    
    /// Cập nhật thông tin sau một request lỗi
    pub fn record_error(&mut self) {
        self.error_count += 1;
        
        // Cập nhật tỷ lệ lỗi
        let total_requests = self.success_count + self.error_count;
        self.error_rate = self.error_count as f64 / total_requests as f64;
        
        // Cập nhật thời gian kiểm tra
        self.last_check = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Cập nhật trạng thái
        if self.error_rate > 0.5 {
            self.status = EndpointStatus::Down;
            warn!("Endpoint {} status changed to Down (error rate: {:.2})", self.url, self.error_rate);
        } else if self.error_rate > 0.1 {
            self.status = EndpointStatus::Degraded;
            warn!("Endpoint {} status changed to Degraded (error rate: {:.2})", self.url, self.error_rate);
        }
    }
    
    /// Kiểm tra endpoint có khả dụng không
    pub fn is_available(&self) -> bool {
        self.status != EndpointStatus::Down
    }
}

/// Cấu trúc chi tiết endpoint với thông tin provider
struct PooledEndpoint {
    /// Thông tin endpoint
    info: EndpointInfo,
    /// Provider đã được khởi tạo
    provider: Provider<Http>,
    /// Semaphore để giới hạn số lượng request đồng thời
    semaphore: Arc<Semaphore>,
    /// Thời gian cuối cùng sử dụng
    last_used: Instant,
}

/// Cấu hình cho RPC pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Thời gian tối đa một kết nối không hoạt động (seconds)
    pub idle_timeout: u64,
    /// Khoảng thời gian kiểm tra sức khỏe (seconds)
    pub health_check_interval: u64,
    /// Số lần thử kết nối lại
    pub reconnect_attempts: usize,
    /// Số lượng kết nối tối đa trong pool
    pub max_connections: usize,
    /// Thời gian chờ tối đa để lấy kết nối (ms)
    pub connection_timeout: u64,
    /// Thời gian chờ tối đa cho một request (ms)
    pub request_timeout: u64,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            idle_timeout: 300,         // 5 phút
            health_check_interval: 60,  // 1 phút
            reconnect_attempts: 3,
            max_connections: 10,
            connection_timeout: 5000,   // 5 giây
            request_timeout: 30000,     // 30 giây
        }
    }
}

/// Quản lý pool kết nối đến các RPC endpoint
pub struct RPCConnectionPool {
    /// Các endpoint chính
    primary_endpoints: Vec<PooledEndpoint>,
    /// Các endpoint backup
    backup_endpoints: Vec<PooledEndpoint>,
    /// Thứ tự endpoint tiếp theo được sử dụng (round-robin)
    next_primary_index: usize,
    /// Thứ tự endpoint backup tiếp theo
    next_backup_index: usize,
    /// Policy retry
    retry_policy: RetryPolicyEnum,
    /// Cấu hình
    config: ConnectionPoolConfig,
    /// Chain ID
    chain_id: u64,
    /// Cache cho các kết quả truy vấn
    cache: JSONCache,
}

impl Cacheable for RPCConnectionPool {
    type Value = serde_json::Value;

    fn get_from_cache(&self, key: &str) -> Option<Self::Value> {
        self.cache.get(key)
    }

    fn store_in_cache(&self, key: &str, value: &Self::Value, ttl_seconds: u64) -> Result<()> {
        self.cache.set(key, value.clone(), ttl_seconds)
    }

    fn cleanup_cache(&self) {
        self.cache.cleanup()
    }
}

impl RPCConnectionPool {
    /// Tạo mới RPC pool
    pub async fn new(
        chain_id: u64,
        primary_urls: Vec<String>,
        backup_urls: Vec<String>,
        config: ConnectionPoolConfig,
        retry_policy: Option<RetryPolicyEnum>,
    ) -> Result<Self> {
        let retry_policy = retry_policy.unwrap_or_else(create_default_retry_policy);
        
        // Khởi tạo endpoint chính
        let mut primary_endpoints = Vec::with_capacity(primary_urls.len());
        for (i, url) in primary_urls.iter().enumerate() {
            match Self::create_endpoint(url, i as u32, config.max_connections).await {
                Ok(endpoint) => primary_endpoints.push(endpoint),
                Err(e) => {
                    warn!("Failed to initialize primary RPC endpoint {}: {}", url, e);
                    // Tiếp tục với endpoint tiếp theo
                }
            }
        }
        
        if primary_endpoints.is_empty() {
            return Err(anyhow!("No valid primary RPC endpoints available"));
        }
        
        // Khởi tạo endpoint backup
        let mut backup_endpoints = Vec::with_capacity(backup_urls.len());
        for (i, url) in backup_urls.iter().enumerate() {
            match Self::create_endpoint(url, (i + 100) as u32, config.max_connections).await {
                Ok(endpoint) => backup_endpoints.push(endpoint),
                Err(e) => {
                    warn!("Failed to initialize backup RPC endpoint {}: {}", url, e);
                    // Tiếp tục với endpoint tiếp theo
                }
            }
        }
        
        info!("Initialized RPC connection pool with {} primary and {} backup endpoints", 
              primary_endpoints.len(), backup_endpoints.len());
        
        Ok(Self {
            primary_endpoints,
            backup_endpoints,
            next_primary_index: 0,
            next_backup_index: 0,
            retry_policy,
            config,
            chain_id,
            cache: JSONCache::new(),
        })
    }
    
    /// Tạo PooledEndpoint từ URL
    async fn create_endpoint(url: &str, priority: u32, max_concurrent: usize) -> Result<PooledEndpoint> {
        // Tạo provider
        let provider = Provider::<Http>::try_from(url)
            .with_context(|| format!("Failed to create provider for URL: {}", url))?;
        
        // Kiểm tra provider có hoạt động không
        match tokio::time::timeout(
            Duration::from_secs(5),
            provider.get_block_number()
        ).await {
            Ok(Ok(_)) => {
                // Provider hoạt động, tạo endpoint
                Ok(PooledEndpoint {
                    info: EndpointInfo::new(url, priority, max_concurrent),
                    provider,
                    semaphore: Arc::new(Semaphore::new(max_concurrent)),
                    last_used: Instant::now(),
                })
            },
            _ => {
                // Provider không hoạt động
                Err(anyhow!("Provider not responding for URL: {}", url))
            }
        }
    }
    
    /// Lấy provider theo round-robin
    pub async fn get_provider(&mut self) -> Result<ProviderGuard> {
        // Thử endpoints chính trước
        if !self.primary_endpoints.is_empty() {
            // Tìm endpoint chính khả dụng
            for _ in 0..self.primary_endpoints.len() {
                let index = self.next_primary_index;
                self.next_primary_index = (self.next_primary_index + 1) % self.primary_endpoints.len();
                
                let endpoint = &self.primary_endpoints[index];
                if endpoint.info.is_available() {
                    // Cố gắng lấy permit từ semaphore
                    match tokio::time::timeout(
                        Duration::from_millis(self.config.connection_timeout),
                        endpoint.semaphore.clone().acquire_owned()
                    ).await {
                        Ok(Ok(permit)) => {
                            // Đã có permit, trả về provider
                            let endpoint = &mut self.primary_endpoints[index];
                            endpoint.last_used = Instant::now();
                            
                            // Khi tạo ProviderGuard, sử dụng Arc::downgrade để cung cấp Weak reference
                            let pool_arc = Arc::new(RwLock::new(self));
                            let pool_weak = Arc::downgrade(&pool_arc);
                            
                            return Ok(ProviderGuard {
                                provider: endpoint.provider.clone(),
                                endpoint_info: endpoint.info.clone(),
                                _permit: permit,
                                pool: pool_weak,
                            });
                        },
                        _ => {
                            // Không thể lấy permit, thử endpoint tiếp theo
                            continue;
                        }
                    }
                }
            }
        }
        
        // Nếu không có endpoint chính khả dụng, thử endpoints backup
        if !self.backup_endpoints.is_empty() {
            for _ in 0..self.backup_endpoints.len() {
                let index = self.next_backup_index;
                self.next_backup_index = (self.next_backup_index + 1) % self.backup_endpoints.len();
                
                let endpoint = &self.backup_endpoints[index];
                if endpoint.info.is_available() {
                    // Cố gắng lấy permit từ semaphore
                    match tokio::time::timeout(
                        Duration::from_millis(self.config.connection_timeout),
                        endpoint.semaphore.clone().acquire_owned()
                    ).await {
                        Ok(Ok(permit)) => {
                            // Đã có permit, trả về provider
                            let endpoint = &mut self.backup_endpoints[index];
                            endpoint.last_used = Instant::now();
                            
                            // Khi tạo ProviderGuard, sử dụng Arc::downgrade để cung cấp Weak reference
                            let pool_arc = Arc::new(RwLock::new(self));
                            let pool_weak = Arc::downgrade(&pool_arc);
                            
                            return Ok(ProviderGuard {
                                provider: endpoint.provider.clone(),
                                endpoint_info: endpoint.info.clone(),
                                _permit: permit,
                                pool: pool_weak,
                            });
                        },
                        _ => {
                            // Không thể lấy permit, thử endpoint tiếp theo
                            continue;
                        }
                    }
                }
            }
        }
        
        // Không có endpoint nào khả dụng
        Err(anyhow!("No available RPC endpoints"))
    }
    
    /// Cập nhật trạng thái endpoint sau một request thành công
    fn record_success(&mut self, url: &str, latency_ms: f64) {
        // Cập nhật endpoint chính
        for endpoint in &mut self.primary_endpoints {
            if endpoint.info.url == url {
                endpoint.info.record_success(latency_ms);
                
                // Cập nhật metrics
                gauge!("rpc_endpoint_latency", latency_ms, "url" => url.to_string());
                gauge!("rpc_endpoint_error_rate", endpoint.info.error_rate, "url" => url.to_string());
                
                return;
            }
        }
        
        // Cập nhật endpoint backup
        for endpoint in &mut self.backup_endpoints {
            if endpoint.info.url == url {
                endpoint.info.record_success(latency_ms);
                
                // Cập nhật metrics
                gauge!("rpc_endpoint_latency", latency_ms, "url" => url.to_string());
                gauge!("rpc_endpoint_error_rate", endpoint.info.error_rate, "url" => url.to_string());
                
                return;
            }
        }
    }
    
    /// Cập nhật trạng thái endpoint sau một request lỗi
    fn record_error(&mut self, url: &str) {
        // Cập nhật endpoint chính
        for endpoint in &mut self.primary_endpoints {
            if endpoint.info.url == url {
                endpoint.info.record_error();
                
                // Cập nhật metrics
                gauge!("rpc_endpoint_error_rate", endpoint.info.error_rate, "url" => url.to_string());
                counter!("rpc_endpoint_errors", 1, "url" => url.to_string());
                
                return;
            }
        }
        
        // Cập nhật endpoint backup
        for endpoint in &mut self.backup_endpoints {
            if endpoint.info.url == url {
                endpoint.info.record_error();
                
                // Cập nhật metrics
                gauge!("rpc_endpoint_error_rate", endpoint.info.error_rate, "url" => url.to_string());
                counter!("rpc_endpoint_errors", 1, "url" => url.to_string());
                
                return;
            }
        }
    }
    
    /// Thực hiện health check cho tất cả endpoint
    pub async fn health_check_all(&mut self) {
        // Kiểm tra endpoint chính
        for endpoint in &mut self.primary_endpoints {
            self.health_check_endpoint(endpoint).await;
        }
        
        // Kiểm tra endpoint backup
        for endpoint in &mut self.backup_endpoints {
            self.health_check_endpoint(endpoint).await;
        }
    }
    
    /// Thực hiện health check cho một endpoint cụ thể
    async fn health_check_endpoint(&mut self, endpoint: &mut PooledEndpoint) {
        let url = endpoint.info.url.clone();
        debug!("Performing health check for RPC endpoint: {}", url);
        
        let start = Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(5),
            endpoint.provider.get_block_number()
        ).await {
            Ok(Ok(_)) => {
                // Endpoint hoạt động
                let latency = start.elapsed().as_millis() as f64;
                endpoint.info.record_success(latency);
                debug!("Health check passed for RPC endpoint {} with latency {}ms", url, latency);
            },
            _ => {
                // Endpoint không hoạt động
                endpoint.info.record_error();
                warn!("Health check failed for RPC endpoint {}", url);
            }
        }
    }
    
    /// Thực hiện health check định kỳ
    pub async fn start_health_check_loop(pool: Arc<RwLock<Self>>) {
        let interval = {
            let guard = pool.read().unwrap();
            Duration::from_secs(guard.config.health_check_interval)
        };
        
        loop {
            // Đợi khoảng thời gian cấu hình
            tokio::time::sleep(interval).await;
            
            // Thực hiện health check
            let mut guard = pool.write().unwrap();
            guard.health_check_all().await;
        }
    }
    
    /// Lấy thông tin tất cả endpoint
    pub fn get_endpoints_info(&self) -> Vec<EndpointInfo> {
        let mut result = Vec::new();
        
        // Thêm endpoint chính
        for endpoint in &self.primary_endpoints {
            result.push(endpoint.info.clone());
        }
        
        // Thêm endpoint backup
        for endpoint in &self.backup_endpoints {
            result.push(endpoint.info.clone());
        }
        
        result
    }
    
    /// Thêm endpoint mới
    pub async fn add_endpoint(&mut self, url: &str, is_primary: bool) -> Result<()> {
        // Kiểm tra endpoint đã tồn tại chưa
        for endpoint in &self.primary_endpoints {
            if endpoint.info.url == url {
                return Err(anyhow!("Endpoint already exists: {}", url));
            }
        }
        
        for endpoint in &self.backup_endpoints {
            if endpoint.info.url == url {
                return Err(anyhow!("Endpoint already exists: {}", url));
            }
        }
        
        // Khởi tạo endpoint mới
        let priority = if is_primary {
            self.primary_endpoints.len() as u32
        } else {
            (self.backup_endpoints.len() + 100) as u32
        };
        
        let new_endpoint = Self::create_endpoint(url, priority, self.config.max_connections).await?;
        
        // Thêm vào danh sách tương ứng
        if is_primary {
            self.primary_endpoints.push(new_endpoint);
            info!("Added new primary RPC endpoint: {}", url);
        } else {
            self.backup_endpoints.push(new_endpoint);
            info!("Added new backup RPC endpoint: {}", url);
        }
        
        Ok(())
    }
    
    /// Thực hiện hàm async với provider từ pool
    pub async fn with_provider<F, Fut, T>(&mut self, operation: F) -> Result<T>
    where
        F: FnOnce(Provider<Http>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let provider_guard = self.get_provider().await?;
        let provider = provider_guard.provider.clone();
        
        let start = Instant::now();
        let result = operation(provider).await;
        let latency = start.elapsed().as_millis() as f64;
        
        // Cập nhật thông tin endpoint
        match &result {
            Ok(_) => {
                self.record_success(&provider_guard.endpoint_info.url, latency);
            },
            Err(_) => {
                self.record_error(&provider_guard.endpoint_info.url);
            }
        }
        
        result
    }
    
    /// Lấy thông tin cấu hình của pool
    pub fn get_config(&self) -> &ConnectionPoolConfig {
        &self.config
    }
    
    /// Lấy chain ID
    pub fn get_chain_id(&self) -> u64 {
        self.chain_id
    }
}

/// Guard để quản lý provider và trả lại permit khi drop
pub struct ProviderGuard {
    /// Provider đang sử dụng
    pub provider: Provider<Http>,
    /// Thông tin endpoint
    pub endpoint_info: EndpointInfo,
    /// Permit để quản lý số lượng kết nối đồng thời
    _permit: tokio::sync::OwnedSemaphorePermit,
    /// Reference đến pool (an toàn hơn con trỏ)
    pool: Weak<RwLock<RPCConnectionPool>>,
}

impl ProviderGuard {
    /// Thực hiện hàm async với provider
    pub async fn with_timeout<F, Fut, T>(&self, operation: F, timeout: Duration) -> Result<T>
    where
        F: FnOnce(Provider<Http>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let provider = self.provider.clone();
        
        match tokio::time::timeout(timeout, operation(provider)).await {
            Ok(result) => result,
            Err(_) => {
                // Timeout, báo lỗi
                if let Some(pool) = self.pool.upgrade() {
                    if let Ok(mut pool_guard) = pool.write() {
                        pool_guard.record_error(&self.endpoint_info.url);
                    }
                }
                Err(anyhow!("RPC request timed out"))
            }
        }
    }
    
    /// Báo cáo lỗi
    pub fn report_error(&self) {
        if let Some(pool) = self.pool.upgrade() {
            if let Ok(mut pool_guard) = pool.write() {
                pool_guard.record_error(&self.endpoint_info.url);
            }
        }
    }
    
    /// Báo cáo thành công
    pub fn report_success(&self, latency_ms: f64) {
        if let Some(pool) = self.pool.upgrade() {
            if let Ok(mut pool_guard) = pool.write() {
                pool_guard.record_success(&self.endpoint_info.url, latency_ms);
            }
        }
    }
}

/// Singleton để quản lý các connection pool
#[derive(Default)]
pub struct ConnectionPoolManager {
    pools: HashMap<u64, Arc<RwLock<RPCConnectionPool>>>,
}

impl ConnectionPoolManager {
    /// Tạo instance mới
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }
    
    /// Lấy hoặc tạo pool cho chain ID cụ thể
    pub async fn get_or_create_pool(
        &mut self,
        chain_id: u64,
        primary_urls: Vec<String>,
        backup_urls: Vec<String>,
        config: Option<ConnectionPoolConfig>,
    ) -> Result<Arc<RwLock<RPCConnectionPool>>> {
        if let Some(pool) = self.pools.get(&chain_id) {
            return Ok(pool.clone());
        }
        
        // Tạo pool mới
        let config = config.unwrap_or_default();
        let pool = RPCConnectionPool::new(chain_id, primary_urls, backup_urls, config, None).await?;
        let pool_arc = Arc::new(RwLock::new(pool));
        
        // Thêm vào manager
        self.pools.insert(chain_id, pool_arc.clone());
        
        // Bắt đầu health check loop
        let loop_pool = pool_arc.clone();
        tokio::spawn(async move {
            RPCConnectionPool::start_health_check_loop(loop_pool).await;
        });
        
        Ok(pool_arc)
    }
    
    /// Lấy pool hiện có cho chain ID
    pub fn get_pool(&self, chain_id: u64) -> Option<Arc<RwLock<RPCConnectionPool>>> {
        self.pools.get(&chain_id).cloned()
    }
    
    /// Thêm endpoint mới cho pool
    pub async fn add_endpoint(
        &self,
        chain_id: u64,
        url: &str,
        is_primary: bool,
    ) -> Result<()> {
        if let Some(pool) = self.pools.get(&chain_id) {
            let mut guard = pool.write().unwrap();
            guard.add_endpoint(url, is_primary).await
        } else {
            Err(anyhow!("No pool found for chain ID: {}", chain_id))
        }
    }
    
    /// Lấy thông tin tất cả pool
    pub fn get_pools_info(&self) -> HashMap<u64, Vec<EndpointInfo>> {
        let mut result = HashMap::new();
        
        for (chain_id, pool) in &self.pools {
            let guard = pool.read().unwrap();
            result.insert(*chain_id, guard.get_endpoints_info());
        }
        
        result
    }
}

/// Singleton để quản lý các connection pool toàn cục
pub static CONNECTION_POOL_MANAGER: once_cell::sync::Lazy<RwLock<ConnectionPoolManager>> = 
    once_cell::sync::Lazy::new(|| RwLock::new(ConnectionPoolManager::new()));

/// Lấy hoặc tạo pool cho chain ID
pub async fn get_or_create_pool(
    chain_id: u64,
    primary_urls: Vec<String>,
    backup_urls: Vec<String>,
    config: Option<ConnectionPoolConfig>,
) -> Result<Arc<RwLock<RPCConnectionPool>>> {
    let mut manager = CONNECTION_POOL_MANAGER.write().unwrap();
    manager.get_or_create_pool(chain_id, primary_urls, backup_urls, config).await
}

/// Lấy pool hiện có cho chain ID
pub fn get_pool(chain_id: u64) -> Option<Arc<RwLock<RPCConnectionPool>>> {
    let manager = CONNECTION_POOL_MANAGER.read().unwrap();
    manager.get_pool(chain_id)
}

/// Thêm endpoint mới cho pool
pub async fn add_endpoint(chain_id: u64, url: &str, is_primary: bool) -> Result<()> {
    let manager = CONNECTION_POOL_MANAGER.read().unwrap();
    manager.add_endpoint(chain_id, url, is_primary).await
}

/// Lấy thông tin tất cả pool
pub fn get_all_pools_info() -> HashMap<u64, Vec<EndpointInfo>> {
    let manager = CONNECTION_POOL_MANAGER.read().unwrap();
    manager.get_pools_info()
} 
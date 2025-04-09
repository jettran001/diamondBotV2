mod config;
mod nodes;
mod api;
mod storage;
mod snipebot;
mod service;
mod mempool;
mod chain_adapters;
mod user;
mod middleware;
mod token_status;
mod risk_analyzer;
mod gas_optimizer;
mod user_subscription;
mod subscription;
mod error;
// Đã hợp nhất endpoint_manager.rs vào main.rs

use std::sync::Arc;
use tokio;
use log::{info, error, debug, warn};
use tokio::signal;
use tokio::sync::{Mutex, mpsc, oneshot};
use crate::config::Config;
use crate::storage::Storage;
use crate::user::UserManager;
use crate::snipebot::SnipeBot;
use crate::service::ServiceManager;
use crate::mempool::MempoolWatcher;
use crate::chain_adapters::{init_chain_adapters, get_chain_adapter};
use crate::api::AppState;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    EnvFilter,
    prelude::*,
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use std::path::Path;
use ethers::providers::Provider;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow, Context};

// Struct EndpointInfo từ endpoint_manager.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub url: String,
    pub chain_id: u64,
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
}

// Struct EndpointStats từ endpoint_manager.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointStats {
    pub total_requests: u64,
    pub success_rate: f64,
    pub average_latency: u64,
    pub last_error: Option<String>,
    pub timestamp: u64,
}

// EndpointManager từ endpoint_manager.rs
#[derive(Debug)]
pub struct EndpointManager {
    endpoints: HashMap<String, Vec<EndpointInfo>>,
    stats: RwLock<HashMap<String, EndpointStats>>,
    last_update: RwLock<u64>,
}

impl EndpointManager {
    // Tạo một EndpointManager mới
    pub fn new() -> Self {
        Self {
            endpoints: HashMap::new(),
            stats: RwLock::new(HashMap::new()),
            last_update: RwLock::new(0),
        }
    }
    
    // Thêm endpoint mới
    pub fn add_endpoint(&mut self, endpoint: EndpointInfo) {
        let chain_id = endpoint.chain_id.to_string();
        
        if !self.endpoints.contains_key(&chain_id) {
            self.endpoints.insert(chain_id.clone(), Vec::new());
        }
        
        if let Some(endpoints) = self.endpoints.get_mut(&chain_id) {
            // Kiểm tra xem endpoint đã tồn tại chưa
            if !endpoints.iter().any(|e| e.url == endpoint.url) {
                endpoints.push(endpoint);
                // Sắp xếp theo priority giảm dần
                endpoints.sort_by(|a, b| b.priority.cmp(&a.priority));
            }
        }
    }
    
    // Lấy endpoint tốt nhất cho một chain
    pub fn get_best_endpoint(&self, chain_id: u64) -> Option<EndpointInfo> {
        let chain_id = chain_id.to_string();
        
        if let Some(endpoints) = self.endpoints.get(&chain_id) {
            // Lọc các endpoint đang enabled và trả về endpoint có priority cao nhất
            endpoints.iter()
                .filter(|e| e.enabled)
                .next()
                .cloned()
        } else {
            None
        }
    }
    
    // Cập nhật thống kê cho một endpoint
    pub async fn update_stats(&self, url: &str, success: bool, latency_ms: u64, error: Option<String>) {
        let mut stats = self.stats.write().await;
        
        let endpoint_stats = stats.entry(url.to_string()).or_insert(EndpointStats {
            total_requests: 0,
            success_rate: 1.0,
            average_latency: 0,
            last_error: None,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        
        // Cập nhật thống kê
        endpoint_stats.total_requests += 1;
        
        // Cập nhật success rate (tỷ lệ thành công)
        if endpoint_stats.total_requests > 1 {
            let old_success_count = (endpoint_stats.success_rate * (endpoint_stats.total_requests - 1) as f64) as u64;
            let new_success_count = old_success_count + if success { 1 } else { 0 };
            endpoint_stats.success_rate = new_success_count as f64 / endpoint_stats.total_requests as f64;
        } else {
            endpoint_stats.success_rate = if success { 1.0 } else { 0.0 };
        }
        
        // Cập nhật latency trung bình
        if endpoint_stats.total_requests > 1 {
            let old_total_latency = endpoint_stats.average_latency * (endpoint_stats.total_requests - 1);
            endpoint_stats.average_latency = (old_total_latency + latency_ms) / endpoint_stats.total_requests;
        } else {
            endpoint_stats.average_latency = latency_ms;
        }
        
        // Cập nhật lỗi cuối cùng
        if !success {
            endpoint_stats.last_error = error;
        }
        
        // Cập nhật timestamp
        endpoint_stats.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        // Cập nhật last_update
        let mut last_update = self.last_update.write().await;
        *last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    // Vô hiệu hóa endpoint không khả dụng
    pub fn disable_endpoint(&mut self, chain_id: u64, url: &str) -> bool {
        let chain_id = chain_id.to_string();
        
        if let Some(endpoints) = self.endpoints.get_mut(&chain_id) {
            for endpoint in endpoints.iter_mut() {
                if endpoint.url == url {
                    endpoint.enabled = false;
                    return true;
                }
            }
        }
        
        false
    }
    
    // Kiểm tra sức khỏe của các endpoints
    pub async fn check_endpoints_health(&mut self) -> Result<HashMap<String, Vec<EndpointInfo>>> {
        let mut results = HashMap::new();
        
        for (chain_id, endpoints) in &mut self.endpoints {
            for endpoint in endpoints.iter_mut() {
                // Thực hiện kiểm tra sức khỏe
                match check_endpoint_health(&endpoint.url).await {
                    Ok(is_healthy) => {
                        endpoint.enabled = is_healthy;
                        
                        if !is_healthy {
                            warn!("Endpoint {} cho chain_id {} không khả dụng", endpoint.url, chain_id);
                        }
                    },
                    Err(e) => {
                        endpoint.enabled = false;
                        warn!("Lỗi khi kiểm tra endpoint {} cho chain_id {}: {}", endpoint.url, chain_id, e);
                    }
                }
            }
            
            // Sắp xếp lại theo priority và tính khả dụng
            endpoints.sort_by(|a, b| {
                if a.enabled == b.enabled {
                    b.priority.cmp(&a.priority)
                } else {
                    a.enabled.cmp(&b.enabled).reverse()
                }
            });
            
            results.insert(chain_id.clone(), endpoints.clone());
        }
        
        Ok(results)
    }
    
    // Lấy tất cả endpoints cho một chain
    pub fn get_endpoints_for_chain(&self, chain_id: u64) -> Vec<EndpointInfo> {
        let chain_id = chain_id.to_string();
        
        if let Some(endpoints) = self.endpoints.get(&chain_id) {
            endpoints.clone()
        } else {
            Vec::new()
        }
    }
    
    // Cung cấp thống kê hiện tại
    pub async fn get_stats(&self) -> HashMap<String, EndpointStats> {
        self.stats.read().await.clone()
    }
    
    // Cung cấp thời gian cập nhật cuối cùng
    pub async fn get_last_update(&self) -> u64 {
        *self.last_update.read().await
    }
}

// Hàm hỗ trợ để kiểm tra sức khỏe của một endpoint
async fn check_endpoint_health(endpoint_url: &str) -> Result<bool> {
    // Gửi một request đơn giản để kiểm tra xem endpoint có phản hồi không
    let provider = Provider::<ethers::providers::Http>::try_from(endpoint_url)
        .map_err(|e| anyhow!("Không thể kết nối tới endpoint {}: {}", endpoint_url, e))?;
        
    // Đặt timeout cho request
    match tokio::time::timeout(
        Duration::from_secs(5),
        provider.get_block_number()
    ).await {
        Ok(Ok(_)) => {
            // Endpoint phản hồi và trả về block number
            Ok(true)
        },
        Ok(Err(e)) => {
            // Endpoint phản hồi nhưng có lỗi
            warn!("Endpoint {} trả về lỗi: {}", endpoint_url, e);
            Ok(false)
        },
        Err(_) => {
            // Endpoint không phản hồi (timeout)
            warn!("Endpoint {} không phản hồi trong thời gian quy định", endpoint_url);
            Ok(false)
        }
    }
}

// Hàm quản lý endpoints cho toàn bộ ứng dụng
async fn start_endpoint_manager_service(endpoint_manager: Arc<RwLock<EndpointManager>>) {
    info!("Khởi động Endpoint Manager service");
    
    // Chạy trong vòng lặp vô hạn
    loop {
        // Kiểm tra sức khỏe các endpoints mỗi 5 phút
        tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
        
        // Kiểm tra sức khỏe các endpoints
        match endpoint_manager.write().await.check_endpoints_health().await {
            Ok(results) => {
                for (chain_id, endpoints) in results {
                    let available_count = endpoints.iter().filter(|e| e.enabled).count();
                    info!("Chain {}: {}/{} endpoints khả dụng", chain_id, available_count, endpoints.len());
                }
            },
            Err(e) => {
                error!("Lỗi khi kiểm tra sức khỏe endpoints: {}", e);
            }
        }
    }
}

// Hàm để lấy ví từ cấu hình (cần được triển khai)
fn get_wallet_from_config(config: &Config) -> Option<ethers::signers::LocalWallet> {
    // Triển khai lấy ví từ config
    None // Placeholder - cần thay thế bằng triển khai thực tế
}

// Đảm bảo main.rs export module này
pub use error::{TransactionError, classify_blockchain_error, get_recovery_info};
// Export EndpointManager để các module khác sử dụng
pub use self::{EndpointInfo, EndpointStats, EndpointManager};

async fn start_gas_optimizer_service() {
    info!("Khởi động Gas Optimizer service");
    
    // Chạy trong vòng lặp vô hạn
    loop {
        // Cập nhật thông tin gas cho tất cả các chain được hỗ trợ
        let chains = chain_adapters::configs::get_supported_chains();
        
        for chain_name in chains {
            // Skip nếu không thể lấy adapter
            let adapter = match get_chain_adapter(chain_name) {
                Ok(adapter) => adapter,
                Err(e) => {
                    error!("Không thể lấy adapter cho chain {}: {}", chain_name, e);
                    continue;
                }
            };
            
            // Lấy provider từ adapter
            let provider = adapter.get_config().rpc_url.clone();
            
            // Cập nhật gas history
            match gas_optimizer::update_gas_price_history(&provider, chain_name).await {
                Ok(_) => {
                    debug!("Cập nhật gas price history cho chain {} thành công", chain_name);
                },
                Err(e) => {
                    error!("Lỗi khi cập nhật gas price history cho chain {}: {}", chain_name, e);
                }
            }
        }
        
        // Dọn dẹp gas cache
        gas_optimizer::cleanup_gas_cache();
        
        // Đợi 30 giây trước khi cập nhật tiếp
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    }
}

async fn cleanup_tasks() {
    loop {
        // Đợi 1 giờ trước khi thực hiện dọn dẹp
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        
        // Dọn dẹp rate limit
        common::middleware::cleanup_rate_limits().await;
        
        // Dọn dẹp gas cache
        gas_optimizer::cleanup_gas_cache();
        
        // Các task dọn dẹp khác có thể thêm vào đây
        
        info!("Đã hoàn thành dọn dẹp các resource định kỳ");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Thiết lập logging
    let log_dir = Path::new("logs");
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir)?;
    }
    
    let file_appender = RollingFileAppender::new(
        Rotation::DAILY,
        "logs",
        "snipebot.log",
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into())
        )
        .with(
            fmt::Layer::new()
                .with_writer(std::io::stderr)
                .with_ansi(true)
        )
        .with(
            fmt::Layer::new()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_span_events(FmtSpan::CLOSE)
        )
        .init();

    info!("Khởi động Snipebot Server...");
    
    // Tải cấu hình
    let config = match config::Config::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            info!("Không thể tải cấu hình từ biến môi trường, sử dụng cấu hình mặc định: {}", e);
            config::Config::new()
        }
    };
    
    // Khởi tạo storage
    let storage = Arc::new(storage::Storage::new());
    
    // Tải dữ liệu từ file nếu có
    if let Err(e) = storage.load_from_file("transactions.json").await {
        error!("Không thể tải dữ liệu từ file: {}", e);
    }
    
    // Khởi tạo UserManager
    let user_manager = match UserManager::new("data/users.json").await {
        Ok(um) => Arc::new(Mutex::new(um)),
        Err(e) => {
            error!("Không thể khởi tạo User Manager: {}", e);
            return Err(Box::new(e));
        }
    };
    
    // Khởi tạo Endpoint Manager
    let endpoint_manager = Arc::new(RwLock::new(EndpointManager::new()));
    
    // Thêm endpoints từ config vào EndpointManager
    {
        let mut endpoint_manager = endpoint_manager.write().await;
        
        // Thêm các endpoint từ cấu hình
        for (chain_id, endpoints) in &config.endpoints {
            for (i, url) in endpoints.iter().enumerate() {
                let endpoint_info = EndpointInfo {
                    url: url.clone(),
                    chain_id: *chain_id,
                    name: format!("endpoint-{}-{}", chain_id, i+1),
                    priority: (endpoints.len() - i) as u32, // Priority giảm dần
                    enabled: true,
                };
                
                endpoint_manager.add_endpoint(endpoint_info);
            }
        }
    }
    
    // Bắt đầu service kiểm tra sức khỏe endpoints
    tokio::spawn(start_endpoint_manager_service(Arc::clone(&endpoint_manager)));
    
    // Khởi tạo các chain adapter
    if let Err(e) = init_chain_adapters().await {
        error!("Không thể khởi tạo các Chain Adapter: {}", e);
        return Err(Box::new(e));
    }
    
    // Lấy chain adapter cho chain mặc định từ config
    let chain_name = config.chain_name.clone();
    let chain_adapter = match get_chain_adapter(&chain_name) {
        Ok(adapter) => adapter,
        Err(e) => {
            error!("Không thể lấy Chain Adapter cho {}: {}", chain_name, e);
            return Err(Box::new(e));
        }
    };

    // Tạo SnipeBot với adapter
    let snipe_bot = match snipebot::SnipeBot::new(config.clone(), Arc::clone(&storage), chain_adapter).await {
        Ok(bot) => Arc::new(bot),
        Err(e) => {
            error!("Không thể khởi tạo SnipeBot: {}", e);
            return Err(Box::new(e));
        }
    };
    
    // Tạo AppState cho API Server
    let app_state = Arc::new(AppState {
        config: config.clone(),
        storage: Arc::clone(&storage),
        snipebot: Arc::clone(&snipe_bot),
        user_manager: Arc::clone(&user_manager),
        endpoint_manager: Arc::clone(&endpoint_manager), // Thêm endpoint_manager vào AppState
    });
    
    // Khởi động các dịch vụ trong thread riêng biệt
    let snipebot_arc = Arc::new(snipe_bot);

    // Khởi động mempool watcher trong một task riêng
    let snipe_bot_clone = Arc::clone(&snipebot_arc);
    let mempool_handle = tokio::spawn(async move {
        match snipe_bot_clone.initialize_mempool_watcher().await {
            Ok(_) => info!("Mempool watcher đã khởi động thành công"),
            Err(e) => error!("Lỗi khi khởi động mempool watcher: {}", e),
        }
    });

    // Khởi động service manager trong một task riêng
    let snipe_bot_clone = Arc::clone(&snipebot_arc);
    let service_manager_handle = tokio::spawn(async move {
        match snipe_bot_clone.initialize_service_manager().await {
            Ok(_) => info!("Service manager đã khởi động thành công"),
            Err(e) => error!("Lỗi khi khởi động service manager: {}", e),
        }
    });

    // Khởi động gas optimizer service
    tokio::spawn(start_gas_optimizer_service());
    
    // Khởi động task dọn dẹp định kỳ
    tokio::spawn(cleanup_tasks());

    // Tạo API server
    let api_handle = tokio::spawn(async move {
        match api::create_api_server(app_state).await {
            Ok(_) => info!("API server đã dừng bình thường"),
            Err(e) => error!("API server lỗi: {}", e),
        }
    });
    
    // Handle CTRL+C và các tín hiệu shutdown khác
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Đã nhận tín hiệu tắt, đang tắt các dịch vụ...");
            // Các công việc dọn dẹp trước khi tắt
        }
        Err(e) => {
            error!("Không thể bắt tín hiệu CTRL+C: {}", e);
        }
    }
    
    // Đợi các task kết thúc
    let _ = tokio::join!(
        mempool_handle,
        service_manager_handle,
        api_handle
    );
    
    info!("Snipebot Server đã tắt thành công");
    Ok(())
}
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

use std::sync::Arc;
use tokio;
use log::{info, error, debug};
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
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Hàm để lấy ví từ cấu hình (cần được triển khai)
fn get_wallet_from_config(config: &Config) -> Option<ethers::signers::LocalWallet> {
    // Triển khai lấy ví từ config
    None // Placeholder - cần thay thế bằng triển khai thực tế
}

// Đảm bảo main.rs export module này
pub use error::{TransactionError, classify_blockchain_error, get_recovery_info};

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
        middleware::cleanup_rate_limits().await;
        
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
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use log::{info, error, warn};
use serde::{Serialize, Deserialize};
use super::config::Config;
use super::storage::Storage;
use super::snipebot::{SnipeBot, TokenInfo};
use crate::error::{TransactionError, classify_blockchain_error, get_recovery_info};
use crate::user_manager::UserManager;
use crate::token_status::TokenSafetyLevel;
use crate::mempool::{PendingSwap, ArbitrageOpportunity, SandwichOpportunity};

// Enum để phân loại các loại service message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceMessage {
    StartService(String),
    StopService(String),
    TransactionRequest(TransactionRequest),
    MonitorRequest(MonitorRequest),
    StatusRequest,
    NewBlock(u64),
    Transaction(String),
    PriceAlert { token: String, price: f64, change: f64 },
    RiskAlert { token: String, risk_score: u8, message: String },
    PendingSwap(PendingSwap),
    LargeTransaction { token_address: String, is_buy: bool, amount_usd: f64 },
    ArbitrageOpportunity(ArbitrageOpportunity),
    SandwichOpportunity(SandwichOpportunity),
    ReserveBalanceAlert { current_percent: f64 },
    TokenSafetyUpdate { token_address: String, safety_level: TokenSafetyLevel },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub id: String,
    pub token_address: String,
    pub amount: String,
    pub gas_price: Option<u64>,
    pub gas_limit: Option<u64>,
    pub slippage: Option<f64>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorRequest {
    pub tokens: Vec<String>,
    pub interval: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub service_name: String,
    pub uptime: u64,
    pub queue_size: usize,
}

// Service Manager quản lý tất cả các service
pub struct ServiceManager {
    config: Config,
    storage: Arc<Storage>,
    snipebot: Arc<SnipeBot>,
    tx_sender: mpsc::Sender<ServiceMessage>,
    rx_receiver: mpsc::Receiver<ServiceMessage>,
    start_time: std::time::Instant,
    running: bool,
}

impl ServiceManager {
    pub fn new(config: Config, storage: Arc<Storage>, snipebot: Arc<SnipeBot>) -> Self {
        let (tx, rx) = mpsc::channel(100); // Kích thước buffer 100
        Self {
            config,
            storage,
            snipebot,
            tx_sender: tx,
            rx_receiver: rx,
            start_time: std::time::Instant::now(),
            running: false,
        }
    }
    
    pub fn get_sender(&self) -> mpsc::Sender<ServiceMessage> {
        self.tx_sender.clone()
    }
    
    pub async fn start(&mut self) {
        info!("Khởi động Service Manager");
        self.running = true;
        
        // Khởi động transaction worker
        let tx_worker = TransactionWorker::new(
            self.config.clone(),
            Arc::clone(&self.storage),
            Arc::clone(&self.snipebot)
        );
        
        // Khởi động monitor service
        let monitor_service = MonitorService::new(
            self.config.clone(),
            Arc::clone(&self.snipebot)
        );
        
        // Vòng lặp xử lý message
        while self.running {
            if let Some(msg) = self.rx_receiver.recv().await {
                match msg {
                    ServiceMessage::StartService(name) => {
                        info!("Yêu cầu khởi động service: {}", name);
                        // Logic khởi động service theo tên
                    },
                    ServiceMessage::StopService(name) => {
                        info!("Yêu cầu dừng service: {}", name);
                        if name == "all" {
                            self.running = false;
                            info!("Dừng tất cả các service");
                        }
                    },
                    ServiceMessage::TransactionRequest(tx_req) => {
                        info!("Nhận yêu cầu giao dịch: {} cho token {}", tx_req.id, tx_req.token_address);
                        // Forward request đến transaction worker
                        match tx_worker.process_transaction(tx_req).await {
                            Ok(result) => {
                                info!("Giao dịch {} hoàn thành: {:?}", result.transaction_hash.unwrap_or_default(), result);
                            },
                            Err(e) => {
                                error!("Giao dịch thất bại: {}", e);
                            }
                        }
                    },
                    ServiceMessage::MonitorRequest(monitor_req) => {
                        info!("Nhận yêu cầu giám sát {} tokens", monitor_req.tokens.len());
                        // Forward request đến monitor service
                        match monitor_service.start_monitoring(monitor_req.tokens, monitor_req.interval.unwrap_or(60)).await {
                            Ok(_) => {
                                info!("Bắt đầu giám sát tokens");
                            },
                            Err(e) => {
                                error!("Lỗi khi giám sát tokens: {}", e);
                            }
                        }
                    },
                    ServiceMessage::StatusRequest => {
                        let status = ServiceStatus {
                            running: self.running,
                            service_name: "ServiceManager".to_string(),
                            uptime: self.start_time.elapsed().as_secs(),
                            queue_size: self.rx_receiver.capacity().unwrap_or(0),
                        };
                        info!("Trạng thái service: {:?}", status);
                    }
                    _ => {
                        // Handle other message types
                    }
                }
            }
        }
        
        info!("Service Manager đã dừng");
    }
}

// Transaction Worker xử lý các giao dịch
pub struct TransactionWorker {
    config: Config,
    storage: Arc<Storage>,
    snipebot: Arc<SnipeBot>,
}

impl TransactionWorker {
    pub fn new(config: Config, storage: Arc<Storage>, snipebot: Arc<SnipeBot>) -> Self {
        Self {
            config,
            storage,
            snipebot,
        }
    }
    
    pub async fn process_transaction(&self, tx_req: TransactionRequest) -> Result<super::snipebot::SnipeResult, Box<dyn std::error::Error>> {
        info!("Xử lý giao dịch: {}", tx_req.id);
        
        // Parse số lượng
        let amount = ethers::utils::parse_ether(tx_req.amount.clone())?;
        
        // Tạo thông tin token
        let token_info = TokenInfo {
            address: tx_req.token_address.clone(),
            symbol: "UNKNOWN".to_string(),
            decimals: 18,
            router: self.config.router_address.clone(),
            pair: None,
        };
        
        // Tạo cấu hình snipe
        let snipe_config = super::snipebot::SnipeConfig {
            gas_limit: tx_req.gas_limit.unwrap_or(self.config.default_gas_limit),
            gas_price: tx_req.gas_price.unwrap_or(self.config.default_gas_price),
            slippage: tx_req.slippage.unwrap_or(self.config.default_slippage),
            timeout: 60,
            auto_approve: true,
        };
        
        // Thực hiện giao dịch với xử lý lỗi tốt hơn
        match self.snipebot.snipe(&token_info, amount, &snipe_config).await {
            Ok(result) => {
                info!("Giao dịch {} hoàn thành thành công", tx_req.id);
                Ok(result)
            },
            Err(e) => {
                let error_msg = e.to_string();
                error!("Lỗi khi xử lý giao dịch {}: {}", tx_req.id, error_msg);
                
                // Phân loại lỗi
                let tx_error = match e.downcast::<TransactionError>() {
                    Ok(tx_error) => *tx_error,
                    Err(_) => classify_blockchain_error(&error_msg),
                };
                
                // Ghi nhật ký chi tiết
                warn!(
                    transaction_id = tx_req.id,
                    token = tx_req.token_address,
                    error_type = ?tx_error,
                    error = error_msg,
                    "Phân loại lỗi giao dịch thất bại"
                );
                
                // Tạo kết quả thất bại để trả về
                let result = super::snipebot::SnipeResult {
                    transaction_hash: None,
                    success: false,
                    token_address: tx_req.token_address.clone(),
                    amount_in: tx_req.amount.clone(),
                    estimated_amount_out: None,
                    error: Some(format!("{}: {}", tx_error, error_msg)),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                
                // Nên lưu lỗi để phân tích sau
                self.storage.add_failed_transaction(tx_req.id.clone(), &result, &format!("{:?}", tx_error)).await?;
                
                Err(Box::new(tx_error))
            }
        }
    }
}

// Monitor Service giám sát token
pub struct MonitorService {
    config: Config,
    snipebot: Arc<SnipeBot>,
    monitoring: bool,
}

impl MonitorService {
    pub fn new(config: Config, snipebot: Arc<SnipeBot>) -> Self {
        Self {
            config,
            snipebot,
            monitoring: false,
        }
    }
    
    pub async fn start_monitoring(&self, tokens: Vec<String>, interval_seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Clone dữ liệu cần thiết cho task
        let snipebot = Arc::clone(&self.snipebot);
        let tokens_clone = tokens.clone();
        
        // Tạo task định kỳ giám sát
        tokio::spawn(async move {
            info!("Bắt đầu giám sát {} tokens, kiểm tra mỗi {} giây", tokens_clone.len(), interval_seconds);
            
            let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
            
            loop {
                interval.tick().await;
                
                for token in &tokens_clone {
                    match snipebot.get_token_balance(token).await {
                        Ok(balance) => {
                            info!("Token {}: balance = {}", token, balance);
                        },
                        Err(e) => {
                            warn!("Không thể lấy số dư cho token {}: {}", token, e);
                        }
                    }
                    
                    // Tránh gửi quá nhiều request cùng lúc
                    sleep(Duration::from_millis(500)).await;
                }
            }
        });
        
        Ok(())
    }
}

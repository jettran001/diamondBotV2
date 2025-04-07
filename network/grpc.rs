use tonic::{transport::Server, Request, Response, Status};
use std::sync::Arc;
use tokio::sync::mpsc;
use anyhow::Result;
use tracing::{info, error, warn};
use std::time::Duration;
use tokio::time::sleep;

use crate::config::Config;
use crate::snipebot::{SnipeBot, SnipeConfig, TokenInfo};

// Import các định nghĩa được tạo bởi tonic-build
pub mod snipebot {
    tonic::include_proto!("snipebot");
}

use snipebot::{
    snipe_service_server::{SnipeService, SnipeServiceServer},
    SnipeRequest, SnipeResponse, StatusRequest, StatusResponse,
    MonitorRequest, MonitorResponse
};

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(1);

pub struct SnipeBotGrpcService {
    config: Arc<Config>,
    snipebot: Arc<SnipeBot>,
    start_time: std::time::Instant,
}

impl SnipeBotGrpcService {
    pub fn new(config: Arc<Config>, snipebot: Arc<SnipeBot>) -> Self {
        Self {
            config,
            snipebot,
            start_time: std::time::Instant::now(),
        }
    }
    
    // Validate input parameters
    fn validate_snipe_request(&self, req: &SnipeRequest) -> Result<(), Status> {
        if req.token_address.is_empty() {
            return Err(Status::invalid_argument("Token address không được để trống"));
        }
        
        if req.amount.is_empty() {
            return Err(Status::invalid_argument("Số lượng token không được để trống"));
        }
        
        if req.gas_limit == 0 {
            return Err(Status::invalid_argument("Gas limit phải lớn hơn 0"));
        }
        
        if req.gas_price == 0 {
            return Err(Status::invalid_argument("Gas price phải lớn hơn 0"));
        }
        
        if req.slippage <= 0.0 || req.slippage > 100.0 {
            return Err(Status::invalid_argument("Slippage phải nằm trong khoảng 0-100"));
        }
        
        Ok(())
    }
}

#[tonic::async_trait]
impl SnipeService for SnipeBotGrpcService {
    async fn execute_snipe(&self, request: Request<SnipeRequest>) -> Result<Response<SnipeResponse>, Status> {
        let req = request.into_inner();
        info!(token_address = %req.token_address, amount = %req.amount, "Nhận yêu cầu snipe qua gRPC");
        
        // Validate input
        self.validate_snipe_request(&req)?;
        
        // Parse lượng token với retry
        let mut retries = 0;
        let amount = loop {
            match ethers::utils::parse_ether(req.amount.clone()) {
                Ok(amount) => break amount,
                Err(e) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        error!(error = %e, "Lỗi khi parse số lượng token sau {} lần thử", MAX_RETRIES);
                        return Err(Status::invalid_argument(format!("Số lượng token không hợp lệ: {}", e)));
                    }
                    warn!(retry = %retries, "Thử lại parse số lượng token");
                    sleep(RETRY_DELAY).await;
                }
            }
        };
        
        // Tạo thông tin token
        let token_info = TokenInfo {
            address: req.token_address.clone(),
            symbol: "UNKNOWN".to_string(),
            decimals: 18,
            router: self.config.router_address.clone(),
            pair: None,
        };
        
        // Tạo cấu hình snipe
        let snipe_config = SnipeConfig {
            gas_limit: req.gas_limit,
            gas_price: req.gas_price,
            slippage: req.slippage,
            timeout: 60,
            auto_approve: true,
        };
        
        // Thực hiện snipe với retry
        let mut retries = 0;
        loop {
            match self.snipebot.snipe(&token_info, amount, &snipe_config).await {
                Ok(result) => {
                    let reply = SnipeResponse {
                        transaction_hash: result.transaction_hash.unwrap_or_default(),
                        success: result.success,
                        error: result.error.unwrap_or_default(),
                    };
                    
                    return Ok(Response::new(reply));
                },
                Err(e) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        error!(error = %e, "Lỗi khi thực hiện snipe sau {} lần thử", MAX_RETRIES);
                        return Err(Status::internal(format!("Lỗi khi thực hiện snipe: {}", e)));
                    }
                    warn!(retry = %retries, "Thử lại snipe");
                    sleep(RETRY_DELAY).await;
                }
            }
        }
    }
    
    async fn get_status(&self, _: Request<StatusRequest>) -> Result<Response<StatusResponse>, Status> {
        let reply = StatusResponse {
            online: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime: self.start_time.elapsed().as_secs(),
        };
        
        Ok(Response::new(reply))
    }
    
    async fn monitor_tokens(&self, request: Request<MonitorRequest>) -> Result<Response<MonitorResponse>, Status> {
        let req = request.into_inner();
        info!(tokens = ?req.token_addresses, "Nhận yêu cầu theo dõi tokens qua gRPC");
        
        // Validate input
        if req.token_addresses.is_empty() {
            return Err(Status::invalid_argument("Danh sách token không được để trống"));
        }
        
        // Thực hiện monitor với retry
        let mut retries = 0;
        loop {
            match self.snipebot.monitor_tokens(req.token_addresses.clone()).await {
                Ok(_) => {
                    let reply = MonitorResponse {
                        success: true,
                        message: "Đã bắt đầu theo dõi tokens".to_string(),
                    };
                    
                    return Ok(Response::new(reply));
                },
                Err(e) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        error!(error = %e, "Lỗi khi theo dõi tokens sau {} lần thử", MAX_RETRIES);
                        return Err(Status::internal(format!("Lỗi khi theo dõi tokens: {}", e)));
                    }
                    warn!(retry = %retries, "Thử lại theo dõi tokens");
                    sleep(RETRY_DELAY).await;
                }
            }
        }
    }
}

use tonic::{transport::Server, Request, Response, Status};
use std::sync::Arc;
use tokio::sync::Mutex;

// Import protobuf đã biên dịch
use crate::proto::snipebot::{
    snipebot_service_server::{SnipebotService, SnipebotServiceServer},
    SnipeRequest, SnipeResponse,
    GetWalletsRequest, GetWalletsResponse,
    CreateWalletRequest, CreateWalletResponse,
    ImportWalletRequest, ImportWalletResponse,
};

use crate::snipebot::SnipeBot;
use diamond_wallet::WalletInfo;

// Triển khai service
pub struct SnipebotGrpcService {
    snipebot: Arc<SnipeBot>,
}

impl SnipebotGrpcService {
    pub fn new(snipebot: Arc<SnipeBot>) -> Self {
        Self { snipebot }
    }
}

#[tonic::async_trait]
impl SnipebotService for SnipebotGrpcService {
    async fn snipe(&self, request: Request<SnipeRequest>) -> Result<Response<SnipeResponse>, Status> {
        let req = request.into_inner();
        
        // Convert request to the format expected by the snipebot
        let token_info = crate::snipebot::TokenInfo {
            address: req.token_address,
            symbol: "UNKNOWN".to_string(),
            decimals: 18,
            router: req.router_address.unwrap_or_else(|| self.snipebot.config.router_address.clone()),
            pair: None,
        };
        
        let amount_in = match ethers::utils::parse_ether(&req.amount) {
            Ok(amount) => amount,
            Err(e) => return Err(Status::invalid_argument(format!("Invalid amount: {}", e))),
        };
        
        let snipe_config = crate::snipebot::SnipeConfig {
            gas_limit: req.gas_limit.unwrap_or(self.snipebot.config.default_gas_limit),
            gas_price: req.gas_price.unwrap_or(self.snipebot.config.default_gas_price),
            slippage: req.slippage.unwrap_or(self.snipebot.config.default_slippage),
            timeout: req.timeout.unwrap_or(60),
            auto_approve: req.auto_approve.unwrap_or(true),
        };
        
        // Execute the snipe operation
        match self.snipebot.snipe(&token_info, amount_in, &snipe_config).await {
            Ok(result) => {
                // Convert the result to gRPC response
                let response = SnipeResponse {
                    success: result.success,
                    transaction_hash: result.transaction_hash.unwrap_or_default(),
                    amount_in: result.amount_in,
                    estimated_amount_out: result.estimated_amount_out.unwrap_or_default(),
                    error: result.error.unwrap_or_default(),
                    timestamp: result.timestamp as i64,
                };
                
                Ok(Response::new(response))
            },
            Err(e) => {
                Err(Status::internal(format!("Snipe operation failed: {}", e)))
            }
        }
    }
    
    async fn get_wallets(&self, _request: Request<GetWalletsRequest>) -> Result<Response<GetWalletsResponse>, Status> {
        match self.snipebot.get_wallet_list().await {
            Ok(wallets) => {
                // Sử dụng view an toàn của ví
                let wallet_responses = wallets.into_iter().map(|w| {
                    proto::snipebot::WalletInfo {
                        address: w.address,
                        // Không còn private_key và mnemonic
                        private_key: String::new(), // Field trống
                        mnemonic: String::new(),    // Field trống
                        balance: w.balance.unwrap_or_default(),
                        chain_id: w.chain_id as i64,
                        created_at: w.created_at as i64,
                        last_used: w.last_used as i64,
                    }
                }).collect();
                
                Ok(Response::new(GetWalletsResponse {
                    wallets: wallet_responses,
                }))
            },
            Err(e) => {
                Err(Status::internal(format!("Failed to get wallets: {}", e)))
            }
        }
    }
    
    async fn create_wallet(&self, _request: Request<CreateWalletRequest>) -> Result<Response<CreateWalletResponse>, Status> {
        match self.snipebot.create_and_switch_wallet().await {
            Ok(wallet) => {
                // Convert wallet to gRPC response
                let wallet_response = proto::snipebot::WalletInfo {
                    address: wallet.address,
                    private_key: wallet.private_key,
                    mnemonic: wallet.mnemonic.unwrap_or_default(),
                    balance: wallet.balance.unwrap_or_default(),
                    chain_id: wallet.chain_id as i64,
                    created_at: wallet.created_at as i64,
                    last_used: wallet.last_used as i64,
                };
                
                Ok(Response::new(CreateWalletResponse {
                    wallet: Some(wallet_response),
                }))
            },
            Err(e) => {
                Err(Status::internal(format!("Failed to create wallet: {}", e)))
            }
        }
    }
    
    async fn import_wallet(&self, request: Request<ImportWalletRequest>) -> Result<Response<ImportWalletResponse>, Status> {
        let req = request.into_inner();
        
        let wallet_result = if req.private_key.is_empty() == false {
            self.snipebot.import_wallet_from_private_key(&req.private_key).await
        } else if req.mnemonic.is_empty() == false {
            self.snipebot.import_wallet_from_mnemonic(&req.mnemonic, req.passphrase.as_deref()).await
        } else {
            return Err(Status::invalid_argument("Either private_key or mnemonic must be provided"));
        };
        
        match wallet_result {
            Ok(wallet) => {
                // Convert wallet to gRPC response
                let wallet_response = proto::snipebot::WalletInfo {
                    address: wallet.address,
                    private_key: wallet.private_key,
                    mnemonic: wallet.mnemonic.unwrap_or_default(),
                    balance: wallet.balance.unwrap_or_default(),
                    chain_id: wallet.chain_id as i64,
                    created_at: wallet.created_at as i64,
                    last_used: wallet.last_used as i64,
                };
                
                Ok(Response::new(ImportWalletResponse {
                    wallet: Some(wallet_response),
                }))
            },
            Err(e) => {
                Err(Status::internal(format!("Failed to import wallet: {}", e)))
            }
        }
    }
}

// Start gRPC server
pub async fn start_grpc_server(snipebot: Arc<SnipeBot>, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let addr = addr.parse()?;
    let snipebot_service = SnipebotGrpcService::new(snipebot);
    
    println!("gRPC server listening on {}", addr);
    
    Server::builder()
        .add_service(SnipebotServiceServer::new(snipebot_service))
        .serve(addr)
        .await?;
    
    Ok(())
}

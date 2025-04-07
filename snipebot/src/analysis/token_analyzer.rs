use ethers::{
    prelude::*,
    providers::{Provider, Middleware},
    types::{Address, Bytes, U256},
    contract::{Contract, ContractFactory},
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::str::FromStr;
use tracing::{info, error, debug, warn};
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use super::TokenAnalysisResult;
use crate::config::Config;
use crate::wasm_engine::WasmEngine;
use crate::tensorflow_model::TensorflowModel;
use crate::snipebot_error::SnipebotError;

// Lưu trữ Token Analyzer global
static TOKEN_ANALYZER: OnceCell<Arc<Mutex<TokenAnalyzer>>> = OnceCell::new();

// Định nghĩa các function signature phổ biến để phân tích
struct FunctionSignatures {
    transfer: [u8; 4],
    transfer_from: [u8; 4],
    approve: [u8; 4],
    mint: [u8; 4],
    burn: [u8; 4],
    set_fees: [u8; 4],
    add_liquidity: [u8; 4],
    remove_liquidity: [u8; 4],
    add_blacklist: [u8; 4],
    remove_blacklist: [u8; 4],
}

// Cấu trúc cho phân tích smart contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSecurityInfo {
    pub address: Address,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub total_supply: Option<U256>,
    pub has_mint_function: bool,
    pub has_blacklist: bool,
    pub has_whitelist: bool,
    pub has_high_fees: bool,
    pub has_owner: bool,
    pub fee_percentage: Option<f64>,
    pub is_proxy: bool,
    pub creation_tx: Option<H256>,
    pub source_verified: bool,
    pub functions: Vec<String>,
    pub risk_factors: Vec<String>,
}

// Tạo trait cho token analyzer để dễ mở rộng
pub trait TokenAnalyzer {
    async fn analyze_token(&self, token_address: &str) -> Result<TokenAnalysisResult, SnipebotError>;
    async fn is_safe(&self, token_address: &str) -> Result<bool, SnipebotError>;
}

// Implement các loại analyzer khác nhau
pub struct BasicTokenAnalyzer {
    provider: Provider<Ws>,
    config: Arc<Config>,
}

pub struct WasmTokenAnalyzer {
    wasm_engine: Arc<WasmEngine>,
    basic_analyzer: BasicTokenAnalyzer,
}

pub struct MachineLearningAnalyzer {
    model: Arc<TensorflowModel>,
    basic_analyzer: BasicTokenAnalyzer,
}

// Cấu trúc dữ liệu TokenAnalyzer
pub struct TokenAnalyzer {
    provider: Provider<Ws>,
    config: Arc<Config>,
    signatures: FunctionSignatures,
    token_cache: HashMap<Address, ContractSecurityInfo>,
}

impl TokenAnalyzer {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let provider = Provider::<Ws>::connect(&config.rpc_url).await?;
        
        let signatures = FunctionSignatures {
            transfer: [0xa9, 0x05, 0x9c, 0xbb], // transfer(address,uint256)
            transfer_from: [0x23, 0xb8, 0x72, 0xdd], // transferFrom(address,address,uint256)
            approve: [0x09, 0x5e, 0xa7, 0xb3], // approve(address,uint256)
            mint: [0x40, 0xc1, 0x0f, 0x19], // mint(address,uint256)
            burn: [0x42, 0x96, 0x6c, 0x68], // burn(uint256)
            set_fees: [0x8a, 0x72, 0x34, 0xc6], // setFees(uint256)
            add_liquidity: [0xe8, 0xe3, 0x37, 0x00], // addLiquidity(...)
            remove_liquidity: [0xba, 0xf1, 0xa5, 0x71], // removeLiquidity(...)
            add_blacklist: [0x0c, 0x53, 0xc5, 0x1c], // addToBlacklist(address)
            remove_blacklist: [0x1a, 0x76, 0x97, 0x95], // removeFromBlacklist(address)
        };
        
        Ok(Self {
            provider,
            config,
            signatures,
            token_cache: HashMap::new(),
        })
    }
    
    // Phân tích token dựa trên địa chỉ
    pub async fn analyze_token(&mut self, token_address: &str) -> Result<TokenAnalysisResult> {
        let address = Address::from_str(token_address)
            .map_err(|e| anyhow!("Địa chỉ token không hợp lệ: {}", e))?;
            
        debug!(token = %token_address, "Phân tích token");
        
        // Kiểm tra cache trước
        if let Some(cached_info) = self.token_cache.get(&address) {
            debug!(token = %token_address, "Sử dụng dữ liệu token từ cache");
            return self.convert_to_analysis_result(cached_info.clone());
        }
        
        // Lấy bytecode của contract
        let code = self.provider.get_code(address, None).await?;
        
        if code.0.is_empty() {
            return Err(anyhow!("Không tìm thấy contract tại địa chỉ: {}", token_address));
        }
        
        // Phân tích bytecode để tìm các function signature
        let mut security_info = ContractSecurityInfo {
            address,
            name: None,
            symbol: None,
            decimals: None,
            total_supply: None,
            has_mint_function: false,
            has_blacklist: false,
            has_whitelist: false,
            has_high_fees: false,
            has_owner: false,
            fee_percentage: None,
            is_proxy: false,
            creation_tx: None,
            source_verified: false,
            functions: Vec::new(),
            risk_factors: Vec::new(),
        };
        
        // Mã hoá bytecode để tìm các function signature
        self.analyze_bytecode(&code, &mut security_info);
        
        // Tạo contract interface để lấy thông tin cơ bản
        self.get_token_metadata(&address, &mut security_info).await?;
        
        // Kiểm tra các rủi ro
        self.check_for_risks(&mut security_info);
        
        // Lưu vào cache
        self.token_cache.insert(address, security_info.clone());
        
        // Chuyển đổi sang kết quả phân tích
        self.convert_to_analysis_result(security_info)
    }
    
    // Phân tích bytecode để tìm function signatures
    fn analyze_bytecode(&self, code: &Bytes, info: &mut ContractSecurityInfo) {
        let bytecode = &code.0;
        
        // Kiểm tra xem bytecode có chứa các function signature cụ thể không
        if bytecode.windows(4).any(|window| window == self.signatures.mint) {
            info.has_mint_function = true;
            info.functions.push("mint".to_string());
        }
        
        if bytecode.windows(4).any(|window| window == self.signatures.add_blacklist) {
            info.has_blacklist = true;
            info.functions.push("addToBlacklist".to_string());
        }
        
        // Kiểm tra delegate call (proxy contract)
        if bytecode.windows(2).any(|window| window == [0xf4, 0x5f]) { // DELEGATECALL opcode
            info.is_proxy = true;
            info.risk_factors.push("Contract là proxy, có thể cập nhật logic".to_string());
        }
        
        // Phân tích thêm các pattern nguy hiểm khác...
    }
    
    // Lấy thông tin metadata của token
    async fn get_token_metadata(&self, address: &Address, info: &mut ContractSecurityInfo) -> Result<()> {
        // Tạo ABI tối thiểu cho ERC20
        let abi = r#"[
            {"constant":true,"inputs":[],"name":"name","outputs":[{"name":"","type":"string"}],"type":"function"},
            {"constant":true,"inputs":[],"name":"symbol","outputs":[{"name":"","type":"string"}],"type":"function"},
            {"constant":true,"inputs":[],"name":"decimals","outputs":[{"name":"","type":"uint8"}],"type":"function"},
            {"constant":true,"inputs":[],"name":"totalSupply","outputs":[{"name":"","type":"uint256"}],"type":"function"},
            {"constant":true,"inputs":[],"name":"owner","outputs":[{"name":"","type":"address"}],"type":"function"}
        ]"#;
        
        let contract = Contract::new(
            *address,
            serde_json::from_str(abi)?,
            Arc::new(self.provider.clone()),
        );
        
        // Lấy thông tin cơ bản
        let name: Result<String, ContractError> = contract.method("name", ())?.call().await;
        if let Ok(value) = name {
            info.name = Some(value);
        }
        
        let symbol: Result<String, ContractError> = contract.method("symbol", ())?.call().await;
        if let Ok(value) = symbol {
            info.symbol = Some(value);
        }
        
        let decimals: Result<u8, ContractError> = contract.method("decimals", ())?.call().await;
        if let Ok(value) = decimals {
            info.decimals = Some(value);
        }
        
        let total_supply: Result<U256, ContractError> = contract.method("totalSupply", ())?.call().await;
        if let Ok(value) = total_supply {
            info.total_supply = Some(value);
        }
        
        // Kiểm tra xem contract có owner không
        let owner: Result<Address, ContractError> = contract.method("owner", ())?.call().await;
        if let Ok(_) = owner {
            info.has_owner = true;
        }
        
        Ok(())
    }
    
    // Kiểm tra các rủi ro trong contract
    fn check_for_risks(&self, info: &mut ContractSecurityInfo) {
        // Kiểm tra các function nguy hiểm
        if info.has_mint_function {
            info.risk_factors.push("Token có thể được mint bởi owner".to_string());
        }
        
        if info.has_blacklist {
            info.risk_factors.push("Token có chức năng blacklist địa chỉ".to_string());
        }
        
        // Kiểm tra proxy contract
        if info.is_proxy {
            info.risk_factors.push("Contract là proxy, logic có thể thay đổi".to_string());
        }
        
        // Kiểm tra các rủi ro khác...
    }
    
    // Chuyển đổi dữ liệu phân tích sang kết quả
    fn convert_to_analysis_result(&self, info: ContractSecurityInfo) -> Result<TokenAnalysisResult> {
        let mut risk_score = 0u8;
        
        // Tính toán risk score dựa trên số lượng risk factors
        if !info.risk_factors.is_empty() {
            let base_risk = 20;
            let factor_risk = info.risk_factors.len() as u8 * 10;
            risk_score = (base_risk + factor_risk).min(100);
        }
        
        Ok(TokenAnalysisResult {
            address: format!("{:?}", info.address),
            is_honeypot: risk_score > 80,
            is_mintable: info.has_mint_function,
            has_blacklist: info.has_blacklist,
            has_whitelist: info.has_blacklist, // Giả định tương tự
            has_trading_cooldown: info.risk_factors.iter().any(|s| s.contains("cooldown")),
            has_anti_whale: info.risk_factors.iter().any(|s| s.contains("whale")),
            has_high_fee: info.has_high_fees,
            risk_score,
            notes: info.risk_factors,
        })
    }
}

// Khởi tạo Token Analyzer global
pub async fn init_token_analyzer(config: Arc<Config>) -> Result<()> {
    let analyzer = TokenAnalyzer::new(config).await?;
    let analyzer = Arc::new(Mutex::new(analyzer));
    
    if TOKEN_ANALYZER.set(analyzer).is_err() {
        error!("Token Analyzer đã được khởi tạo trước đó");
    }
    
    info!("Token Analyzer đã được khởi tạo thành công");
    Ok(())
}

// Phân tích token từ địa chỉ
pub async fn analyze_token(token_address: &str) -> Result<TokenAnalysisResult> {
    let analyzer = TOKEN_ANALYZER.get()
        .ok_or_else(|| anyhow!("Token Analyzer chưa được khởi tạo"))?;
    
    let mut analyzer = analyzer.lock().await;
    
    analyzer.analyze_token(token_address).await
}

// Kiểm tra nhanh một token có an toàn không
pub async fn is_token_safe(token_address: &str) -> Result<bool> {
    let analysis = analyze_token(token_address).await?;
    
    // Token được coi là an toàn nếu risk score < 50
    Ok(analysis.risk_score < 50)
}

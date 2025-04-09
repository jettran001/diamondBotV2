// External imports
use ethers::{
    types::{Address, H256, U256, Bytes},
    contract::{Contract, ContractFactory, ContractError},
    providers::{Provider, Http, Middleware, Ws},
    prelude::*,
};

// Standard library imports
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH, Instant},
    str::FromStr,
};

// Internal imports
use crate::{
    types::TradeConfig,
    chain_adapters::ChainAdapterEnum,
    utils,
    abi_utils,
    config::Config,
};

// Third party imports
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use tracing::{info, warn, error, debug};
use once_cell::sync::OnceCell;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};

/// Định nghĩa các function signature phổ biến để phân tích
#[derive(Debug, Clone)]
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

impl Default for FunctionSignatures {
    fn default() -> Self {
        Self {
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
        }
    }
}

/// Cấu trúc cơ sở cho phân tích rủi ro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAnalysis {
    /// Điểm rủi ro tổng cộng (0-100)
    pub risk_score: f64,
    /// Danh sách các yếu tố rủi ro
    pub risk_factors: Vec<RiskFactor>,
    /// Thời gian thực hiện phân tích
    pub timestamp: u64,
}

/// Cấu trúc yếu tố rủi ro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    /// Tên yếu tố rủi ro
    pub name: String,
    /// Điểm rủi ro (0-10)
    pub score: f64,
    /// Mô tả chi tiết
    pub description: String,
}

/// Vấn đề token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenIssue {
    /// Mã vấn đề
    pub code: String,
    /// Mức độ vấn đề (critical, high, medium, low)
    pub severity: String,
    /// Mô tả vấn đề
    pub description: String,
}

/// Kết quả phân tích token đơn giản (từ module analysis cũ)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAnalysisResult {
    /// Địa chỉ token
    pub address: String,
    /// Có phải honeypot
    pub is_honeypot: bool,
    /// Có thể mint
    pub is_mintable: bool,
    /// Có blacklist
    pub has_blacklist: bool,
    /// Có whitelist
    pub has_whitelist: bool,
    /// Có trading cooldown
    pub has_trading_cooldown: bool,
    /// Có hạn chế whale
    pub has_anti_whale: bool,
    /// Có phí cao bất thường
    pub has_high_fee: bool,
    /// Điểm rủi ro (0-100)
    pub risk_score: u8,
    /// Ghi chú
    pub notes: Vec<String>,
}

/// Kết quả phân tích giao dịch đơn giản (từ module analysis cũ)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionAnalysisResult {
    /// Hash giao dịch
    pub transaction_hash: String,
    /// Có phải swap
    pub is_swap: bool,
    /// Địa chỉ token
    pub token_address: Option<String>,
    /// Giá trị USD
    pub value_usd: Option<f64>,
    /// Method ID
    pub method_id: String,
    /// Tên phương thức
    pub method_name: Option<String>,
    /// Gas price
    pub gas_price: u64,
    /// Độ ưu tiên (0-10)
    pub priority: u8,
}

/// Cấu trúc cho phân tích smart contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSecurityInfo {
    /// Địa chỉ hợp đồng
    pub address: Address,
    /// Tên token
    pub name: Option<String>,
    /// Ký hiệu token
    pub symbol: Option<String>,
    /// Số thập phân
    pub decimals: Option<u8>,
    /// Tổng cung
    pub total_supply: Option<U256>,
    /// Có hàm mint
    pub has_mint_function: bool,
    /// Có blacklist
    pub has_blacklist: bool,
    /// Có whitelist
    pub has_whitelist: bool,
    /// Có phí cao
    pub has_high_fees: bool,
    /// Có owner
    pub has_owner: bool,
    /// Phần trăm phí
    pub fee_percentage: Option<f64>,
    /// Là proxy
    pub is_proxy: bool,
    /// TX tạo hợp đồng
    pub creation_tx: Option<H256>,
    /// Có source code
    pub source_verified: bool,
    /// Danh sách hàm
    pub functions: Vec<String>,
    /// Yếu tố rủi ro
    pub risk_factors: Vec<String>,
}

/// Trait cho các phân tích rủi ro
#[async_trait]
pub trait RiskAnalyzer: Send + Sync + 'static {
    /// Phân tích rủi ro token
    async fn analyze_token(&self, token: Address) -> Result<TokenRiskAnalysis>;
    /// Phân tích rủi ro giao dịch
    async fn analyze_transaction(&self, tx: Vec<u8>) -> Result<TransactionRiskAnalysis>;
    /// Phân tích rủi ro hợp đồng
    async fn analyze_contract(&self, contract: Address) -> Result<ContractRiskAnalysis>;
}

/// Trait cho phân tích token (từ module analysis cũ)
#[async_trait]
pub trait TokenAnalyzer: Send + Sync + 'static {
    /// Phân tích token
    async fn analyze_token(&self, token_address: &str) -> Result<TokenAnalysisResult>;
    /// Kiểm tra token có an toàn không
    async fn is_safe(&self, token_address: &str) -> Result<bool>;
}

/// Phân tích rủi ro token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRiskAnalysis {
    /// Thông tin phân tích rủi ro cơ bản
    #[serde(flatten)]
    pub base: RiskAnalysis,
    /// Địa chỉ token
    pub token: Address,
    /// Symbol token
    pub symbol: String,
    /// Tên token
    pub name: String,
    /// Tổng số vấn đề
    pub total_issues: u32,
    /// Số vấn đề nghiêm trọng
    pub critical_issues: u32,
    /// Số vấn đề cao 
    pub high_issues: u32,
    /// Số vấn đề trung bình
    pub medium_issues: u32,
    /// Số vấn đề thấp
    pub low_issues: u32,
    /// Danh sách vấn đề
    pub issues: Vec<TokenIssue>,
    /// Danh sách rủi ro
    pub risks: Vec<String>,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Có được xác minh trên Etherscan không
    pub is_verified: bool,
    /// Tỷ lệ thanh khoản
    pub liquidity_ratio: f64,
    /// Số lượng holder
    pub holder_count: u64,
    /// Vấn đề về quyền sở hữu
    pub ownership_issues: Vec<String>,
    /// Các hàm nguy hiểm
    pub dangerous_functions: Vec<String>,
}

/// Loại vấn đề token
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueType {
    /// Token chưa được xác minh
    ContractNotVerified,
    /// Rủi ro rug pull
    RugPullRisk,
    /// Token có thể mint tùy ý
    UnlimitedMint,
    /// Token có khả năng tạm dừng giao dịch
    TradingCanBePaused,
    /// Vấn đề về owner
    OwnershipIssue,
    /// Vấn đề về thanh khoản
    LiquidityIssue,
    /// Token có thể bị khóa
    TokenCanBeLocked,
    /// Phí giao dịch cao bất thường
    HighTransactionFee,
    /// Khác
    Other,
}

/// Mức độ nghiêm trọng của vấn đề
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueSeverity {
    /// Nghiêm trọng
    Critical,
    /// Cao
    High, 
    /// Trung bình
    Medium,
    /// Thấp
    Low,
}

/// Phân tích rủi ro giao dịch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRiskAnalysis {
    /// Thông tin phân tích rủi ro cơ bản
    #[serde(flatten)]
    pub base: RiskAnalysis,
    /// Hash giao dịch
    pub tx_hash: H256,
    /// Địa chỉ người gửi
    pub sender: Address,
    /// Địa chỉ người nhận
    pub recipient: Address,
    /// Giá trị giao dịch
    pub value: U256,
    /// Gas price
    pub gas_price: U256,
    /// Gas limit
    pub gas_limit: U256,
    /// Dữ liệu giao dịch
    pub data: Vec<u8>,
    /// Thời gian phân tích
    pub created_at: SystemTime,
}

/// Phân tích rủi ro hợp đồng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractRiskAnalysis {
    /// Thông tin phân tích rủi ro cơ bản
    #[serde(flatten)]
    pub base: RiskAnalysis,
    /// Địa chỉ hợp đồng
    pub contract: Address,
    /// Tên hợp đồng
    pub name: String,
    /// Hợp đồng được xác minh hay không
    pub verified: bool,
    /// Danh sách hàm nguy hiểm
    pub dangerous_functions: Vec<String>,
    /// Danh sách hàm không thể được gọi bởi EOA
    pub blocked_for_eoa: Vec<String>,
    /// Thông tin về quyền sở hữu hợp đồng
    pub ownership_info: HashMap<String, String>,
    /// Thời gian tạo
    pub created_at: SystemTime,
}

/// Kịch bản stress test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressTestScenario {
    /// Tên kịch bản
    pub name: String,
    /// Mô tả kịch bản
    pub description: String,
    /// Thời gian thực hiện
    pub timestamp: u64,
    /// Tác động đến tài sản
    pub asset_impacts: Vec<ScenarioAssetImpact>,
    /// Tác động tổng thể đến danh mục
    pub overall_portfolio_impact: f64,
}

/// Kết quả thực hiện stress test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressTestResults {
    /// Danh sách kịch bản đã thực hiện
    pub scenarios: Vec<StressTestScenario>,
    /// Thời gian tạo kết quả
    pub created_at: u64,
}

/// Tác động của kịch bản đến tài sản
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioAssetImpact {
    /// Tên tài sản
    pub asset: String,
    /// Giá trị ban đầu
    pub initial_value: f64,
    /// Giá trị cuối cùng
    pub final_value: f64,
    /// Phần trăm thay đổi
    pub percent_change: f64,
}

/// Cấu hình risk analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// ID config
    pub config_id: String,
    /// Tên config
    pub name: String,
    /// Phiên bản
    pub version: String,
    /// Thời gian tạo
    pub created_at: SystemTime,
    /// Tham số max_allocation_per_token
    pub max_allocation_per_token: f64,
    /// Tham số max_tokens_in_portfolio
    pub max_tokens_in_portfolio: u32,
    /// Chế độ offline
    pub offline_mode: bool,
}

/// Triển khai RiskAnalyzer cơ bản
#[derive(Debug, Clone)]
pub struct BasicRiskAnalyzer<P: Provider + 'static> {
    /// Cấu hình risk analyzer
    config: RiskConfig,
    /// Provider
    provider: Arc<P>,
    /// Cache phân tích
    analysis_cache: Arc<RwLock<HashMap<Address, TokenRiskAnalysis>>>,
    /// Cache hợp đồng
    contract_cache: Arc<RwLock<HashMap<Address, ContractRiskAnalysis>>>,
    /// Function signatures
    signatures: FunctionSignatures,
    /// Cache an toàn cho token
    safety_cache: Arc<RwLock<HashMap<Address, bool>>>,
}

impl<P: Provider + 'static> BasicRiskAnalyzer<P> {
    /// Tạo RiskAnalyzer mới
    pub fn new(provider: Arc<P>, config: RiskConfig) -> Self {
        Self {
            config,
            provider,
            analysis_cache: Arc::new(RwLock::new(HashMap::new())),
            contract_cache: Arc::new(RwLock::new(HashMap::new())),
            signatures: FunctionSignatures::default(),
            safety_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Kiểm tra bytecode để phát hiện các function signature nguy hiểm
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
    }

    /// Phân tích token dựa trên địa chỉ string (hỗ trợ cho TokenAnalyzer interface)
    pub async fn analyze_token_by_address(&self, token_address: &str) -> Result<TokenAnalysisResult> {
        let address = Address::from_str(token_address)
            .map_err(|e| anyhow!("Địa chỉ token không hợp lệ: {}", e))?;
            
        debug!(token = %token_address, "Phân tích token");
        
        // Kiểm tra cache trước
        {
            let cache = self.safety_cache.read().unwrap();
            if let Some(&is_safe) = cache.get(&address) {
                debug!(token = %token_address, "Sử dụng dữ liệu an toàn từ cache");
                let risk_score = if is_safe { 0 } else { 100 };
                return Ok(TokenAnalysisResult {
                    address: token_address.to_string(),
                    is_honeypot: !is_safe,
                    is_mintable: false,
                    has_blacklist: false,
                    has_whitelist: false,
                    has_trading_cooldown: false,
                    has_anti_whale: false,
                    has_high_fee: false,
                    risk_score: risk_score as u8,
                    notes: Vec::new(),
                });
            }
        }
        
        // Lấy thông tin chi tiết
        let token_analysis = match self.analyze_token(address).await {
            Ok(analysis) => analysis,
            Err(e) => {
                error!(token = %token_address, error = %e, "Lỗi khi phân tích token");
                return Err(anyhow!("Lỗi khi phân tích token: {}", e));
            }
        };
        
        // Chuyển đổi từ TokenRiskAnalysis sang TokenAnalysisResult
        let is_safe = token_analysis.base.risk_score < 50.0;
        
        // Cập nhật cache
        {
            let mut cache = self.safety_cache.write().unwrap();
            cache.insert(address, is_safe);
        }
        
        Ok(TokenAnalysisResult {
            address: token_address.to_string(),
            is_honeypot: token_analysis.base.risk_score > 80.0,
            is_mintable: token_analysis.dangerous_functions.contains(&"mint".to_string()),
            has_blacklist: token_analysis.dangerous_functions.contains(&"blacklist".to_string()),
            has_whitelist: token_analysis.dangerous_functions.contains(&"whitelist".to_string()),
            has_trading_cooldown: token_analysis.risks.iter().any(|r| r.contains("cooldown")),
            has_anti_whale: token_analysis.risks.iter().any(|r| r.contains("anti-whale")),
            has_high_fee: token_analysis.risks.iter().any(|r| r.contains("high fee")),
            risk_score: token_analysis.base.risk_score as u8,
            notes: token_analysis.risks,
        })
    }
    
    /// Kiểm tra token có an toàn không (hỗ trợ cho TokenAnalyzer interface)
    pub async fn is_token_safe(&self, token_address: &str) -> Result<bool> {
        let address = Address::from_str(token_address)
            .map_err(|e| anyhow!("Địa chỉ token không hợp lệ: {}", e))?;
            
        // Kiểm tra cache trước
        {
            let cache = self.safety_cache.read().unwrap();
            if let Some(&is_safe) = cache.get(&address) {
                return Ok(is_safe);
            }
        }
        
        // Phân tích token
        let analysis = self.analyze_token_by_address(token_address).await?;
        let is_safe = analysis.risk_score < 50;
        
        // Cập nhật cache
        {
            let mut cache = self.safety_cache.write().unwrap();
            cache.insert(address, is_safe);
        }
        
        Ok(is_safe)
    }
}

#[async_trait]
impl<P: Provider + 'static> RiskAnalyzer for BasicRiskAnalyzer<P> {
    async fn analyze_token(&self, token: Address) -> Result<TokenRiskAnalysis> {
        info!("Đang phân tích rủi ro cho token: {}", token);
        
        if self.config.offline_mode {
            // Trong chế độ offline, trả về phân tích cơ bản
            let mut analysis = TokenRiskAnalysis {
                base: RiskAnalysis::new(),
                token,
                symbol: "UNKNOWN".to_string(),
                name: "Unknown Token".to_string(),
                total_issues: 1,
                critical_issues: 0,
                high_issues: 1,
                medium_issues: 0,
                low_issues: 0,
                issues: Vec::new(),
                risks: Vec::new(),
                created_at: SystemTime::now(),
                is_verified: false,
                liquidity_ratio: 0.0,
                holder_count: 0,
                ownership_issues: Vec::new(),
                dangerous_functions: Vec::new(),
            };
            
            analysis.base.risk_score = 50.0; // Mặc định rủi ro trung bình khi không có dữ liệu
            analysis.issues.push(TokenIssue {
                code: "C001".to_string(),
                severity: "High".to_string(),
                description: "Không thể phân tích token do thiếu kết nối blockchain".to_string(),
            });
            analysis.risks.push("Không thể phân tích do thiếu kết nối blockchain".to_string());
            
            return Ok(analysis);
        }
        
        // TODO: Implement online analysis
        let analysis = TokenRiskAnalysis {
            base: RiskAnalysis {
                risk_score: 50.0,
                risk_factors: vec![
                    RiskFactor {
                        name: "Test Risk".to_string(),
                        score: 5.0,
                        description: "This is a test risk factor".to_string(),
                    }
                ],
                timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            },
            token,
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            total_issues: 1,
            critical_issues: 0,
            high_issues: 1,
            medium_issues: 0,
            low_issues: 0,
            issues: vec![
                TokenIssue {
                    code: "R001".to_string(),
                    severity: "High".to_string(),
                    description: "Test".to_string(),
                }
            ],
            risks: vec!["Test risk".to_string()],
            created_at: SystemTime::now(),
            is_verified: false,
            liquidity_ratio: 0.0,
            holder_count: 0,
            ownership_issues: Vec::new(),
            dangerous_functions: Vec::new(),
        };
        
        Ok(analysis)
    }
    
    async fn analyze_transaction(&self, tx: Vec<u8>) -> Result<TransactionRiskAnalysis> {
        // TODO: Implement transaction risk analysis
        let risk_analysis = RiskAnalysis::new();
        let analysis = TransactionRiskAnalysis {
            base: risk_analysis,
            tx_hash: H256::zero(), // Placeholder
            sender: Address::zero(),
            recipient: Address::zero(),
            value: U256::zero(),
            gas_price: U256::zero(),
            gas_limit: U256::zero(),
            data: Vec::new(),
            created_at: SystemTime::now(),
        };
        
        Ok(analysis)
    }
    
    async fn analyze_contract(&self, contract: Address) -> Result<ContractRiskAnalysis> {
        let risk_analysis = RiskAnalysis::new();
        let analysis = ContractRiskAnalysis {
            base: risk_analysis,
            contract,
            name: "Unknown Contract".to_string(),
            verified: false,
            dangerous_functions: Vec::new(),
            blocked_for_eoa: Vec::new(),
            ownership_info: HashMap::new(),
            created_at: SystemTime::now(),
        };
        
        Ok(analysis)
    }
}

/// Triển khai RiskAnalyzer nâng cao
#[derive(Debug, Clone)]
pub struct AdvancedRiskAnalyzer {
    /// BasicRiskAnalyzer nền tảng
    basic_analyzer: BasicRiskAnalyzer<Http>,
    /// Bật/tắt stress test
    stress_test_enabled: bool,
}

impl AdvancedRiskAnalyzer {
    /// Tạo advanced analyzer mới
    pub fn new(config: RiskConfig) -> Self {
        Self {
            basic_analyzer: BasicRiskAnalyzer::new(Arc::new(Http), config),
            stress_test_enabled: false,
        }
    }
    
    /// Tạo advanced analyzer với adapter
    pub fn new_with_adapter(
        config: RiskConfig,
        adapter: ChainAdapterEnum,
        weth_address: &str
    ) -> Result<Self> {
        Ok(Self {
            basic_analyzer: BasicRiskAnalyzer::new_with_adapter(Arc::new(Http), config, adapter, weth_address)?,
            stress_test_enabled: false,
        })
    }
    
    /// Bật stress test
    pub fn enable_stress_test(&mut self) {
        self.stress_test_enabled = true;
    }
    
    /// Chạy stress test
    pub async fn run_stress_test(&self, token: Address) -> Result<StressTestResults> {
        // TODO: Implement stress test
        info!("Đang chạy stress test cho token: {}", token);
        
        // Khởi tạo kết quả
        let mut results = StressTestResults {
            scenarios: Vec::new(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs(),
        };
        
        // Thêm kịch bản đơn giản
        let scenario = StressTestScenario {
            name: "Kịch bản đơn giản".to_string(),
            description: "Kiểm tra ảnh hưởng của biến động thị trường".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs(),
            asset_impacts: vec![
                ScenarioAssetImpact {
                    asset: format!("{:?}", token),
                    initial_value: 100.0,
                    final_value: 80.0,
                    percent_change: -20.0,
                }
            ],
            overall_portfolio_impact: -10.0,
        };
        
        results.scenarios.push(scenario);
        
        Ok(results)
    }
}

#[async_trait]
impl RiskAnalyzer for AdvancedRiskAnalyzer {
    async fn analyze_token(&self, token: Address) -> Result<TokenRiskAnalysis> {
        // Dùng basic analyzer cơ bản
        let mut analysis = self.basic_analyzer.analyze_token(token).await?;
        
        // Thêm phân tích nâng cao
        if self.stress_test_enabled {
            // Chạy stress test và cập nhật phân tích
            // TODO: Implement stress test result integration
            analysis.risks.push("Đã thực hiện stress test".to_string());
        }
        
        Ok(analysis)
    }
    
    async fn analyze_transaction(&self, tx: Vec<u8>) -> Result<TransactionRiskAnalysis> {
        self.basic_analyzer.analyze_transaction(tx).await
    }
    
    async fn analyze_contract(&self, contract: Address) -> Result<ContractRiskAnalysis> {
        self.basic_analyzer.analyze_contract(contract).await
    }
}

#[async_trait]
impl<P: Provider + 'static> TokenAnalyzer for BasicRiskAnalyzer<P> {
    async fn analyze_token(&self, token_address: &str) -> Result<TokenAnalysisResult> {
        self.analyze_token_by_address(token_address).await
    }
    
    async fn is_safe(&self, token_address: &str) -> Result<bool> {
        self.is_token_safe(token_address).await
    }
}

impl RiskAnalysis {
    /// Tạo phân tích rủi ro mới
    pub fn new() -> Self {
        RiskAnalysis {
            risk_score: 0.0,
            risk_factors: Vec::new(),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }
    }
}

/// Singleton pattern để lấy RiskAnalyzer instance trong ứng dụng
static RISK_ANALYZER: OnceCell<Arc<Mutex<dyn RiskAnalyzer + Send + Sync>>> = OnceCell::new();

/// Khởi tạo hệ thống phân tích rủi ro
pub async fn init_risk_analyzer(config: Arc<Config>) -> Result<()> {
    if RISK_ANALYZER.get().is_some() {
        return Ok(()); // Đã khởi tạo
    }
    
    let provider = Provider::<Http>::try_from(&config.rpc_url)?;
    
    let risk_config = RiskConfig {
        config_id: "default".to_string(),
        name: "Standard Risk Analysis".to_string(),
        version: "1.0.0".to_string(),
        created_at: SystemTime::now(),
        max_allocation_per_token: 0.1, // 10%
        max_tokens_in_portfolio: 20,
        offline_mode: false,
    };
    
    let analyzer = BasicRiskAnalyzer::new(Arc::new(provider), risk_config);
    
    RISK_ANALYZER.set(Arc::new(Mutex::new(analyzer)))
        .map_err(|_| anyhow!("Không thể set RiskAnalyzer global"))?;
    
    info!("Khởi tạo RiskAnalyzer thành công");
    Ok(())
}

/// Lấy RiskAnalyzer instance
pub async fn get_risk_analyzer() -> Result<Arc<Mutex<dyn RiskAnalyzer + Send + Sync>>> {
    RISK_ANALYZER.get()
        .cloned()
        .ok_or_else(|| anyhow!("RiskAnalyzer chưa được khởi tạo"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    
    #[test]
    fn test_token_risk_analysis() {
        let analysis = TokenRiskAnalysis {
            token: Address::zero(),
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            risk_score: 75.0,
            total_issues: 3,
            critical_issues: 1,
            high_issues: 1,
            medium_issues: 1,
            low_issues: 0,
            issues: vec![
                TokenIssue {
                    code: "R001".to_string(),
                    severity: "High".to_string(),
                    description: "Test".to_string(),
                }
            ],
            risks: vec!["Test risk".to_string()],
            created_at: SystemTime::now(),
            is_verified: false,
            liquidity_ratio: 0.0,
            holder_count: 0,
            ownership_issues: Vec::new(),
            dangerous_functions: Vec::new(),
        };
        
        assert_eq!(analysis.token, Address::zero());
        assert_eq!(analysis.symbol, "TEST");
        assert_eq!(analysis.risk_score, 75.0);
        assert_eq!(analysis.total_issues, 3);
    }
    
    #[test]
    fn test_transaction_risk_analysis() {
        let base = RiskAnalysis {
            risk_score: 50.0,
            risk_factors: vec![RiskFactor {
                name: "Test risk".to_string(),
                score: 5.0,
                description: "Test description".to_string(),
            }],
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        };
        
        let analysis = TransactionRiskAnalysis {
            base,
            tx_hash: H256::zero(),
            sender: Address::zero(),
            recipient: Address::zero(),
            value: U256::zero(),
            gas_price: U256::zero(),
            gas_limit: U256::zero(),
            data: Vec::new(),
            created_at: SystemTime::now(),
        };
        
        assert_eq!(analysis.tx_hash, H256::zero());
        assert_eq!(analysis.base.risk_score, 50.0);
        assert_eq!(analysis.base.risk_factors.len(), 1);
    }
    
    #[test]
    fn test_contract_risk_analysis() {
        let base = RiskAnalysis {
            risk_score: 25.0,
            risk_factors: vec![RiskFactor {
                name: "Test risk".to_string(),
                score: 2.5,
                description: "Test description".to_string(),
            }],
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        };
        
        let analysis = ContractRiskAnalysis {
            base,
            contract: Address::zero(),
            name: "Test Contract".to_string(),
            verified: false,
            dangerous_functions: vec!["Test danger".to_string()],
            blocked_for_eoa: Vec::new(),
            ownership_info: HashMap::new(),
            created_at: SystemTime::now(),
        };
        
        assert_eq!(analysis.contract, Address::zero());
        assert_eq!(analysis.base.risk_score, 25.0);
        assert_eq!(analysis.name, "Test Contract");
        assert_eq!(analysis.dangerous_functions.len(), 1);
    }
    
    #[test]
    fn test_risk_config() {
        let config = RiskConfig {
            config_id: "test".to_string(),
            name: "Test Config".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            max_allocation_per_token: 0.1,
            max_tokens_in_portfolio: 20,
        };
        
        assert_eq!(config.config_id, "test");
        assert_eq!(config.name, "Test Config");
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.max_allocation_per_token, 0.1);
        assert_eq!(config.max_tokens_in_portfolio, 20);
    }
    
    #[test]
    fn test_basic_risk_analyzer() {
        let config = RiskConfig {
            config_id: "test".to_string(),
            name: "Test Config".to_string(),
            version: "1.0.0".to_string(),
            created_at: SystemTime::now(),
            max_allocation_per_token: 0.1,
            max_tokens_in_portfolio: 20,
        };
        
        let analyzer = BasicRiskAnalyzer::new(Arc::new(Http), config);
        assert!(analyzer.provider.is_none());
        assert!(analyzer.token_abi.is_none());
    }
}

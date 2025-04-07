use wasm_bindgen::prelude::*;
use serde::{Serialize, Deserialize};

// Input structure cho transaction analytics
#[derive(Serialize, Deserialize)]
pub struct TransactionInput {
    pub method_id: String,
    pub from_address: String,
    pub to_address: String,
    pub value: String,
    pub gas_price: String,
    pub transaction_data: String,
    pub token_address: Option<String>,
    pub chain_id: u64,
}

// Output structure cho transaction analytics
#[wasm_bindgen]
#[derive(Debug, Clone, Serialize)]
struct TransactionOutput {
    is_valid: bool,
    risk_score: u32,
    description: String,
    warnings: Vec<String>,
    contract_analysis: Option<ContractAnalysis>,
}

// Contract analysis
#[wasm_bindgen]
#[derive(Debug, Clone, Serialize)]
struct ContractAnalysis {
    is_verified: bool,
    is_proxy: bool,
    has_known_vulnerabilities: bool,
    creator_address: String,
    creation_date: String,
    risk_factors: Vec<String>,
}

// Hàm chính để phân tích transaction - sử dụng wasm_bindgen thay vì unsafe
#[wasm_bindgen]
pub fn analyze_transaction_js(input_json: &str) -> String {
    // Phân tích JSON input
    let input: TransactionInput = match serde_json::from_str(input_json) {
        Ok(input) => input,
        Err(_) => {
            // Trả về kết quả lỗi dưới dạng JSON
            return create_error_json("Lỗi parse JSON");
        }
    };
    
    // Phân tích transaction
    let result = analyze_tx_data(&input);
    
    // Serialize kết quả thành JSON
    match serde_json::to_string(&result) {
        Ok(json) => json,
        Err(_) => create_error_json("Lỗi serialize kết quả"),
    }
}

// Tạo kết quả lỗi dưới dạng JSON string - cách tiếp cận an toàn hơn
fn create_error_json(message: &str) -> String {
    let result = TransactionOutput {
        is_valid: false,
        risk_score: 0,
        description: message.to_string(),
        warnings: vec![message.to_string()],
        contract_analysis: None,
    };
    
    serde_json::to_string(&result).unwrap_or_else(|_| 
        String::from("{\"is_valid\":false,\"risk_score\":0,\"description\":\"Lỗi nội bộ\",\"warnings\":[\"Lỗi nội bộ\"],\"contract_analysis\":null}")
    )
}

// Phân tích dữ liệu transaction
fn analyze_tx_data(input: &TransactionInput) -> TransactionOutput {
    let mut risk_factors = Vec::new();
    let mut confidence = 0u8;
    
    // Phân tích method ID
    analyze_method_id(&input.method_id, &mut risk_factors, &mut confidence);
    
    // Phân tích transaction data
    if input.transaction_data.len() > 10 {
        analyze_transaction_data(&input.transaction_data, &mut risk_factors, &mut confidence);
    }
    
    // Phân tích gas price (phát hiện gas front-running)
    if let Ok(gas_price) = input.gas_price.parse::<u64>() {
        if gas_price > 500_000_000_000u64 { // 500 gwei
            risk_factors.push("Gas price rất cao, có thể là front-running attack".to_string());
            confidence += 10;
        }
    }
    
    // Phân tích value (phát hiện scam transfers)
    if let Ok(value) = input.value.parse::<u128>() {
        if value > 1_000_000_000_000_000_000_000u128 { // > 1000 ETH
            risk_factors.push("Giá trị giao dịch cực lớn, cần xác minh kỹ".to_string());
            confidence += 5;
        }
    }
    
    // Đánh giá cuối cùng
    let _safe_to_proceed = confidence < 30;
    
    let risk_score = calculate_risk_score(input);
    let warnings = get_warnings(input);
    
    let contract_analysis = input.token_address.as_ref().map(|token_addr| analyze_contract(token_addr));
    
    TransactionOutput {
        is_valid: true,
        risk_score,
        description: create_description(input, risk_score),
        warnings,
        contract_analysis,
    }
}

// Phân tích transaction data
fn analyze_transaction_data(data: &str, risk_factors: &mut Vec<String>, confidence: &mut u8) {
    if data.len() < 10 {
        return;
    }
    
    // Kiểm tra các chuỗi nguy hiểm trong transaction data
    if data.contains("setApprovalForAll") {
        risk_factors.push("Giao dịch setApprovalForAll có thể nguy hiểm nếu người nhận không đáng tin".to_string());
        *confidence += 30;
    }
    
    if data.contains("transferOwnership") {
        risk_factors.push("Giao dịch chuyển quyền sở hữu contract, cẩn thận!".to_string());
        *confidence += 40;
    }
    
    // Kiểm tra độ phức tạp của calldata
    if data.len() > 1000 {
        risk_factors.push("Calldata rất dài và phức tạp, khó xác minh mục đích".to_string());
        *confidence += 15;
    }
}

// Phân tích method ID
fn analyze_method_id(method_id: &str, risk_factors: &mut Vec<String>, confidence: &mut u8) {
    match method_id {
        "0x23b872dd" => { // transferFrom
            // Không có vấn đề
        },
        "0xa9059cbb" => { // transfer
            // Không có vấn đề
        },
        "0x095ea7b3" => { // approve
            // Không có vấn đề
        },
        _ => {
            // Method ID không xác định
            risk_factors.push(format!("Method ID {} không phổ biến, cần kiểm tra kỹ", method_id));
            *confidence += 10;
        }
    }
}

// Import từ JS environment
#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);
    
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

// Thêm các hàm utility sử dụng wasm_bindgen
#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Xin chào, {}!", name)
}

#[wasm_bindgen]
pub fn initialize() {
    log("Mô-đun phân tích giao dịch đã được khởi tạo");
}

#[wasm_bindgen]
pub fn compute_hash(input: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[wasm_bindgen]
pub struct NetworkClient {
    id: String,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl NetworkClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        log(&format!("Tạo NetworkClient mới với ID: {}", id));
        Self { id }
    }
    
    #[wasm_bindgen]
    pub fn get_id(&self) -> String {
        self.id.clone()
    }
    
    #[wasm_bindgen]
    pub fn connect(&self, url: &str) -> Result<JsValue, JsValue> {
        log(&format!("Kết nối đến {}", url));
        Ok(JsValue::from_str("connected"))
    }
    
    pub fn send_request(&self, _data: &JsValue) -> JsValue {
        // Implement network request functionality
        let response = r#"{"status": "success", "message": "Request sent"}"#;
        JsValue::from_str(response)
    }
}

// Tính toán risk score cho giao dịch
fn calculate_risk_score(input: &TransactionInput) -> u32 {
    // Giả lập risk score
    let mut score = 0;
    
    // Kiểm tra method ID
    if input.method_id == "0xa9059cbb" {
        // ERC20 transfer
        score += 10;
    } else if input.method_id == "0x095ea7b3" {
        // ERC20 approve
        score += 50;
    } else if input.method_id == "0x23b872dd" {
        // ERC20 transferFrom
        score += 30;
    }
    
    // Thêm logic tính risk score khác ở đây
    
    score
}

// Tạo cảnh báo cho giao dịch
fn get_warnings(input: &TransactionInput) -> Vec<String> {
    let mut warnings = Vec::new();
    
    // Kiểm tra approve
    if input.method_id == "0x095ea7b3" {
        warnings.push("Giao dịch này đang approve một lượng token không giới hạn".to_string());
    }
    
    // Kiểm tra gas price quá cao
    if input.gas_price.starts_with("0x") {
        if let Ok(gas_price) = u64::from_str_radix(&input.gas_price[2..], 16) {
            if gas_price > 100_000_000_000 {
                warnings.push("Gas price quá cao".to_string());
            }
        }
    }
    
    // Thêm logic kiểm tra warning khác ở đây
    
    warnings
}

// Tạo mô tả cho giao dịch
fn create_description(input: &TransactionInput, risk_score: u32) -> String {
    match input.method_id.as_str() {
        "0xa9059cbb" => format!("ERC20 Transfer - Risk Score: {}", risk_score),
        "0x095ea7b3" => format!("ERC20 Approve - Risk Score: {}", risk_score),
        "0x23b872dd" => format!("ERC20 TransferFrom - Risk Score: {}", risk_score),
        _ => format!("Unknown Method - Risk Score: {}", risk_score),
    }
}

// Phân tích hợp đồng smart contract
fn analyze_contract(_address: &str) -> ContractAnalysis {
    // Đây là chức năng demo, trong thực tế bạn sẽ phân tích smart contract
    ContractAnalysis {
        is_verified: true,
        is_proxy: false,
        has_known_vulnerabilities: false,
        creator_address: "0x1234567890123456789012345678901234567890".to_string(),
        creation_date: "2023-01-01".to_string(),
        risk_factors: vec!["Không tìm thấy rủi ro".to_string()],
    }
}

// Phân tích giao dịch để phát hiện rủi ro
#[wasm_bindgen]
pub fn analyze_transaction(tx_data: JsValue) -> JsValue {
    log("Bắt đầu phân tích giao dịch");
    
    // Đọc dữ liệu input từ JS
    let input_str = match js_sys::JSON::stringify(&tx_data) {
        Ok(json_str) => json_str.as_string().unwrap_or_default(),
        Err(_) => {
            log("Lỗi chuyển đổi JsValue sang JSON string");
            return JsValue::NULL;
        }
    };
    
    // Parse dữ liệu
    let input: TransactionInput = match serde_json::from_str(&input_str) {
        Ok(parsed) => parsed,
        Err(e) => {
            log(&format!("Lỗi parse input: {}", e));
            return JsValue::NULL;
        }
    };
    
    // Phân tích rủi ro
    let risk_score = calculate_risk_score(&input);
    let warnings = get_warnings(&input);
    
    // Phân tích hợp đồng nếu có địa chỉ token
    let contract_analysis = input.token_address.as_ref().map(|token_addr| {
        log(&format!("Phân tích hợp đồng: {}", token_addr));
        analyze_contract(token_addr)
    });
    
    // Tạo output
    let output = TransactionOutput {
        is_valid: true,
        risk_score,
        description: create_description(&input, risk_score),
        warnings,
        contract_analysis,
    };
    
    // Chuyển output thành JsValue
    match serde_json::to_string(&output) {
        Ok(json_str) => js_sys::JSON::parse(&json_str).unwrap_or(JsValue::NULL),
        Err(e) => {
            log(&format!("Lỗi serialization: {}", e));
            JsValue::NULL
        }
    }
}

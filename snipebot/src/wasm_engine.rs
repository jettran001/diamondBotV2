use wasmer::{Store, Module, Instance, Value, Function, imports};
use wasmer_compiler_llvm::LLVM;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::path::Path;
use tracing::{info, error, debug, warn};
use once_cell::sync::OnceCell;
use serde::{Serialize, Deserialize};
use ethers::types::Transaction;

// Lưu trữ WASM runtime
static WASM_RUNTIME: OnceCell<Arc<Mutex<WasmRuntime>>> = OnceCell::new();

// Cấu trúc dữ liệu cho WASM runtime
pub struct WasmRuntime {
    store: Store,
    modules: Vec<(String, Module)>,
}

// Dữ liệu input cho WASM modules
#[derive(Serialize, Deserialize)]
pub struct WasmInput {
    pub transaction_data: String,
    pub token_address: Option<String>,
    pub method_id: String,
    pub sender: String,
    pub receiver: String,
    pub value: String,
    pub gas_price: String,
    pub gas_limit: String,
}

// Kết quả phân tích từ WASM
#[derive(Serialize, Deserialize, Debug)]
pub struct WasmAnalysisResult {
    pub is_scam: bool,
    pub confidence: u8,
    pub risk_factors: Vec<String>,
    pub safe_to_proceed: bool,
}

impl WasmRuntime {
    // Khởi tạo WASM runtime
    pub fn new() -> Result<Self> {
        let compiler = LLVM::default();
        let store = Store::new(&compiler);
        
        Ok(Self {
            store,
            modules: Vec::new(),
        })
    }
    
    // Tải module WASM từ đường dẫn
    pub fn load_module(&mut self, name: &str, path: &Path) -> Result<()> {
        debug!(path = %path.display(), "Đang tải WASM module");
        
        let wasm_bytes = std::fs::read(path)?;
        let module = Module::new(&self.store, wasm_bytes)?;
        
        self.modules.push((name.to_string(), module));
        info!(name = name, "Đã tải WASM module thành công");
        
        Ok(())
    }
    
    // Chạy module WASM với dữ liệu input
    pub fn execute_analysis(&self, module_name: &str, input: &WasmInput) -> Result<WasmAnalysisResult> {
        let module = self.modules.iter()
            .find(|(name, _)| name == module_name)
            .map(|(_, module)| module)
            .ok_or_else(|| anyhow!("Không tìm thấy module: {}", module_name))?;
        
        // Chuyển đổi input thành JSON
        let input_json = serde_json::to_string(input)?;
        
        // Tạo import object
        let import_object = imports! {};
        
        // Khởi tạo instance từ module
        let instance = Instance::new(module, &import_object)?;
        
        // Lấy memory export
        let memory = instance.exports.get_memory("memory")?;
        
        // Lấy các hàm exports
        let alloc = instance.exports.get_function("alloc")?;
        let analyze = instance.exports.get_function("analyze_transaction")?;
        let dealloc = instance.exports.get_function("dealloc")?;
        
        // Cấp phát memory cho input
        let input_bytes = input_json.as_bytes();
        let input_size = input_bytes.len();
        
        let alloc_result = alloc.call(&[Value::I32(input_size as i32)])?;
        let input_ptr = alloc_result[0].unwrap_i32() as usize;
        
        // Ghi input vào memory
        memory.view()[input_ptr..(input_ptr + input_size)].copy_from_slice(input_bytes);
        
        // Gọi hàm phân tích
        let analyze_result = analyze.call(&[
            Value::I32(input_ptr as i32),
            Value::I32(input_size as i32),
        ])?;
        
        // Lấy con trỏ và kích thước kết quả
        let result_ptr = analyze_result[0].unwrap_i32() as usize;
        let result_size = analyze_result[1].unwrap_i32() as usize;
        
        // Đọc kết quả từ memory
        let result_bytes = memory.view()[result_ptr..(result_ptr + result_size)].to_vec();
        let result_json = String::from_utf8(result_bytes)?;
        
        // Giải phóng memory
        dealloc.call(&[
            Value::I32(input_ptr as i32),
            Value::I32(input_size as i32),
        ])?;
        
        dealloc.call(&[
            Value::I32(result_ptr as i32),
            Value::I32(result_size as i32),
        ])?;
        
        // Parse kết quả
        let result: WasmAnalysisResult = serde_json::from_str(&result_json)?;
        
        Ok(result)
    }
}

// Khởi tạo WASM runtime global
pub async fn init_wasm_engine() -> Result<()> {
    let mut runtime = WasmRuntime::new()?;
    
    // Tải các module WASM
    let wasm_path = Path::new("wasm/target/wasm32-unknown-unknown/release/token_analyzer.wasm");
    
    if wasm_path.exists() {
        runtime.load_module("token_analyzer", wasm_path)?;
    } else {
        warn!("WASM module không tồn tại tại đường dẫn: {}", wasm_path.display());
    }
    
    // Lưu runtime vào biến global
    let runtime = Arc::new(Mutex::new(runtime));
    
    if WASM_RUNTIME.set(runtime).is_err() {
        error!("WASM runtime đã được khởi tạo trước đó");
    }
    
    info!("WASM Engine đã được khởi tạo thành công");
    Ok(())
}

// Phân tích transaction sử dụng WASM
pub async fn analyze_transaction(transaction: &Transaction) -> Result<WasmAnalysisResult> {
    let runtime = WASM_RUNTIME.get()
        .ok_or_else(|| anyhow!("WASM runtime chưa được khởi tạo"))?;
    
    let runtime = runtime.lock().await;
    
    // Chuẩn bị input để phân tích
    let input = prepare_wasm_input(transaction)?;
    
    // Thực hiện phân tích
    let result = runtime.execute_analysis("token_analyzer", &input)?;
    debug!(result = ?result, "Kết quả phân tích WASM");
    
    Ok(result)
}

// Chuẩn bị dữ liệu input cho WASM từ transaction
fn prepare_wasm_input(tx: &Transaction) -> Result<WasmInput> {
    let method_id = if tx.input.len() >= 4 {
        format!("0x{}", hex::encode(&tx.input.0[0..4]))
    } else {
        "0x".to_string()
    };
    
    Ok(WasmInput {
        transaction_data: format!("0x{}", hex::encode(&tx.input.0)),
        token_address: tx.to.map(|addr| format!("{:?}", addr)),
        method_id,
        sender: format!("{:?}", tx.from),
        receiver: tx.to.map_or("0x".to_string(), |addr| format!("{:?}", addr)),
        value: tx.value.to_string(),
        gas_price: tx.gas_price.unwrap_or_default().to_string(),
        gas_limit: tx.gas.to_string(),
    })
}

// External imports
use ethers::{
    abi::{self, Token},
    contract::Contract,
    middleware::Middleware,
    types::{Address, Bytes},
};

// Standard library imports
use std::{
    sync::Arc,
    str::FromStr,
    fs,
    path::Path,
};

// Third party imports
use anyhow::{Result, anyhow, Context};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};

/// Thông tin về smart contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    /// Địa chỉ của contract
    pub address: String,
    /// Tên của contract
    pub name: String,
    /// ABI của contract dạng JSON string
    pub abi: String,
    /// Chain ID nơi contract được triển khai
    pub chain_id: u64,
    /// Thời gian tạo (Unix timestamp)
    pub created_at: u64,
    /// Thời gian sử dụng gần nhất (Unix timestamp)
    pub last_used: u64,
    /// Trạng thái xác minh
    pub verified: bool,
}

/// Quản lý các smart contract
pub struct ContractManager {
    /// Danh sách các contract đã lưu
    contracts: Vec<ContractInfo>,
    /// Đường dẫn lưu trữ
    path: String,
}

impl ContractManager {
    /// Tạo contract manager mới từ đường dẫn
    pub async fn new(path: &str) -> Result<Self> {
        let contracts = if Path::new(path).exists() {
            let content = fs::read_to_string(path)
                .with_context(|| format!("Không thể đọc file contracts: {}", path))?;
            serde_json::from_str(&content)
                .with_context(|| format!("Không thể parse JSON từ file {}", path))?
        } else {
            Vec::new()
        };
        
        let manager = Self {
            contracts,
            path: path.to_string(),
        };
        
        Ok(manager)
    }
    
    /// Lưu trạng thái hiện tại vào file
    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.contracts)
            .with_context(|| "Không thể serialize contracts to JSON")?;
        
        let dir_path = Path::new(&self.path).parent();
        if let Some(dir) = dir_path {
            if !dir.exists() {
                fs::create_dir_all(dir)
                    .with_context(|| format!("Không thể tạo thư mục: {:?}", dir))?;
            }
        }
        
        fs::write(&self.path, content)
            .with_context(|| format!("Không thể ghi file: {}", self.path))?;
        
        Ok(())
    }
    
    /// Thêm contract mới
    pub fn add_contract(&mut self, contract: ContractInfo) -> Result<()> {
        // Kiểm tra contract đã tồn tại chưa
        if self.contracts.iter().any(|c| 
            c.address.to_lowercase() == contract.address.to_lowercase() && 
            c.chain_id == contract.chain_id
        ) {
            return Err(anyhow!("Contract đã tồn tại"));
        }
        
        // Kiểm tra ABI hợp lệ
        let _: abi::Abi = serde_json::from_str(&contract.abi)
            .with_context(|| "ABI không hợp lệ")?;
        
        self.contracts.push(contract);
        self.save()?;
        
        Ok(())
    }
    
    /// Lấy thông tin contract theo địa chỉ và chain ID
    pub fn get_contract(&self, address: &str, chain_id: u64) -> Option<&ContractInfo> {
        self.contracts.iter().find(|c| 
            c.address.to_lowercase() == address.to_lowercase() && 
            c.chain_id == chain_id
        )
    }
    
    /// Lấy danh sách tất cả các contract
    pub fn get_all_contracts(&self) -> &[ContractInfo] {
        &self.contracts
    }
    
    /// Lấy danh sách contract theo chain ID
    pub fn get_contracts_by_chain(&self, chain_id: u64) -> Vec<&ContractInfo> {
        self.contracts.iter()
            .filter(|c| c.chain_id == chain_id)
            .collect()
    }
    
    /// Cập nhật thời gian sử dụng gần nhất
    pub fn update_last_used(&mut self, address: &str, chain_id: u64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let contract = self.contracts.iter_mut()
            .find(|c| c.address.to_lowercase() == address.to_lowercase() && c.chain_id == chain_id)
            .ok_or_else(|| anyhow!("Contract không tồn tại"))?;
        
        contract.last_used = now;
        self.save()?;
        
        Ok(())
    }
    
    /// Xóa contract
    pub fn remove_contract(&mut self, address: &str, chain_id: u64) -> Result<()> {
        let initial_len = self.contracts.len();
        
        self.contracts.retain(|c| 
            c.address.to_lowercase() != address.to_lowercase() || 
            c.chain_id != chain_id
        );
        
        if self.contracts.len() == initial_len {
            return Err(anyhow!("Contract không tồn tại"));
        }
        
        self.save()?;
        
        Ok(())
    }
    
    /// Tương tác với contract
    pub async fn interact<M, T>(&self, client: Arc<M>, address: &str, function: &str, args: Vec<T>) -> Result<Bytes> 
    where
        M: Middleware + 'static,
        T: Into<Token> + Send + Sync,
    {
        // Lấy chain ID từ client
        let chain_id = client.get_chainid().await?;
        
        // Tìm thông tin contract
        let contract_info = self.get_contract(address, chain_id.as_u64())
            .ok_or_else(|| anyhow!("Contract không tìm thấy"))?;
        
        // Parse địa chỉ contract
        let contract_address = Address::from_str(address)
            .with_context(|| format!("Địa chỉ contract không hợp lệ: {}", address))?;
        
        // Parse ABI của contract
        let abi: abi::Abi = serde_json::from_str(&contract_info.abi)
            .with_context(|| "Không thể parse ABI")?;
        
        // Tạo contract instance
        let contract = Contract::new(contract_address, abi, client);
        
        // Chuyển đổi tham số
        let params: Vec<Token> = args.into_iter()
            .map(|arg| arg.into())
            .collect();
        
        // Gọi phương thức
        let result = contract.method(function, params)
            .with_context(|| format!("Lỗi khi chuẩn bị gọi hàm {}", function))?
            .call()
            .await
            .with_context(|| format!("Lỗi khi gọi hàm {}", function))?;
        
        // Cập nhật thời gian sử dụng gần nhất
        let mut manager = self.clone();
        let _ = manager.update_last_used(address, chain_id.as_u64());
        
        Ok(result)
    }
    
    /// Tương tác với contract sử dụng Token trực tiếp
    pub async fn interact_with_tokens<M>(&self, client: Arc<M>, address: &str, function: &str, args: Vec<Token>) -> Result<Bytes> 
    where
        M: Middleware + 'static,
    {
        // Lấy chain ID từ client
        let chain_id = client.get_chainid().await?;
        
        // Tìm thông tin contract
        let contract_info = self.get_contract(address, chain_id.as_u64())
            .ok_or_else(|| anyhow!("Contract không tìm thấy"))?;
        
        // Parse địa chỉ contract
        let contract_address = Address::from_str(address)
            .with_context(|| format!("Địa chỉ contract không hợp lệ: {}", address))?;
        
        // Parse ABI của contract
        let abi: abi::Abi = serde_json::from_str(&contract_info.abi)
            .with_context(|| "Không thể parse ABI")?;
        
        // Tạo contract instance
        let contract = Contract::new(contract_address, abi, client);
        
        // Gọi phương thức với tham số đã được chuyển đổi sẵn
        let result = contract.method(function, args)
            .with_context(|| format!("Lỗi khi chuẩn bị gọi hàm {}", function))?
            .call()
            .await
            .with_context(|| format!("Lỗi khi gọi hàm {}", function))?;
        
        // Cập nhật thời gian sử dụng gần nhất
        let mut manager = self.clone();
        let _ = manager.update_last_used(address, chain_id.as_u64());
        
        Ok(result)
    }
    
    /// Kiểm tra ABI hợp lệ
    pub fn validate_abi(abi_str: &str) -> Result<()> {
        let _: abi::Abi = serde_json::from_str(abi_str)
            .with_context(|| "ABI không hợp lệ")?;
        Ok(())
    }
}

impl Clone for ContractManager {
    fn clone(&self) -> Self {
        Self {
            contracts: self.contracts.clone(),
            path: self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    
    fn create_test_contract() -> ContractInfo {
        ContractInfo {
            address: "0x1234567890123456789012345678901234567890".to_string(),
            name: "Test Contract".to_string(),
            abi: r#"[{"inputs":[],"stateMutability":"nonpayable","type":"constructor"}]"#.to_string(),
            chain_id: 1,
            created_at: 1617235200,
            last_used: 1617235200,
            verified: true,
        }
    }
    
    #[tokio::test]
    async fn test_contract_manager() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("contracts.json");
        let path_str = path.to_str().unwrap();
        
        let mut manager = ContractManager::new(path_str).await.unwrap();
        let contract = create_test_contract();
        
        // Thêm contract
        assert!(manager.add_contract(contract.clone()).is_ok());
        
        // Kiểm tra contract đã thêm
        let retrieved = manager.get_contract(&contract.address, contract.chain_id);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.name, "Test Contract");
        
        // Không thể thêm contract trùng
        assert!(manager.add_contract(contract.clone()).is_err());
        
        // Lấy contract theo chain
        let chain_contracts = manager.get_contracts_by_chain(1);
        assert_eq!(chain_contracts.len(), 1);
        
        // Xóa contract
        assert!(manager.remove_contract(&contract.address, contract.chain_id).is_ok());
        assert!(manager.get_contract(&contract.address, contract.chain_id).is_none());
    }
    
    #[test]
    fn test_validate_abi() {
        // ABI hợp lệ
        let valid_abi = r#"[{"inputs":[],"stateMutability":"nonpayable","type":"constructor"}]"#;
        assert!(ContractManager::validate_abi(valid_abi).is_ok());
        
        // ABI không hợp lệ
        let invalid_abi = "not a json";
        assert!(ContractManager::validate_abi(invalid_abi).is_err());
    }
} 
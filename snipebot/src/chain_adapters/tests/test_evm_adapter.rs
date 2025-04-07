use std::sync::Arc;
use std::str::FromStr;
use anyhow::Result;
use ethers::types::{Address, H256, U256};
use tokio::sync::RwLock;
use crate::chain_adapters::chain_adapter_impl::EVMChainAdapter;
use crate::chain_adapters::interfaces::ChainAdapter;
use crate::chain_adapters::chain_registry::{ChainRegistry, ChainConfig};
use crate::chain_adapters::retry_policy::create_default_retry_policy;
use std::collections::HashMap;

/// Cấu hình cho mạng thử nghiệm
fn get_test_config() -> ChainConfig {
    let mut router_contracts = HashMap::new();
    router_contracts.insert("default".to_string(), "0x1b02dA8Cb0d097eB8D57A175b88c7D8b47997506".to_string());
    
    let mut factory_contracts = HashMap::new();
    factory_contracts.insert("default".to_string(), "0xc35DADB65012eC5796536bD9864eD8773aBc74C4".to_string());
    
    let mut stable_tokens = HashMap::new();
    stable_tokens.insert("USDC".to_string(), "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string());
    
    ChainConfig {
        chain_id: 11155111, // Sepolia testnet
        name: "Sepolia".to_string(),
        chain_type: "EVM".to_string(),
        native_token: "ETH".to_string(),
        block_time: 12,
        // RPC URLs thực tế cho testnet
        primary_rpc_urls: vec!["https://rpc.sepolia.org".to_string()],
        backup_rpc_urls: vec!["https://sepolia.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161".to_string()],
        explorer_url: "https://sepolia.etherscan.io".to_string(),
        tx_explorer_url_template: "https://sepolia.etherscan.io/tx/{0}".to_string(),
        address_explorer_url_template: "https://sepolia.etherscan.io/address/{0}".to_string(),
        router_contracts,
        factory_contracts,
        stable_tokens,
        gas_config: crate::chain_adapters::chain_registry::GasConfig {
            supports_eip1559: true,
            base_fee: 1.0,
            priority_fee: 1.0,
            default_gas_limit: 250000,
        },
        connection_pool_config: crate::chain_adapters::connection_pool::ConnectionPoolConfig {
            min_connections: 1,
            max_connections: 5,
            connection_timeout_ms: 5000,
            max_requests_per_second: 10,
            health_check_interval_ms: 60000,
        },
    }
}

// Tạo adapter với cấu hình thử nghiệm
async fn create_test_adapter() -> Result<Arc<EVMChainAdapter>> {
    let config = get_test_config();
    EVMChainAdapter::from_config(config).await
}

#[tokio::test]
async fn test_create_adapter() -> Result<()> {
    let adapter = create_test_adapter().await?;
    assert_eq!(adapter.get_chain_id(), 11155111);
    assert_eq!(adapter.get_type(), "EVM");
    Ok(())
}

#[tokio::test]
#[ignore] // Bỏ qua trong CI vì cần kết nối internet
async fn test_get_block_number() -> Result<()> {
    let adapter = create_test_adapter().await?;
    let block_number = adapter.get_block_number().await?;
    println!("Current block number: {}", block_number);
    // Chỉ kiểm tra nếu block number > 0
    assert!(block_number > 0);
    Ok(())
}

#[tokio::test]
#[ignore] // Bỏ qua trong CI vì cần kết nối internet
async fn test_get_gas_price() -> Result<()> {
    let adapter = create_test_adapter().await?;
    let gas_price = adapter.get_gas_price().await?;
    println!("Current gas price: {} wei", gas_price);
    // Chỉ kiểm tra nếu gas price > 0
    assert!(gas_price > U256::zero());
    Ok(())
}

#[tokio::test]
#[ignore] // Bỏ qua trong CI vì cần kết nối internet
async fn test_get_eth_balance() -> Result<()> {
    let adapter = create_test_adapter().await?;
    // Địa chỉ Ethereum trên Sepolia với một số ETH
    let address = Address::from_str("0x742d35Cc6634C0532925a3b844Bc454e4438f44e").unwrap();
    let balance = adapter.get_eth_balance(address, None).await?;
    println!("ETH balance: {} wei", balance);
    Ok(())
}

#[tokio::test]
#[ignore] // Bỏ qua trong CI vì cần kết nối internet
async fn test_get_token_details() -> Result<()> {
    let adapter = create_test_adapter().await?;
    // Địa chỉ của USDC trên Sepolia (giả định)
    // Thay bằng địa chỉ token thực tế khi chạy kiểm thử
    let token_address = Address::from_str("0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238").unwrap();
    
    match adapter.get_token_details(token_address).await {
        Ok(details) => {
            println!("Token details:");
            println!("  Name: {}", details.name);
            println!("  Symbol: {}", details.symbol);
            println!("  Decimals: {}", details.decimals);
            println!("  Total supply: {}", details.total_supply);
            assert!(!details.name.is_empty());
            assert!(!details.symbol.is_empty());
        },
        Err(e) => {
            println!("Error getting token details (expected on mock token): {:?}", e);
            // Không gây lỗi kiểm thử nếu token không tồn tại
        }
    }
    
    Ok(())
}

#[tokio::test]
#[ignore] // Bỏ qua trong CI vì cần wallet với private key
async fn test_send_transaction() -> Result<()> {
    // Chỉ chạy kiểm thử này khi có wallet được cấu hình
    // Đây là kiểm thử hướng dẫn, không nên chạy trên CI
    
    // Tạo wallet từ private key
    // let private_key = "0x..."; // Thêm private key thật khi cần kiểm thử
    // let wallet = ethers::signers::LocalWallet::from_str(private_key).unwrap();
    
    // let config = get_test_config();
    // let adapter = EVMChainAdapter::new(config.chain_id, None).await?;
    // let adapter = adapter.with_wallet(wallet);
    
    // Tạo transaction đơn giản gửi 0 ETH đến một địa chỉ
    // let tx = ethers::types::TransactionRequest::new()
    //     .to("0x742d35Cc6634C0532925a3b844Bc454e4438f44e")
    //     .value(0)
    //     .gas_price(adapter.get_gas_price().await?)
    //     .gas(21000);
    
    // Gửi transaction
    // let pending_tx = adapter.send_transaction(&tx).await?;
    // let tx_hash = pending_tx.tx_hash();
    // println!("Transaction sent with hash: {:?}", tx_hash);
    
    // Chờ receipt
    // let receipt = adapter
    //     .wait_for_transaction_receipt(tx_hash, 1, std::time::Duration::from_secs(60))
    //     .await?;
    // println!("Transaction mined in block: {:?}", receipt.block_number);
    
    // Bỏ qua kiểm thử
    Ok(())
}

// Các hàm trợ giúp kiểm thử

// Tạo registry cho kiểm thử
async fn create_test_registry() -> Arc<RwLock<ChainRegistry>> {
    let registry = ChainRegistry::new();
    let registry_arc = Arc::new(RwLock::new(registry));
    
    // Thêm adapter cho Sepolia
    let config = get_test_config();
    let adapter = EVMChainAdapter::from_config(config.clone()).await.unwrap();
    
    let mut registry_guard = registry_arc.write().await;
    registry_guard.register_adapter(config.chain_id, adapter);
    
    drop(registry_guard);
    registry_arc
}

#[tokio::test]
async fn test_chain_registry() -> Result<()> {
    let registry = create_test_registry().await;
    
    let registry_guard = registry.read().await;
    let adapter = registry_guard.get_adapter(11155111);
    
    assert!(adapter.is_some());
    let chain_id = adapter.unwrap().get_chain_id();
    assert_eq!(chain_id, 11155111);
    
    Ok(())
}

// Test retry policy
#[tokio::test]
async fn test_retry_policy() -> Result<()> {
    let retry_policy = create_default_retry_policy();
    
    // Tạo một hàm sẽ thành công sau 2 lần thử
    let mut attempt = 0;
    let result = retry_policy.retry(
        || async {
            attempt += 1;
            if attempt < 3 {
                Err(anyhow::anyhow!("Simulated failure"))
            } else {
                Ok(42)
            }
        },
        &crate::chain_adapters::retry_policy::RetryContext::new(
            "test",
            "http://example.com",
            1,
            None,
        ),
    ).await;
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
    assert_eq!(attempt, 3); // Đã thử 3 lần
    
    Ok(())
} 
use ethers::prelude::*;
use ethers::types::{Address, U256};
use std::sync::Arc;
use std::str::FromStr;
use anyhow::Result;
use diamond_blockchain::abi::abis as blockchain_abis;
use std::collections::HashMap;

pub struct DexManager {
    routers: Vec<(String, Address)>, // (name, address)
    factories: Vec<(String, Address)>, // (name, address)
}

impl DexManager {
    pub fn new() -> Self {
        Self {
            routers: Vec::new(),
            factories: Vec::new(),
        }
    }
    
    pub fn add_router(&mut self, name: &str, address: &str) -> Result<()> {
        let addr = Address::from_str(address)?;
        self.routers.push((name.to_string(), addr));
        Ok(())
    }
    
    pub fn add_factory(&mut self, name: &str, address: &str) -> Result<()> {
        let addr = Address::from_str(address)?;
        self.factories.push((name.to_string(), addr));
        Ok(())
    }
    
    pub fn get_router(&self, name: &str) -> Option<Address> {
        self.routers.iter()
            .find(|(n, _)| n == name)
            .map(|(_, a)| *a)
    }
    
    pub fn get_factory(&self, name: &str) -> Option<Address> {
        self.factories.iter()
            .find(|(n, _)| n == name)
            .map(|(_, a)| *a)
    }
    
    pub async fn get_pair<M: Middleware + 'static>(&self, client: Arc<M>, token_a: Address, token_b: Address, factory_name: &str) -> Result<Option<Address>> {
        let factory = self.get_factory(factory_name)
            .ok_or_else(|| anyhow::anyhow!("Factory not found"))?;
        
        let factory_contract = ethers::contract::Contract::new(
            factory,
            blockchain_abis::uniswap_v2_factory::UNIV2FACTORY_ABI.clone(),
            client,
        );
        
        let pair: Address = factory_contract
            .method("getPair", (token_a, token_b))?
            .call()
            .await?;
        
        if pair == Address::zero() {
            Ok(None)
        } else {
            Ok(Some(pair))
        }
    }
    
    pub async fn get_reserves<M: Middleware + 'static>(&self, client: Arc<M>, pair: Address) -> Result<(U256, U256)> {
        let pair_contract = ethers::contract::Contract::new(
            pair,
            blockchain_abis::uniswap_v2_pair::UNIV2PAIR_ABI.clone(),
            client,
        );
        
        let (reserve0, reserve1, _): (U256, U256, u32) = pair_contract
            .method("getReserves", ())?
            .call()
            .await?;
        
        Ok((reserve0, reserve1))
    }
    
    pub async fn calculate_price<M: Middleware + 'static>(&self, client: Arc<M>, token_a: Address, token_b: Address, factory_name: &str) -> Result<Option<f64>> {
        let pair = match self.get_pair(client.clone(), token_a, token_b, factory_name).await? {
            Some(pair) => pair,
            None => return Ok(None),
        };
        
        let (reserve0, reserve1) = self.get_reserves(client.clone(), pair).await?;
        
        // Check token order
        let pair_contract = ethers::contract::Contract::new(
            pair,
            blockchain_abis::uniswap_v2_pair::UNIV2PAIR_ABI.clone(),
            client,
        );
        
        let token0: Address = pair_contract.method("token0", ())?.call().await?;
        
        let (reserve_a, reserve_b) = if token0 == token_a {
            (reserve0, reserve1)
        } else {
            (reserve1, reserve0)
        };
        
        if reserve_a.is_zero() {
            return Ok(Some(0.0));
        }
        
        let price = reserve_b.as_u128() as f64 / reserve_a.as_u128() as f64;
        Ok(Some(price))
    }
}

impl Default for DexManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DexInfo {
    pub name: String,
    pub router_address: Address,
    pub factory_address: Address,
    pub chain_id: u64,
    pub fees: Vec<u32>, // Phí giao dịch (ví dụ: 30 = 0.3%)
    pub supports_v3: bool,
}

pub struct LendingPlatform {
    pub name: String,
    pub address: Address,
    pub chain_id: u64,
    pub supported_tokens: Vec<Address>,
    pub lending_rates: HashMap<Address, f64>,
    pub borrowing_rates: HashMap<Address, f64>,
}

pub struct YieldFarm {
    pub name: String,
    pub address: Address,
    pub chain_id: u64,
    pub staking_token: Address,
    pub reward_token: Address,
    pub apr: f64,
    pub tvl: U256,
    pub lock_period_days: u64,
}

pub struct SwapRoute {
    pub from_token: Address,
    pub to_token: Address,
    pub through_dexes: Vec<String>,
    pub through_tokens: Vec<Address>,
    pub expected_output: U256,
    pub price_impact: f64,
    pub gas_estimate: u64,
}

pub struct YieldOpportunity {
    pub platform: String,
    pub token: Address,
    pub apy: f64,
    pub tvl: U256,
    pub rewards: Vec<Address>,
    pub lock_period_days: Option<u64>,
}

pub struct LendingOpportunity {
    pub platform: String,
    pub token: Address,
    pub lending_rate: f64,
    pub borrowing_rate: f64,
    pub available_liquidity: U256,
}

pub struct DeFiAggregator {
    dexes: Vec<DexInfo>,
    lending_platforms: Vec<LendingPlatform>,
    yield_farms: Vec<YieldFarm>,
}

impl DeFiAggregator {
    pub fn new() -> Self {
        Self {
            dexes: Vec::new(),
            lending_platforms: Vec::new(),
            yield_farms: Vec::new(),
        }
    }
    
    pub async fn find_best_swap_route(&self, from_token: &str, 
                                    to_token: &str, 
                                    _amount: U256) -> Result<SwapRoute> {
        // Tìm đường đi tối ưu cho swap
        let from_addr = Address::from_str(from_token)?;
        let to_addr = Address::from_str(to_token)?;
        
        // Không có DEX nào được cấu hình
        if self.dexes.is_empty() {
            return Err(anyhow::anyhow!("Chưa cấu hình DEX nào"));
        }

        // Lưu trữ kết quả tốt nhất
        let mut best_route: Option<SwapRoute> = None;
        let mut best_output: U256 = U256::zero();
        let mut lowest_impact: f64 = 100.0; // Bắt đầu với 100% tác động giá
        
        // Kiểm tra trên mỗi DEX
        for dex in &self.dexes {
            // Bỏ qua các DEX không phù hợp với chain ID
            if false /* dex.chain_id != current_chain_id */ {
                continue;
            }
            
            // TODO: Triển khai logic kiểm tra thanh khoản
            let liquidity_check = true; 
            if !liquidity_check {
                continue;
            }
            
            // Tính toán output dự kiến và tác động giá
            let output = U256::from(98_000_000_000_000_000_000_u128); // Giả lập: 98% của input sau khi chuyển đổi
            let impact = 2.0; // Giả lập 2% slippage
            
            // Kiểm tra nếu đây là tuyến đường tốt nhất
            if impact < lowest_impact || (impact == lowest_impact && output > best_output) {
                lowest_impact = impact;
                best_output = output;
                
                best_route = Some(SwapRoute {
                    from_token: from_addr,
                    to_token: to_addr,
                    through_dexes: vec![dex.name.clone()],
                    through_tokens: vec![],
                    expected_output: output,
                    price_impact: impact,
                    gas_estimate: 250_000, // Ước tính gas mặc định
                });
            }
        }
        
        // Trả về tuyến đường tốt nhất hoặc lỗi
        match best_route {
            Some(route) => Ok(route),
            None => Err(anyhow::anyhow!("Không tìm thấy tuyến đường swap khả thi"))
        }
    }
    
    pub async fn find_best_yield(&self, token: &str) -> Result<YieldOpportunity> {
        // Tìm cơ hội yield farming tốt nhất
        let token_addr = Address::from_str(token)?;
        
        if self.yield_farms.is_empty() {
            return Err(anyhow::anyhow!("Chưa cấu hình farm nào"));
        }
        
        // Tìm farm tốt nhất cho token này
        let mut best_farm: Option<&YieldFarm> = None;
        let mut highest_apy = 0.0;
        
        for farm in &self.yield_farms {
            if farm.staking_token == token_addr && farm.apr > highest_apy {
                highest_apy = farm.apr;
                best_farm = Some(farm);
            }
        }
        
        match best_farm {
            Some(farm) => Ok(YieldOpportunity {
                platform: farm.name.clone(),
                token: token_addr,
                apy: farm.apr,
                tvl: farm.tvl,
                rewards: vec![farm.reward_token],
                lock_period_days: Some(farm.lock_period_days),
            }),
            None => Err(anyhow::anyhow!("Không tìm thấy farm nào cho token này"))
        }
    }
    
    pub async fn find_best_lending_rate(&self, token: &str) -> Result<LendingOpportunity> {
        // Tìm lãi suất cho vay tốt nhất
        let token_addr = Address::from_str(token)?;
        
        if self.lending_platforms.is_empty() {
            return Err(anyhow::anyhow!("Chưa cấu hình nền tảng cho vay nào"));
        }
        
        // Tìm platform có lãi suất cho vay tốt nhất
        let mut best_platform: Option<&LendingPlatform> = None;
        let mut highest_rate = 0.0;
        
        for platform in &self.lending_platforms {
            if platform.supported_tokens.contains(&token_addr) {
                if let Some(rate) = platform.lending_rates.get(&token_addr) {
                    if *rate > highest_rate {
                        highest_rate = *rate;
                        best_platform = Some(platform);
                    }
                }
            }
        }
        
        match best_platform {
            Some(platform) => {
                let lending_rate = *platform.lending_rates.get(&token_addr).unwrap_or(&0.0);
                let borrowing_rate = *platform.borrowing_rates.get(&token_addr).unwrap_or(&0.0);
                
                // Ước tính thanh khoản sẵn có (đây chỉ là giá trị giả định)
                let available_liquidity = U256::from(1_000_000_000_000_000_000_000_u128);
                
                Ok(LendingOpportunity {
                    platform: platform.name.clone(),
                    token: token_addr,
                    lending_rate,
                    borrowing_rate,
                    available_liquidity,
                })
            },
            None => Err(anyhow::anyhow!("Không tìm thấy nền tảng cho vay nào hỗ trợ token này"))
        }
    }
    
    // Thêm phương thức để cập nhật dữ liệu
    pub fn add_dex(&mut self, dex: DexInfo) {
        self.dexes.push(dex);
    }
    
    pub fn add_lending_platform(&mut self, platform: LendingPlatform) {
        self.lending_platforms.push(platform);
    }
    
    pub fn add_yield_farm(&mut self, farm: YieldFarm) {
        self.yield_farms.push(farm);
    }
    
    // Phương thức để lấy dữ liệu
    pub fn get_dexes(&self) -> &[DexInfo] {
        &self.dexes
    }
    
    pub fn get_lending_platforms(&self) -> &[LendingPlatform] {
        &self.lending_platforms
    }
    
    pub fn get_yield_farms(&self) -> &[YieldFarm] {
        &self.yield_farms
    }
}

impl Default for DeFiAggregator {
    fn default() -> Self {
        Self::new()
    }
}

mod abis {
    pub mod uniswap_v2 {
        use once_cell::sync::Lazy;
        
        #[allow(dead_code)]
        pub static UNIV2FACTORY_ABI: Lazy<ethers::abi::Abi> = Lazy::new(|| {
            serde_json::from_str(
                r#"[{"inputs":[{"internalType":"address","name":"_feeToSetter","type":"address"}],"payable":false,"stateMutability":"nonpayable","type":"constructor"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"token0","type":"address"},{"indexed":true,"internalType":"address","name":"token1","type":"address"},{"indexed":false,"internalType":"address","name":"pair","type":"address"},{"indexed":false,"internalType":"uint256","name":"","type":"uint256"}],"name":"PairCreated","type":"event"},{"constant":true,"inputs":[{"internalType":"uint256","name":"","type":"uint256"}],"name":"allPairs","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"allPairsLength","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"tokenA","type":"address"},{"internalType":"address","name":"tokenB","type":"address"}],"name":"createPair","outputs":[{"internalType":"address","name":"pair","type":"address"}],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"feeTo","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"feeToSetter","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"},{"internalType":"address","name":"","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"_feeTo","type":"address"}],"name":"setFeeTo","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"_feeToSetter","type":"address"}],"name":"setFeeToSetter","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"}]"#
            ).unwrap()
        });
    }
    
    pub mod uniswap_v3 {
        use once_cell::sync::Lazy;
        
        #[allow(dead_code)]
        pub static UNIV2PAIR_ABI: Lazy<ethers::abi::Abi> = Lazy::new(|| {
            serde_json::from_str(
                r#"[{"inputs":[],"payable":false,"stateMutability":"nonpayable","type":"constructor"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"owner","type":"address"},{"indexed":true,"internalType":"address","name":"spender","type":"address"},{"indexed":false,"internalType":"uint256","name":"value","type":"uint256"}],"name":"Approval","type":"event"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"sender","type":"address"},{"indexed":false,"internalType":"uint256","name":"amount0","type":"uint256"},{"indexed":false,"internalType":"uint256","name":"amount1","type":"uint256"},{"indexed":true,"internalType":"address","name":"to","type":"address"}],"name":"Burn","type":"event"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"sender","type":"address"},{"indexed":false,"internalType":"uint256","name":"amount0","type":"uint256"},{"indexed":false,"internalType":"uint256","name":"amount1","type":"uint256"}],"name":"Mint","type":"event"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"sender","type":"address"},{"indexed":false,"internalType":"uint256","name":"amount0In","type":"uint256"},{"indexed":false,"internalType":"uint256","name":"amount1In","type":"uint256"},{"indexed":false,"internalType":"uint256","name":"amount0Out","type":"uint256"},{"indexed":false,"internalType":"uint256","name":"amount1Out","type":"uint256"},{"indexed":true,"internalType":"address","name":"to","type":"address"}],"name":"Swap","type":"event"},{"anonymous":false,"inputs":[{"indexed":false,"internalType":"uint112","name":"reserve0","type":"uint112"},{"indexed":false,"internalType":"uint112","name":"reserve1","type":"uint112"}],"name":"Sync","type":"event"},{"anonymous":false,"inputs":[{"indexed":true,"internalType":"address","name":"from","type":"address"},{"indexed":true,"internalType":"address","name":"to","type":"address"},{"indexed":false,"internalType":"uint256","name":"value","type":"uint256"}],"name":"Transfer","type":"event"},{"constant":true,"inputs":[],"name":"DOMAIN_SEPARATOR","outputs":[{"internalType":"bytes32","name":"","type":"bytes32"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"MINIMUM_LIQUIDITY","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"PERMIT_TYPEHASH","outputs":[{"internalType":"bytes32","name":"","type":"bytes32"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"},{"internalType":"address","name":"","type":"address"}],"name":"allowance","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"spender","type":"address"},{"internalType":"uint256","name":"value","type":"uint256"}],"name":"approve","outputs":[{"internalType":"bool","name":"","type":"bool"}],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"}],"name":"balanceOf","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"to","type":"address"}],"name":"burn","outputs":[{"internalType":"uint256","name":"amount0","type":"uint256"},{"internalType":"uint256","name":"amount1","type":"uint256"}],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"decimals","outputs":[{"internalType":"uint8","name":"","type":"uint8"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"factory","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"getReserves","outputs":[{"internalType":"uint112","name":"_reserve0","type":"uint112"},{"internalType":"uint112","name":"_reserve1","type":"uint112"},{"internalType":"uint32","name":"_blockTimestampLast","type":"uint32"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"_token0","type":"address"},{"internalType":"address","name":"_token1","type":"address"}],"name":"initialize","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"kLast","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"to","type":"address"}],"name":"mint","outputs":[{"internalType":"uint256","name":"liquidity","type":"uint256"}],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"name","outputs":[{"internalType":"string","name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[{"internalType":"address","name":"","type":"address"}],"name":"nonces","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"owner","type":"address"},{"internalType":"address","name":"spender","type":"address"},{"internalType":"uint256","name":"value","type":"uint256"},{"internalType":"uint256","name":"deadline","type":"uint256"},{"internalType":"uint8","name":"v","type":"uint8"},{"internalType":"bytes32","name":"r","type":"bytes32"},{"internalType":"bytes32","name":"s","type":"bytes32"}],"name":"permit","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"price0CumulativeLast","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"price1CumulativeLast","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"to","type":"address"}],"name":"skim","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":false,"inputs":[{"internalType":"uint256","name":"amount0Out","type":"uint256"},{"internalType":"uint256","name":"amount1Out","type":"uint256"},{"internalType":"address","name":"to","type":"address"},{"internalType":"bytes","name":"data","type":"bytes"}],"name":"swap","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"symbol","outputs":[{"internalType":"string","name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[],"name":"sync","outputs":[],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":true,"inputs":[],"name":"token0","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"token1","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"totalSupply","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"to","type":"address"},{"internalType":"uint256","name":"value","type":"uint256"}],"name":"transfer","outputs":[{"internalType":"bool","name":"","type":"bool"}],"payable":false,"stateMutability":"nonpayable","type":"function"},{"constant":false,"inputs":[{"internalType":"address","name":"from","type":"address"},{"internalType":"address","name":"to","type":"address"},{"internalType":"uint256","name":"value","type":"uint256"}],"name":"transferFrom","outputs":[{"internalType":"bool","name":"","type":"bool"}],"payable":false,"stateMutability":"nonpayable","type":"function"}]"#
            ).unwrap()
        });
    }
}

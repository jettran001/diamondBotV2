use diamond_blockchain::abi;

// Re-export ABI cho ERC20
pub fn get_erc20_abi() -> &'static str {
    include_str!("../../blockchain/src/abi/erc20.json")
}

// Re-export ABI cho Uniswap V2 Router
pub fn get_router_abi() -> &'static str {
    include_str!("../../blockchain/src/abi/router.json")
}

// Re-export ABI cho Uniswap V2 Factory
pub fn get_factory_abi() -> &'static str {
    include_str!("../../blockchain/src/abi/factory.json")
}

// Re-export ABI cho Uniswap V2 Pair
pub fn get_pair_abi() -> &'static str {
    include_str!("../../blockchain/src/abi/pair.json")
}

// Re-export ABI hoặc trực tiếp
pub use abi::abis::erc20::ERC20_ABI;
pub use abi::abis::uniswap_v2_router::UNIV2ROUTER_ABI;
pub use abi::abis::uniswap_v2_factory::UNIV2FACTORY_ABI;
pub use abi::abis::uniswap_v2_pair::UNIV2PAIR_ABI; 
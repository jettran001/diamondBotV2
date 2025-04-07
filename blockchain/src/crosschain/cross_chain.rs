pub struct CrossChainBridge {
    supported_chains: HashMap<ChainId, ChainConfig>,
    bridge_contracts: HashMap<(ChainId, ChainId), Address>,
}

impl CrossChainBridge {
    pub fn new() -> Self { /* ... */ }
    
    pub async fn bridge_token(&self, token: &str, 
                           from_chain: ChainId, 
                           to_chain: ChainId, 
                           amount: U256,
                           recipient: &str) -> Result<H256> {
        // Chuyển token qua bridge
    }
    
    pub async fn get_bridge_fee(&self, from_chain: ChainId, 
                              to_chain: ChainId) -> Result<U256> {
        // Lấy phí bridge
    }
}

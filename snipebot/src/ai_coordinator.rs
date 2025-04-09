// snipebot/src/ai/ai_coordinator.rs
// External imports
use ethers::prelude::*;

// Standard library imports
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

// Internal imports
use crate::types::*;
use crate::AIModule::{AIModule, AIDecision};
use crate::token_status::TokenStatus;
use crate::risk_analyzer::TokenRiskAnalysis;

pub struct AICoordinator {
    ai_module: Arc<Mutex<AIModule>>,
    confidence_threshold: f64,
    auto_trade_enabled: bool,
    last_decisions: HashMap<String, (AIDecision, u64)>, // (token_address, (decision, timestamp))
}

impl AICoordinator {
    pub fn new(ai_module: Arc<Mutex<AIModule>>, confidence_threshold: f64, auto_trade_enabled: bool) -> Self {
        Self {
            ai_module,
            confidence_threshold,
            auto_trade_enabled,
            last_decisions: HashMap::new(),
        }
    }
    
    // Lấy quyết định giao dịch từ AI
    pub async fn get_ai_trade_decision(&self, token_address: &str) -> Result<AIDecision, Box<dyn std::error::Error>> {
        // Kiểm tra cache
        if let Some((decision, timestamp)) = self.last_decisions.get(token_address) {
            // Kiểm tra thời gian hiệu lực
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            // Nếu quyết định còn hiệu lực (chưa quá 5 phút), trả về từ cache
            if now - timestamp < 300 {
                return Ok(decision.clone());
            }
        }
        
        // Lấy quyết định mới từ AI module
        let ai_module = self.ai_module.lock().await;
        let decision = ai_module.get_trade_decision(token_address).await?;
        
        // Lưu vào cache
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Clone decision và token_address để tránh borrowing issues
        let token_address_owned = token_address.to_string();
        let decision_clone = decision.clone();
        
        // Thêm vào cache
        self.last_decisions.insert(token_address_owned, (decision_clone, now));
        
        Ok(decision)
    }
    
    // Kiểm tra nếu nên tự động giao dịch
    pub fn should_auto_trade(&self, decision: &AIDecision) -> bool {
        self.auto_trade_enabled && decision.confidence >= self.confidence_threshold
    }
    
    // Phân tích token mới
    pub async fn analyze_new_token(
        &self, 
        token_address: &str, 
        status: &TokenStatus, 
        risk_analysis: Option<&TokenRiskAnalysis>
    ) -> Result<AIDecision, Box<dyn std::error::Error>> {
        let ai_module = self.ai_module.lock().await;
        ai_module.analyze_new_token(token_address, status, risk_analysis).await
    }
    
    // Cập nhật cấu hình
    pub fn update_config(&mut self, confidence_threshold: f64, auto_trade_enabled: bool) {
        self.confidence_threshold = confidence_threshold;
        self.auto_trade_enabled = auto_trade_enabled;
    }
}

// External imports
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LookupMap, UnorderedMap, Vector},
    env, log, AccountId, Balance, BorshStorageKey, NearToken, PanicOnDefault, Promise, PromiseOrValue,
    near_bindgen, require,
};

// Standard library imports
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    fmt::{self, Display, Formatter},
    error::Error,
};

// Third party imports
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use async_trait::async_trait;
use tokio::time::{timeout, sleep};

// Internal imports
mod node_info;
mod network_stats;

use node_info::{NodeInfo, NodeId, NodeStatus};
use network_stats::{NetworkStats, NodeMetrics};

/// Mạng lưới các node DePIN
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault, Clone)]
pub struct NodeNetwork {
    /// Danh sách các node trong mạng
    pub nodes: Vector<NodeInfo>,
    /// Các kết nối giữa các node
    pub connections: LookupMap<NodeId, Vector<NodeId>>,
    /// Thống kê mạng lưới
    pub stats: NetworkStats,
    /// ID chủ sở hữu
    pub owner_id: AccountId,
    /// Thời gian cập nhật cuối cùng
    pub last_update: u64,
    /// ID của mạng lưới
    pub id: String,
    /// Thời gian tạo
    pub created_at: u64,
}

/// Storage keys cho các collection
#[derive(BorshSerialize, BorshStorageKey)]
enum StorageKey {
    Nodes,
    Connections,
    Stats,
}

#[near_bindgen]
impl NodeNetwork {
    /// Khởi tạo mạng lưới node mới
    /// 
    /// # Arguments
    /// 
    /// * `owner_id` - ID chủ sở hữu
    /// 
    /// # Returns
    /// 
    /// * `Self` - Instance mới của NodeNetwork
    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        Self {
            nodes: Vector::new(StorageKey::Nodes),
            connections: LookupMap::new(StorageKey::Connections),
            stats: NetworkStats::new(),
            owner_id,
            last_update: current_time,
            id: format!("node_network_{}", current_time),
            created_at: current_time,
        }
    }
    
    /// Đăng ký node mới vào mạng
    /// 
    /// # Arguments
    /// 
    /// * `node` - Thông tin node cần đăng ký
    /// 
    /// # Returns
    /// 
    /// * `Result<NodeId>` - ID của node mới nếu thành công
    pub fn register_node(&mut self, node: NodeInfo) -> Result<NodeId> {
        self.assert_owner();
        
        // Kiểm tra node đã tồn tại chưa
        for existing_node in self.nodes.iter() {
            if existing_node.id == node.id {
                return Err(anyhow::anyhow!("Node already exists"));
            }
        }
        
        // Thêm node mới
        self.nodes.push(&node);
        
        // Khởi tạo danh sách kết nối trống
        self.connections.insert(&node.id, &Vector::new(StorageKey::Connections));
        
        // Cập nhật thống kê
        self.stats.total_nodes += 1;
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        info!("Registered new node: {}", node.id);
        
        Ok(node.id)
    }
    
    /// Xác thực node
    /// 
    /// # Arguments
    /// 
    /// * `node_id` - ID của node cần xác thực
    /// 
    /// # Returns
    /// 
    /// * `Result<bool>` - Kết quả xác thực
    pub fn verify_node(&self, node_id: &NodeId) -> Result<bool> {
        // Kiểm tra node có tồn tại không
        let node = self.nodes.iter()
            .find(|n| n.id == *node_id)
            .ok_or_else(|| anyhow::anyhow!("Node not found"))?;
            
        // Kiểm tra trạng thái node
        Ok(node.status == NodeStatus::Active)
    }
    
    /// Lấy thống kê mạng lưới
    /// 
    /// # Returns
    /// 
    /// * `NetworkStats` - Thống kê mạng lưới
    pub fn get_network_stats(&self) -> NetworkStats {
        self.stats.clone()
    }
    
    /// Kiểm tra người gọi có phải là owner không
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only owner can call this method"
        );
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_context, VMContextBuilder};
    use near_sdk::{testing_env, AccountId};

    /// Test khởi tạo mạng lưới
    #[test]
    fn test_new() {
        let owner_id = AccountId::new_unchecked("alice.near".to_string());
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let network = NodeNetwork::new(owner_id.clone());
        
        assert_eq!(network.owner_id, owner_id);
        assert_eq!(network.stats.total_nodes, 0);
        assert!(!network.id.is_empty());
        assert!(network.created_at > 0);
        assert_eq!(network.last_update, network.created_at);
    }

    /// Test đăng ký node
    #[test]
    fn test_register_node() {
        let owner_id = AccountId::new_unchecked("alice.near".to_string());
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let mut network = NodeNetwork::new(owner_id.clone());
        
        let node = NodeInfo {
            id: "node1".to_string(),
            owner: owner_id.clone(),
            status: NodeStatus::Active,
            metrics: NodeMetrics::default(),
        };
        
        let result = network.register_node(node.clone());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), node.id);
        assert_eq!(network.stats.total_nodes, 1);
        assert!(network.last_update > network.created_at);
    }

    /// Test xác thực node
    #[test]
    fn test_verify_node() {
        let owner_id = AccountId::new_unchecked("alice.near".to_string());
        let context = get_context(owner_id.clone());
        testing_env!(context);
        
        let mut network = NodeNetwork::new(owner_id.clone());
        
        let node = NodeInfo {
            id: "node1".to_string(),
            owner: owner_id.clone(),
            status: NodeStatus::Active,
            metrics: NodeMetrics::default(),
        };
        
        network.register_node(node.clone()).unwrap();
        
        assert!(network.verify_node(&node.id).unwrap());
    }
}

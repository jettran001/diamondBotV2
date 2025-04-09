// External imports
use ethers::core::types::{Address, H256, U256};

// Standard library imports
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// Internal imports
use crate::error::Result;

// Third party imports
use anyhow::{anyhow};
use argon2::{
    password_hash::{
        rand_core::OsRng,
        SaltString, PasswordHash, PasswordHasher, PasswordVerifier,
    },
    Argon2,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::Utc;
use log::{info, error};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Phân quyền người dùng
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    User,
}

/// Cấp độ subscription
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubscriptionLevel {
    Free,
    Basic,
    Premium,
    VIP,
}

/// Thông tin subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub level: SubscriptionLevel,
    pub start_date: u64,
    pub expiry_date: u64,
    pub active: bool,
}

impl Subscription {
    /// Tạo subscription mới với level và thời hạn
    pub fn new(level: SubscriptionLevel, duration_days: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            level,
            start_date: now,
            expiry_date: now + duration_days * 86400,
            active: true,
        }
    }
    
    /// Kiểm tra subscription có đang hoạt động
    pub fn is_active(&self) -> bool {
        if !self.active {
            return false;
        }
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        now < self.expiry_date
    }
    
    /// Gia hạn subscription thêm số ngày
    pub fn extend(&mut self, days: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        // Nếu subscription đã hết hạn, tính từ hiện tại
        if now > self.expiry_date {
            self.start_date = now;
            self.expiry_date = now + days * 86400;
        } else {
            // Nếu chưa hết hạn, cộng thêm thời gian
            self.expiry_date += days * 86400;
        }
        
        self.active = true;
    }
    
    /// Cập nhật cấp độ subscription
    pub fn update_level(&mut self, level: SubscriptionLevel) {
        self.level = level;
    }
    
    /// Tính số ngày còn lại cho subscription
    pub fn remaining_days(&self) -> u64 {
        if !self.is_active() {
            return 0;
        }
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        if now >= self.expiry_date {
            return 0;
        }
        
        (self.expiry_date - now) / 86400
    }
    
    /// Vô hiệu hóa subscription
    pub fn deactivate(&mut self) {
        self.active = false;
    }
    
    /// Khôi phục subscription đã bị vô hiệu hóa
    pub fn reactivate(&mut self) {
        self.active = true;
    }
}

/// Cấu trúc thông tin người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub email: String,
    pub role: String,
    pub wallets: Vec<String>,
    pub last_login: Option<u64>,
}

/// Cấu trúc thông tin người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: String,
    pub updated_at: chrono::DateTime<Utc>,
    pub last_login: Option<chrono::DateTime<Utc>>,
    pub api_key: Option<String>,
    pub roles: Vec<String>,
    pub settings: HashMap<String, serde_json::Value>,
    pub subscription_id: Option<String>,
    pub role: Option<UserRole>,
    pub wallets: Vec<String>,
    pub active: bool,
    pub subscription: Option<Subscription>,
}

/// Quản lý người dùng
pub struct UserManager {
    users: HashMap<String, User>,
    path: String,
    lock: Arc<Mutex<()>>,
}

impl User {
    /// Tạo người dùng mới
    pub fn new(username: &str, email: &str, password: &str) -> Result<Self> {
        let password_hash = hash(password, DEFAULT_COST)?;
        let now = Utc::now();
        
        Ok(User {
            id: Uuid::new_v4().to_string(),
            username: username.to_string(),
            email: email.to_string(),
            password_hash,
            created_at: now.to_string(),
            updated_at: now,
            last_login: None,
            api_key: Some(Uuid::new_v4().to_string()),
            roles: vec!["user".to_string()],
            settings: HashMap::new(),
            subscription_id: None,
            role: Some(UserRole::User),
            wallets: Vec::new(),
            active: true,
            subscription: Some(Subscription::new(SubscriptionLevel::Free, 365)),
        })
    }
    
    /// Kiểm tra mật khẩu
    pub fn verify_password(&self, password: &str) -> bool {
        match verify(password, &self.password_hash) {
            Ok(result) => result,
            Err(_) => false,
        }
    }
    
    /// Đặt mật khẩu mới
    pub fn set_password(&mut self, password: &str) -> Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        
        let argon2 = Argon2::default();
        
        let password_hash_result = argon2.hash_password(password.as_bytes(), &salt);
        
        if let Ok(password_hash) = password_hash_result {
            self.password_hash = password_hash.to_string();
            self.updated_at = Utc::now();
            Ok(())
        } else {
            Err(anyhow!("Failed to hash password"))
        }
    }
    
    /// Cập nhật thời gian đăng nhập cuối
    pub fn update_last_login(&mut self) {
        self.last_login = Some(Utc::now());
    }
    
    /// Cập nhật API key
    pub fn regenerate_api_key(&mut self) {
        self.api_key = Some(Uuid::new_v4().to_string());
        self.updated_at = Utc::now();
    }
    
    /// Kiểm tra nếu người dùng có quyền truy cập vào ví
    pub fn has_wallet_access(&self, wallet_address: &str) -> bool {
        // Admin có quyền truy cập vào tất cả các ví
        if let Some(role) = &self.role {
            if *role == UserRole::Admin {
                return true;
            }
        }
        
        self.wallets.contains(&wallet_address.to_string())
    }
    
    /// Kiểm tra subscription có đang hoạt động
    pub fn has_active_subscription(&self) -> bool {
        if let Some(subscription) = &self.subscription {
            return subscription.is_active();
        }
        false
    }
    
    /// Cập nhật subscription
    pub fn update_subscription(&mut self, level: SubscriptionLevel, duration_days: u64) {
        if let Some(subscription) = &mut self.subscription {
            if subscription.is_active() {
                subscription.extend(duration_days);
                subscription.update_level(level);
            } else {
                *subscription = Subscription::new(level, duration_days);
            }
        } else {
            self.subscription = Some(Subscription::new(level, duration_days));
        }
        self.updated_at = Utc::now();
    }
    
    /// Thêm ví vào danh sách ví của người dùng
    pub fn add_wallet(&mut self, wallet_address: &str) {
        if !self.wallets.contains(&wallet_address.to_string()) {
            self.wallets.push(wallet_address.to_string());
            self.updated_at = Utc::now();
        }
    }
    
    /// Xóa ví khỏi danh sách ví của người dùng
    pub fn remove_wallet(&mut self, wallet_address: &str) {
        self.wallets.retain(|addr| addr != wallet_address);
        self.updated_at = Utc::now();
    }
    
    /// Chuyển đổi thành UserInfo để hiển thị công khai
    pub fn to_user_info(&self) -> UserInfo {
        UserInfo {
            username: self.username.clone(),
            email: self.email.clone(),
            role: self.roles.first().cloned().unwrap_or_else(|| "user".to_string()),
            wallets: self.wallets.clone(),
            last_login: self.last_login.map(|dt| dt.timestamp() as u64),
        }
    }
}

impl UserManager {
    /// Tạo UserManager mới
    pub async fn new(path: &str) -> Result<Self> {
        let mut manager = Self {
            users: HashMap::new(),
            path: path.to_string(),
            lock: Arc::new(Mutex::new(())),
        };
        
        // Đảm bảo thư mục tồn tại
        if let Some(parent) = Path::new(path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        
        // Load dữ liệu từ file nếu tồn tại
        if Path::new(path).exists() {
            manager.load().await?;
        } else {
            // Tạo tài khoản admin mặc định nếu chưa có file
            let admin_user = User {
                id: Uuid::new_v4().to_string(),
                username: "admin".to_string(),
                password_hash: hash("admin123", DEFAULT_COST)?,
                email: "admin@example.com".to_string(),
                created_at: Utc::now().to_string(),
                updated_at: Utc::now(),
                last_login: None,
                api_key: Some(Uuid::new_v4().to_string()),
                roles: vec!["admin".to_string()],
                settings: HashMap::new(),
                subscription_id: None,
                role: Some(UserRole::Admin),
                wallets: Vec::new(),
                active: true,
                subscription: Some(Subscription::new(SubscriptionLevel::VIP, 365)),
            };
            
            manager.users.insert(admin_user.id.clone(), admin_user);
            manager.save().await?;
        }
        
        Ok(manager)
    }
    
    /// Tải dữ liệu từ file
    async fn load(&mut self) -> Result<()> {
        let _guard = self.lock.lock().await;
        
        let mut file = File::open(&self.path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        
        self.users = serde_json::from_str(&contents)?;
        info!("Loaded {} users from {}", self.users.len(), self.path);
        
        Ok(())
    }
    
    /// Lưu dữ liệu vào file
    async fn save(&self) -> Result<()> {
        let _guard = self.lock.lock().await;
        
        let contents = serde_json::to_string_pretty(&self.users)?;
        let mut file = File::create(&self.path).await?;
        file.write_all(contents.as_bytes()).await?;
        
        info!("Saved {} users to {}", self.users.len(), self.path);
        
        Ok(())
    }
    
    /// Thêm người dùng mới
    pub async fn add_user(&mut self, user: User) -> Result<()> {
        // Kiểm tra trùng lặp
        if self.users.values().any(|u| u.username == user.username || u.email == user.email) {
            return Err(anyhow!("Username or email already exists"));
        }
        
        self.users.insert(user.id.clone(), user);
        self.save().await?;
        
        Ok(())
    }
    
    /// Lấy người dùng theo ID
    pub fn get_user(&self, id: &str) -> Option<&User> {
        self.users.get(id)
    }
    
    /// Lấy người dùng có thể thay đổi theo ID
    pub fn get_user_mut(&mut self, id: &str) -> Option<&mut User> {
        self.users.get_mut(id)
    }
    
    /// Lấy người dùng theo tên
    pub fn get_user_by_username(&self, username: &str) -> Option<&User> {
        self.users.values().find(|u| u.username == username)
    }
    
    /// Lấy người dùng theo email
    pub fn get_user_by_email(&self, email: &str) -> Option<&User> {
        self.users.values().find(|u| u.email == email)
    }
    
    /// Xác thực người dùng
    pub async fn authenticate(&mut self, username: &str, password: &str) -> Result<&User> {
        let user_id = self.users.values()
            .find(|u| u.username == username && u.verify_password(password) && u.active)
            .map(|u| u.id.clone())
            .ok_or_else(|| anyhow!("Invalid username or password"))?;
        
        // Cập nhật thời gian đăng nhập
        if let Some(user) = self.users.get_mut(&user_id) {
            user.update_last_login();
            self.save().await?;
            Ok(user)
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Cập nhật thông tin người dùng
    pub async fn update_user(&mut self, id: &str, update_fn: impl FnOnce(&mut User)) -> Result<()> {
        if let Some(user) = self.users.get_mut(id) {
            update_fn(user);
            user.updated_at = Utc::now();
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Xóa người dùng
    pub async fn delete_user(&mut self, id: &str) -> Result<()> {
        if self.users.remove(id).is_some() {
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Đổi mật khẩu
    pub async fn change_password(&mut self, username: &str, old_password: &str, new_password: &str) -> Result<()> {
        let user_id = self.users.values()
            .find(|u| u.username == username && u.verify_password(old_password))
            .map(|u| u.id.clone())
            .ok_or_else(|| anyhow!("Invalid username or password"))?;
            
        if let Some(user) = self.users.get_mut(&user_id) {
            user.set_password(new_password)?;
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Reset mật khẩu (chỉ Admin)
    pub async fn reset_password(&mut self, id: &str, new_password: &str) -> Result<()> {
        if let Some(user) = self.users.get_mut(id) {
            user.set_password(new_password)?;
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Gán ví cho người dùng
    pub async fn assign_wallet(&mut self, id: &str, wallet_address: &str) -> Result<()> {
        if let Some(user) = self.users.get_mut(id) {
            user.add_wallet(wallet_address);
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Gỡ bỏ ví khỏi người dùng
    pub async fn unassign_wallet(&mut self, id: &str, wallet_address: &str) -> Result<()> {
        if let Some(user) = self.users.get_mut(id) {
            user.remove_wallet(wallet_address);
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Cập nhật subscription của người dùng
    pub async fn update_user_subscription(&mut self, id: &str, level: SubscriptionLevel, duration_days: u64) -> Result<()> {
        if let Some(user) = self.users.get_mut(id) {
            user.update_subscription(level, duration_days);
            self.save().await?;
            Ok(())
        } else {
            Err(anyhow!("User not found"))
        }
    }
    
    /// Lấy danh sách tất cả người dùng
    pub fn get_all_users(&self) -> Vec<&User> {
        self.users.values().collect()
    }
    
    /// Lấy thông tin subscription của người dùng
    pub fn get_user_subscription(&self, username: &str) -> Result<Subscription> {
        if let Some(user) = self.get_user_by_username(username) {
            if let Some(subscription) = &user.subscription {
                return Ok(subscription.clone());
            }
            return Err(anyhow!("User has no subscription"));
        }
        Err(anyhow!("User not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_user_creation() {
        let user = User::new("testuser", "test@example.com", "password123").unwrap();
        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "test@example.com");
        assert!(user.verify_password("password123"));
    }
    
    #[tokio::test]
    async fn test_subscription() {
        let mut subscription = Subscription::new(SubscriptionLevel::Basic, 30);
        assert!(subscription.is_active());
        
        subscription.extend(60);
        assert!(subscription.is_active());
        
        // Kiểm tra ngày hết hạn đã được cộng thêm
        assert!(subscription.expiry_date > subscription.start_date + 30 * 86400);
        
        // Kiểm tra số ngày còn lại
        assert!(subscription.remaining_days() > 0);
        
        // Kiểm tra vô hiệu hóa subscription
        subscription.deactivate();
        assert!(!subscription.is_active());
        
        // Kiểm tra khôi phục subscription
        subscription.reactivate();
        assert!(subscription.is_active());
        
        // Kiểm tra cập nhật cấp độ
        subscription.update_level(SubscriptionLevel::Premium);
        assert_eq!(subscription.level, SubscriptionLevel::Premium);
    }
    
    #[tokio::test]
    async fn test_user_wallet_management() {
        let mut user = User::new("walletuser", "wallet@example.com", "password123").unwrap();
        
        // Thêm ví
        user.add_wallet("0x12345678901234567890");
        assert!(user.wallets.contains(&"0x12345678901234567890".to_string()));
        
        // Thêm lại ví đã có
        user.add_wallet("0x12345678901234567890");
        assert_eq!(user.wallets.len(), 1); // Không duplicate
        
        // Thêm ví mới
        user.add_wallet("0xabcdef1234567890");
        assert_eq!(user.wallets.len(), 2);
        
        // Xóa ví
        user.remove_wallet("0x12345678901234567890");
        assert_eq!(user.wallets.len(), 1);
        assert!(!user.wallets.contains(&"0x12345678901234567890".to_string()));
    }
}
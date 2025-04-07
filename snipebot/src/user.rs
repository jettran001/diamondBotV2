use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;
use bcrypt::{hash, verify, DEFAULT_COST};
use anyhow::{Result, anyhow};
use log::{info, error};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::user_subscription::{Subscription, SubscriptionLevel};

/// Cấu trúc thông tin người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub email: String,
    pub role: String,
    pub wallets: Vec<String>,
    pub last_login: Option<u64>,
}

// Phân quyền người dùng
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UserRole {
    Admin,
    User,
}

// Thông tin người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub email: String,
    pub role: UserRole,
    pub created_at: u64,
    pub last_login: Option<u64>,
    pub wallets: Vec<String>, // Danh sách địa chỉ ví mà người dùng có quyền truy cập
    pub active: bool,
    pub subscription: Subscription,
}

#[derive(Debug)]
pub struct UserManager {
    users: HashMap<String, User>, // username -> User
    path: String,
}

impl UserManager {
    pub async fn new(path: &str) -> Result<Self> {
        let mut manager = Self {
            users: HashMap::new(),
            path: path.to_string(),
        };
        
        // Đảm bảo thư mục tồn tại
        if let Some(parent) = Path::new(path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        
        // Tải danh sách người dùng nếu file tồn tại
        if Path::new(path).exists() {
            manager.load_users().await?;
        } else {
            // Tạo tài khoản admin mặc định nếu chưa có file
            let admin_user = User {
                id: Uuid::new_v4().to_string(),
                username: "admin".to_string(),
                password_hash: hash("admin123", DEFAULT_COST)?,
                email: "admin@example.com".to_string(),
                role: UserRole::Admin,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                last_login: None,
                wallets: Vec::new(),
                active: true,
                subscription: Subscription::new(SubscriptionLevel::VIP, 365),
            };
            
            manager.users.insert(admin_user.username.clone(), admin_user);
            manager.save_users().await?;
        }
        
        Ok(manager)
    }
    
    // Tải danh sách người dùng từ file
    pub async fn load_users(&mut self) -> Result<()> {
        let mut file = File::open(&self.path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        
        self.users = serde_json::from_str(&contents)?;
        Ok(())
    }
    
    // Lưu danh sách người dùng vào file
    pub async fn save_users(&self) -> Result<()> {
        let json = serde_json::to_string(&self.users)?;
        let mut file = File::create(&self.path).await?;
        file.write_all(json.as_bytes()).await?;
        Ok(())
    }
    
    // Xác thực người dùng
    pub fn authenticate(&mut self, username: &str, password: &str) -> Result<&User> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        
        if !user.active {
            return Err(anyhow!("Tài khoản bị vô hiệu hóa"));
        }
        
        if verify(password, &user.password_hash)? {
            // Cập nhật thời gian đăng nhập cuối
            user.last_login = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            );
            
            Ok(user)
        } else {
            Err(anyhow!("Mật khẩu không đúng"))
        }
    }
    
    // Tạo người dùng mới
    pub fn create_user(&mut self, username: &str, password: &str, email: &str, role: UserRole) -> Result<&User> {
        if self.users.contains_key(username) {
            return Err(anyhow!("Tên người dùng đã tồn tại"));
        }
        
        let user = User {
            id: Uuid::new_v4().to_string(),
            username: username.to_string(),
            password_hash: hash(password, DEFAULT_COST)?,
            email: email.to_string(),
            role,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            last_login: None,
            wallets: Vec::new(),
            active: true,
            subscription: Subscription::new(SubscriptionLevel::Free, 365),
        };
        
        self.users.insert(username.to_string(), user);
        Ok(self.users.get(username).unwrap())
    }
    
    // Cập nhật thông tin người dùng
    pub fn update_user(&mut self, username: &str, email: Option<&str>, role: Option<UserRole>, active: Option<bool>) -> Result<&User> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        
        if let Some(email) = email {
            user.email = email.to_string();
        }
        
        if let Some(role) = role {
            user.role = role;
        }
        
        if let Some(active) = active {
            user.active = active;
        }
        
        Ok(user)
    }
    
    // Đổi mật khẩu
    pub fn change_password(&mut self, username: &str, old_password: &str, new_password: &str) -> Result<()> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        
        if verify(old_password, &user.password_hash)? {
            user.password_hash = hash(new_password, DEFAULT_COST)?;
            Ok(())
        } else {
            Err(anyhow!("Mật khẩu cũ không đúng"))
        }
    }
    
    // Reset mật khẩu (chỉ Admin)
    pub fn reset_password(&mut self, username: &str, new_password: &str) -> Result<()> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        user.password_hash = hash(new_password, DEFAULT_COST)?;
        Ok(())
    }
    
    // Xóa người dùng
    pub fn delete_user(&mut self, username: &str) -> Result<()> {
        if self.users.remove(username).is_none() {
            return Err(anyhow!("Không tìm thấy người dùng"));
        }
        
        Ok(())
    }
    
    // Gán ví cho người dùng
    pub fn assign_wallet(&mut self, username: &str, wallet_address: &str) -> Result<()> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        
        if !user.wallets.contains(&wallet_address.to_string()) {
            user.wallets.push(wallet_address.to_string());
        }
        
        Ok(())
    }
    
    // Gỡ bỏ ví khỏi người dùng
    pub fn unassign_wallet(&mut self, username: &str, wallet_address: &str) -> Result<()> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        user.wallets.retain(|addr| addr != wallet_address);
        Ok(())
    }
    
    // Kiểm tra nếu người dùng có quyền truy cập vào ví
    pub fn has_wallet_access(&self, username: &str, wallet_address: &str) -> bool {
        if let Some(user) = self.users.get(username) {
            // Admin có quyền truy cập vào tất cả các ví
            if user.role == UserRole::Admin {
                return true;
            }
            
            user.wallets.contains(&wallet_address.to_string())
        } else {
            false
        }
    }
    
    // Lấy danh sách người dùng
    pub fn get_all_users(&self) -> Vec<User> {
        self.users.values().cloned().collect()
    }
    
    // Lấy thông tin người dùng theo username
    pub fn get_user(&self, username: &str) -> Option<&User> {
        self.users.get(username)
    }

    // Thêm phương thức để cập nhật subscription
    pub fn update_user_subscription(&mut self, username: &str, level: SubscriptionLevel, duration_days: u64) -> Result<&User> {
        let user = self.users.get_mut(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        
        // Nếu subscription hiện tại vẫn còn hạn, gia hạn thêm
        if user.subscription.is_active() {
            user.subscription.extend(duration_days);
        } else {
            // Nếu hết hạn, tạo mới
            user.subscription = Subscription::new(level, duration_days);
        }
        
        // Cập nhật level nếu cần
        user.subscription.level = level;
        
        Ok(user)
    }
    
    // Lấy thông tin subscription
    pub fn get_user_subscription(&self, username: &str) -> Result<&Subscription> {
        let user = self.users.get(username).ok_or_else(|| anyhow!("Không tìm thấy người dùng"))?;
        Ok(&user.subscription)
    }
}

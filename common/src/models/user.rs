// External imports
use argon2::{
    password_hash::{PasswordHash, PasswordVerifier, SaltString},
    Argon2, PasswordHasher,
};

// Standard library imports
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    time::Duration,
};

// Third party imports
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Vai trò của người dùng
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    /// Người dùng thường
    User,
    /// Quản trị viên
    Admin,
}

impl Display for UserRole {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            UserRole::Admin => write!(f, "Admin"),
            UserRole::User => write!(f, "User"),
        }
    }
}

/// Trạng thái người dùng
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    /// Hoạt động
    Active,
    /// Không hoạt động
    Inactive,
    /// Bị khóa
    Banned,
}

impl Display for UserStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            UserStatus::Active => write!(f, "Active"),
            UserStatus::Inactive => write!(f, "Inactive"),
            UserStatus::Banned => write!(f, "Banned"),
        }
    }
}

/// Thông tin người dùng
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// ID người dùng
    pub id: Uuid,
    /// Tên người dùng
    pub username: String,
    /// Email
    pub email: String,
    /// Hash mật khẩu
    pub password_hash: String,
    /// Vai trò
    pub role: UserRole,
    /// Trạng thái
    pub status: UserStatus,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Thời gian đăng nhập cuối
    pub last_login: Option<DateTime<Utc>>,
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl User {
    /// Tạo người dùng mới
    pub fn new(username: String, email: String, role: UserRole) -> Self {
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        
        let password_hash = argon2
            .hash_password(b"", &salt)
            .unwrap()
            .to_string();
        
        Self {
            id: Uuid::new_v4(),
            username,
            email,
            password_hash,
            role,
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: None,
            metadata: HashMap::new(),
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
            Err(anyhow::anyhow!("Failed to hash password"))
        }
    }

    /// Xác thực mật khẩu
    pub fn verify_password(&self, password: &str) -> Result<bool> {
        if self.password_hash.is_empty() {
            return Err(anyhow::anyhow!("No password hash set"));
        }
        
        let parsed_hash_result = PasswordHash::new(&self.password_hash);
        
        if let Ok(parsed_hash) = parsed_hash_result {
            let argon2 = Argon2::default();
            Ok(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
        } else {
            Err(anyhow::anyhow!("Failed to parse password hash"))
        }
    }

    /// Cập nhật thời gian đăng nhập
    pub fn update_last_login(&mut self) {
        self.last_login = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Cập nhật trạng thái
    pub fn update_status(&mut self, status: UserStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Cập nhật vai trò
    pub fn update_role(&mut self, role: UserRole) {
        self.role = role;
        self.updated_at = Utc::now();
    }

    /// Thêm metadata
    pub fn add_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
        self.updated_at = Utc::now();
    }

    /// Xóa metadata
    pub fn remove_metadata(&mut self, key: &str) {
        self.metadata.remove(key);
        self.updated_at = Utc::now();
    }
}

/// Module tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Test tạo user
    #[test]
    fn test_create_user() {
        let user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );
        assert_eq!(user.username, "test_user");
        assert_eq!(user.email, "test@example.com");
        assert_eq!(user.role, UserRole::User);
        assert_eq!(user.status, UserStatus::Active);
    }

    /// Test cập nhật mật khẩu
    #[test]
    fn test_update_password() {
        let mut user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );
        user.set_password("new_password").unwrap();
        assert!(!user.password_hash.is_empty());
        assert!(user.verify_password("new_password").unwrap());
        assert!(!user.verify_password("wrong_password").unwrap());
    }

    /// Test cập nhật trạng thái
    #[test]
    fn test_update_status() {
        let mut user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );
        user.update_status(UserStatus::Banned);
        assert_eq!(user.status, UserStatus::Banned);
    }

    /// Test cập nhật vai trò
    #[test]
    fn test_update_role() {
        let mut user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );
        user.update_role(UserRole::Admin);
        assert_eq!(user.role, UserRole::Admin);
    }

    /// Test last login
    #[test]
    fn test_last_login() {
        let mut user = User::new(
            "test_user".to_string(),
            "test@example.com".to_string(),
            UserRole::User,
        );

        assert!(user.last_login.is_none());

        let before = Utc::now();
        user.update_last_login();
        let after = Utc::now();

        assert!(user.last_login.is_some());
        let last_login = user.last_login.unwrap();
        assert!(last_login >= before && last_login <= after);
    }
} 
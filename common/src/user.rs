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
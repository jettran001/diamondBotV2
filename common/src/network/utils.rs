use std::time::{SystemTime, UNIX_EPOCH};

/// Lấy thời gian hiện tại dưới dạng timestamp (số giây từ epoch)
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Kiểm tra một địa chỉ có hợp lệ không
pub fn is_valid_address(address: &str) -> bool {
    address.contains(':') && address.split(':').count() == 2
}

/// Tạo một định danh ngẫu nhiên
pub fn random_id() -> String {
    use rand::{thread_rng, Rng};
    let random_bytes: [u8; 16] = thread_rng().gen();
    hex::encode(random_bytes)
}

/// Kiểm tra xem một node có đang online không
pub fn is_node_online(last_heartbeat: u64, timeout_seconds: u64) -> bool {
    let now = current_timestamp();
    now - last_heartbeat < timeout_seconds
}

/// Mã hóa/giải mã đơn giản cho dữ liệu (XOR với key)
pub fn xor_encrypt_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(key.iter().cycle())
        .map(|(d, k)| d ^ k)
        .collect()
} 
use hex;

/// Chuyển đổi chuỗi hex thành vector bytes
pub fn hex_to_bytes(hex_string: &str) -> Result<Vec<u8>, hex::FromHexError> {
    let hex_string = hex_string.trim_start_matches("0x");
    hex::decode(hex_string)
}

/// Chuyển đổi vector bytes thành chuỗi hex
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
} 
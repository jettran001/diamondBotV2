use ethers::types::{Address, U256, H256};
use regex::Regex;
use lazy_static::lazy_static;
use std::str::FromStr;
use log::warn;

/// Định dạng địa chỉ ví thành chuỗi ngắn gọn
pub fn short_address(address: &Address) -> String {
    let address_str = format!("{:?}", address);
    if address_str.len() < 10 {
        return address_str;
    }
    
    format!("{}...{}", &address_str[0..6], &address_str[address_str.len() - 4..])
}

/// Định dạng hash thành chuỗi ngắn gọn
pub fn short_hash(hash: &H256) -> String {
    let hash_str = format!("{:?}", hash);
    if hash_str.len() < 10 {
        return hash_str;
    }
    
    format!("{}...{}", &hash_str[0..10], &hash_str[hash_str.len() - 6..])
}

/// Định dạng số lượng token với số chữ số thập phân
pub fn format_token_amount(amount: U256, decimals: u8, display_decimals: usize) -> String {
    if amount.is_zero() {
        return "0".to_string();
    }
    
    // Chuyển đổi amount sang chuỗi decimal
    let amount_str = amount.to_string();
    let amount_len = amount_str.len();
    
    if amount_len <= decimals as usize {
        // Số lượng nhỏ hơn 1 token
        let padding_zeros = decimals as usize - amount_len;
        let decimal_part = format!("{}{}", "0".repeat(padding_zeros), amount_str);
        
        // Lấy số chữ số thập phân cần hiển thị
        let formatted_decimal = if decimal_part.len() > display_decimals {
            &decimal_part[0..display_decimals]
        } else {
            &decimal_part
        };
        
        format!("0.{}", formatted_decimal)
    } else {
        // Số lượng lớn hơn 1 token
        let integer_part = &amount_str[0..(amount_len - decimals as usize)];
        let decimal_part = &amount_str[(amount_len - decimals as usize)..];
        
        // Lấy số chữ số thập phân cần hiển thị
        let formatted_decimal = if decimal_part.len() > display_decimals {
            &decimal_part[0..display_decimals]
        } else {
            decimal_part
        };
        
        if display_decimals > 0 && !formatted_decimal.is_empty() && formatted_decimal != "0".repeat(formatted_decimal.len()) {
            format!("{}.{}", integer_part, formatted_decimal)
        } else {
            integer_part.to_string()
        }
    }
}

/// Định dạng số ETH từ wei
pub fn format_eth(wei_amount: U256, display_decimals: usize) -> String {
    format_token_amount(wei_amount, 18, display_decimals)
}

/// Định dạng giá token so với ETH
pub fn format_price_eth(price: f64, display_decimals: usize) -> String {
    format!("{:.1$} ETH", price, display_decimals)
}

/// Định dạng giá token so với USD
pub fn format_price_usd(price: f64, display_decimals: usize) -> String {
    format!("${:.1$}", price, display_decimals)
}

/// Định dạng phần trăm
pub fn format_percentage(value: f64, display_decimals: usize) -> String {
    format!("{:.1$}%", value, display_decimals)
}

/// Chuyển chuỗi thành địa chỉ
pub fn parse_address(address_str: &str) -> Option<Address> {
    lazy_static! {
        static ref ADDRESS_REGEX: Regex = Regex::new(r"^(0x)?[0-9a-fA-F]{40}$").unwrap();
    }
    
    if !ADDRESS_REGEX.is_match(address_str) {
        warn!("Địa chỉ không hợp lệ: {}", address_str);
        return None;
    }
    
    let formatted_address = if !address_str.starts_with("0x") {
        format!("0x{}", address_str)
    } else {
        address_str.to_string()
    };
    
    match Address::from_str(&formatted_address) {
        Ok(address) => Some(address),
        Err(e) => {
            warn!("Không thể phân tích địa chỉ: {}, lỗi: {}", address_str, e);
            None
        }
    }
}

/// Chuyển chuỗi thành hash
pub fn parse_hash(hash_str: &str) -> Option<H256> {
    lazy_static! {
        static ref HASH_REGEX: Regex = Regex::new(r"^(0x)?[0-9a-fA-F]{64}$").unwrap();
    }
    
    if !HASH_REGEX.is_match(hash_str) {
        warn!("Hash không hợp lệ: {}", hash_str);
        return None;
    }
    
    let formatted_hash = if !hash_str.starts_with("0x") {
        format!("0x{}", hash_str)
    } else {
        hash_str.to_string()
    };
    
    match H256::from_str(&formatted_hash) {
        Ok(hash) => Some(hash),
        Err(e) => {
            warn!("Không thể phân tích hash: {}, lỗi: {}", hash_str, e);
            None
        }
    }
}

/// Định dạng thông báo lỗi ngắn gọn
pub fn format_error(error: &str) -> String {
    if error.len() <= 100 {
        return error.to_string();
    }
    
    // Tìm thông báo lỗi cuối cùng (thường là thông báo quan trọng nhất)
    if let Some(last_error_idx) = error.rfind("error:") {
        let last_error = &error[last_error_idx..];
        if last_error.len() <= 100 {
            return last_error.to_string();
        }
        return format!("{}...", &last_error[0..97]);
    }
    
    // Nếu không tìm thấy, trả về 100 ký tự đầu tiên
    format!("{}...", &error[0..97])
}

/// Định dạng chuỗi theo độ dài cụ thể
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    
    let half_len = (max_len - 3) / 2;
    format!("{}...{}", &s[0..half_len], &s[s.len() - half_len..])
}

/// Định dạng số lượng lớn thành dạng K, M, B
pub fn format_large_number(number: f64) -> String {
    if number < 1_000.0 {
        return format!("{:.2}", number);
    } else if number < 1_000_000.0 {
        return format!("{:.2}K", number / 1_000.0);
    } else if number < 1_000_000_000.0 {
        return format!("{:.2}M", number / 1_000_000.0);
    } else {
        return format!("{:.2}B", number / 1_000_000_000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_short_address() {
        let address = Address::from_str("0x1234567890abcdef1234567890abcdef12345678").unwrap();
        assert_eq!(short_address(&address), "0x1234...5678");
    }
    
    #[test]
    fn test_format_token_amount() {
        let amount = U256::from_dec_str("1000000000000000000").unwrap(); // 1 ETH
        assert_eq!(format_token_amount(amount, 18, 2), "1.00");
        
        let amount = U256::from_dec_str("1234567890123456789").unwrap();
        assert_eq!(format_token_amount(amount, 18, 4), "1.2345");
        
        let amount = U256::from_dec_str("123456789").unwrap();
        assert_eq!(format_token_amount(amount, 18, 6), "0.000123");
    }
    
    #[test]
    fn test_format_eth() {
        let wei = U256::from_dec_str("1000000000000000000").unwrap(); // 1 ETH
        assert_eq!(format_eth(wei, 2), "1.00");
        
        let wei = U256::from_dec_str("500000000000000000").unwrap(); // 0.5 ETH
        assert_eq!(format_eth(wei, 1), "0.5");
    }
    
    #[test]
    fn test_parse_address() {
        let address_str = "0x1234567890abcdef1234567890abcdef12345678";
        let address = parse_address(address_str).unwrap();
        assert_eq!(address.to_string(), "0x1234567890abcdef1234567890abcdef12345678".to_lowercase());
        
        let address_str = "1234567890abcdef1234567890abcdef12345678";
        let address = parse_address(address_str).unwrap();
        assert_eq!(address.to_string(), "0x1234567890abcdef1234567890abcdef12345678".to_lowercase());
        
        let address_str = "invalid";
        assert_eq!(parse_address(address_str), None);
    }
    
    #[test]
    fn test_format_large_number() {
        assert_eq!(format_large_number(123.45), "123.45");
        assert_eq!(format_large_number(1234.56), "1.23K");
        assert_eq!(format_large_number(1234567.89), "1.23M");
        assert_eq!(format_large_number(1234567890.12), "1.23B");
    }
} 
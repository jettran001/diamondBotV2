use std::time::{SystemTime, UNIX_EPOCH};
use ethers::types::U256;
use num_traits::cast::ToPrimitive;
use log::warn;

pub mod format;
pub mod time;
pub mod math;

/// Hàm lấy thời gian hiện tại một cách an toàn
pub fn safe_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Chuyển đổi Wei thành ETH
pub fn wei_to_eth(wei: U256) -> f64 {
    // 1 ETH = 10^18 Wei
    if wei.is_zero() {
        return 0.0;
    }
    
    let wei_f64 = wei.to_f64().unwrap_or_else(|| {
        warn!("Không thể chuyển đổi U256 thành f64: {:?}", wei);
        0.0
    });
    
    wei_f64 / 1e18
}

/// Phân tích chuỗi số thành U256 với số lượng decimals phù hợp
pub fn parse_decimal_u256(amount: String, decimals: u32) -> U256 {
    let parts: Vec<&str> = amount.split('.').collect();
    
    match parts.len() {
        1 => {
            // Không có phần thập phân, thêm decimals số 0
            let mut amount_str = parts[0].to_string();
            amount_str.push_str(&"0".repeat(decimals as usize));
            U256::from_dec_str(&amount_str).unwrap_or_else(|_| {
                warn!("Không thể chuyển đổi chuỗi thành U256: {}", amount_str);
                U256::zero()
            })
        },
        2 => {
            // Có phần thập phân
            let integer_part = parts[0];
            let mut decimal_part = parts[1].to_string();
            
            // Nếu phần thập phân dài hơn decimals, cắt bớt
            if decimal_part.len() > decimals as usize {
                decimal_part = decimal_part[0..decimals as usize].to_string();
            } else if decimal_part.len() < decimals as usize {
                // Nếu phần thập phân ngắn hơn decimals, thêm số 0
                decimal_part.push_str(&"0".repeat(decimals as usize - decimal_part.len()));
            }
            
            let amount_str = format!("{}{}", integer_part, decimal_part);
            U256::from_dec_str(&amount_str).unwrap_or_else(|_| {
                warn!("Không thể chuyển đổi chuỗi thành U256: {}", amount_str);
                U256::zero()
            })
        },
        _ => {
            warn!("Chuỗi số không hợp lệ: {}", amount);
            U256::zero()
        }
    }
}

/// Định dạng địa chỉ thành chuỗi ngắn gọn
pub fn format_address(address: &str) -> String {
    if address.len() < 10 {
        return address.to_string();
    }
    
    let start = &address[0..6];
    let end = &address[address.len() - 4..];
    format!("{}...{}", start, end)
}

/// Kiểm tra địa chỉ Ethereum hợp lệ
pub fn is_valid_eth_address(address: &str) -> bool {
    // Địa chỉ Ethereum bắt đầu bằng 0x và có độ dài 42 ký tự (2 cho 0x + 40 cho địa chỉ hex)
    if !address.starts_with("0x") || address.len() != 42 {
        return false;
    }
    
    // Kiểm tra xem tất cả các ký tự còn lại có phải là hex không
    address[2..].chars().all(|c| c.is_digit(16))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wei_to_eth() {
        let wei = U256::from_dec_str("1000000000000000000").unwrap(); // 1 ETH
        assert_eq!(wei_to_eth(wei), 1.0);
        
        let wei = U256::from_dec_str("500000000000000000").unwrap(); // 0.5 ETH
        assert_eq!(wei_to_eth(wei), 0.5);
        
        let wei = U256::zero();
        assert_eq!(wei_to_eth(wei), 0.0);
    }
    
    #[test]
    fn test_parse_decimal_u256() {
        let amount = "1.0".to_string();
        let decimals = 18;
        let wei = parse_decimal_u256(amount, decimals);
        assert_eq!(wei, U256::from_dec_str("1000000000000000000").unwrap());
        
        let amount = "0.5".to_string();
        let wei = parse_decimal_u256(amount, decimals);
        assert_eq!(wei, U256::from_dec_str("500000000000000000").unwrap());
        
        let amount = "1".to_string();
        let wei = parse_decimal_u256(amount, decimals);
        assert_eq!(wei, U256::from_dec_str("1000000000000000000").unwrap());
    }
    
    #[test]
    fn test_format_address() {
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        assert_eq!(format_address(address), "0x1234...5678");
        
        let address = "0x123";
        assert_eq!(format_address(address), "0x123");
    }
    
    #[test]
    fn test_is_valid_eth_address() {
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        assert!(is_valid_eth_address(address));
        
        let address = "0x123";
        assert!(!is_valid_eth_address(address));
        
        let address = "0xG234567890abcdef1234567890abcdef12345678"; // Chứa ký tự không hợp lệ
        assert!(!is_valid_eth_address(address));
    }
} 
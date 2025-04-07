use ethers::types::U256;
use num_traits::cast::ToPrimitive;
use log::warn;
use std::cmp::{max, min};

/// Tính trung bình của dãy số
pub fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    let sum: f64 = values.iter().sum();
    sum / values.len() as f64
}

/// Tính trung vị của dãy số
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// Tính độ lệch chuẩn của dãy số
pub fn standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    let avg = average(values);
    let variance = values.iter()
        .map(|value| {
            let diff = avg - *value;
            diff * diff
        })
        .sum::<f64>() / values.len() as f64;
    
    variance.sqrt()
}

/// Tính tỷ lệ phần trăm thay đổi giữa hai giá trị
pub fn percentage_change(old_value: f64, new_value: f64) -> f64 {
    if old_value == 0.0 {
        return if new_value == 0.0 { 0.0 } else { 100.0 };
    }
    
    ((new_value - old_value) / old_value) * 100.0
}

/// Tính tỷ lệ phần trăm giữa hai giá trị
pub fn percentage_ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        return 0.0;
    }
    
    (numerator / denominator) * 100.0
}

/// Tính giá trị an toàn U256
pub fn safe_add_u256(a: U256, b: U256) -> U256 {
    match a.checked_add(b) {
        Some(result) => result,
        None => {
            warn!("Phép cộng U256 bị tràn: {:?} + {:?}", a, b);
            U256::max_value()
        }
    }
}

/// Tính giá trị an toàn U256
pub fn safe_sub_u256(a: U256, b: U256) -> U256 {
    match a.checked_sub(b) {
        Some(result) => result,
        None => {
            warn!("Phép trừ U256 bị tràn: {:?} - {:?}", a, b);
            U256::zero()
        }
    }
}

/// Tính giá trị trung bình của dãy U256
pub fn average_u256(values: &[U256]) -> U256 {
    if values.is_empty() {
        return U256::zero();
    }
    
    let mut sum = U256::zero();
    for value in values {
        sum = safe_add_u256(sum, *value);
    }
    
    sum / U256::from(values.len())
}

/// Tính moving average cho dãy giá trị
pub fn moving_average(values: &[f64], window: usize) -> Vec<f64> {
    if values.is_empty() || window == 0 || window > values.len() {
        return Vec::new();
    }
    
    let mut result = Vec::with_capacity(values.len() - window + 1);
    let mut sum = values.iter().take(window).sum::<f64>();
    
    result.push(sum / window as f64);
    
    for i in window..values.len() {
        sum = sum - values[i - window] + values[i];
        result.push(sum / window as f64);
    }
    
    result
}

/// Tính moving median cho dãy giá trị
pub fn moving_median(values: &[f64], window: usize) -> Vec<f64> {
    if values.is_empty() || window == 0 || window > values.len() {
        return Vec::new();
    }
    
    let mut result = Vec::with_capacity(values.len() - window + 1);
    
    for i in 0..=(values.len() - window) {
        let window_values = &values[i..(i + window)];
        result.push(median(window_values));
    }
    
    result
}

/// Tính giới hạn trên và dưới cho giá trị theo phần trăm
pub fn calculate_price_bounds(current_price: f64, percent: f64) -> (f64, f64) {
    let factor = percent / 100.0;
    let lower_bound = current_price * (1.0 - factor);
    let upper_bound = current_price * (1.0 + factor);
    (lower_bound, upper_bound)
}

/// Kiểm tra giá trị nằm trong phạm vi
pub fn is_within_range(value: f64, min_value: f64, max_value: f64) -> bool {
    value >= min_value && value <= max_value
}

/// Cắt giá trị theo phạm vi min max
pub fn clamp(value: f64, min_value: f64, max_value: f64) -> f64 {
    if value < min_value {
        min_value
    } else if value > max_value {
        max_value
    } else {
        value
    }
}

/// Tính số gas tối ưu dựa trên lợi nhuận kỳ vọng và chi phí gas
pub fn optimize_gas(expected_profit: f64, base_gas_cost: u64, gas_price: f64) -> u64 {
    // Nếu không có lợi nhuận kỳ vọng hoặc gas_price = 0, trả về gas tối thiểu
    if expected_profit <= 0.0 || gas_price <= 0.0 {
        return base_gas_cost;
    }
    
    // Chuyển đổi lợi nhuận kỳ vọng sang đơn vị gas
    let profit_in_gas = (expected_profit / gas_price) as u64;
    
    // Giới hạn gas tối đa là lợi nhuận kỳ vọng / 2 (để đảm bảo lợi nhuận)
    let max_gas = base_gas_cost + profit_in_gas / 2;
    
    // Trả về gas tối ưu, giới hạn không vượt quá max_gas
    min(max_gas, base_gas_cost * 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_average() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(average(&values), 3.0);
        
        let empty: Vec<f64> = vec![];
        assert_eq!(average(&empty), 0.0);
    }
    
    #[test]
    fn test_median() {
        let values = vec![1.0, 3.0, 5.0, 7.0, 9.0];
        assert_eq!(median(&values), 5.0);
        
        let values = vec![1.0, 3.0, 5.0, 7.0];
        assert_eq!(median(&values), 4.0);
        
        let empty: Vec<f64> = vec![];
        assert_eq!(median(&empty), 0.0);
    }
    
    #[test]
    fn test_standard_deviation() {
        let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((standard_deviation(&values) - 2.0).abs() < 0.001);
        
        let empty: Vec<f64> = vec![];
        assert_eq!(standard_deviation(&empty), 0.0);
    }
    
    #[test]
    fn test_percentage_change() {
        assert_eq!(percentage_change(100.0, 120.0), 20.0);
        assert_eq!(percentage_change(100.0, 80.0), -20.0);
        assert_eq!(percentage_change(0.0, 100.0), 100.0);
        assert_eq!(percentage_change(0.0, 0.0), 0.0);
    }
    
    #[test]
    fn test_moving_average() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ma = moving_average(&values, 3);
        assert_eq!(ma, vec![2.0, 3.0, 4.0]);
        
        let empty: Vec<f64> = vec![];
        let ma = moving_average(&empty, 3);
        assert_eq!(ma, Vec::<f64>::new());
    }
    
    #[test]
    fn test_optimize_gas() {
        // Lợi nhuận 0.1 ETH, chi phí gas cơ bản 21000, giá gas 50 Gwei
        let optimal_gas = optimize_gas(0.1, 21000, 0.00000005);
        assert!(optimal_gas > 21000);
        
        // Không có lợi nhuận kỳ vọng
        let optimal_gas = optimize_gas(0.0, 21000, 0.00000005);
        assert_eq!(optimal_gas, 21000);
    }
} 
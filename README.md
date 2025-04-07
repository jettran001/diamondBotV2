# Snipebot - Bot giao dịch DeFi thông minh

Snipebot là một bot giao dịch DeFi mạnh mẽ được xây dựng bằng Rust, thiết kế để thực hiện các giao dịch tự động và an toàn trên nhiều blockchain khác nhau, bao gồm Ethereum, BSC, và các EVM-compatible chains khác. Với kiến trúc linh hoạt, Snipebot cung cấp khả năng theo dõi thị trường, phân tích cơ hội, và thực hiện giao dịch một cách hiệu quả.

## Tính năng chính

- **Hỗ trợ đa chuỗi (Multi-chain)**: Giao dịch trên nhiều blockchain với cấu trúc Chain Adapter linh hoạt.
- **Quản lý ví an toàn**: Lưu trữ và quản lý ví với mã hóa mạnh mẽ và bảo vệ dữ liệu nhạy cảm.
- **Theo dõi thị trường**: Theo dõi giá và thanh khoản tự động.
- **Giao dịch thông minh**: Tối ưu gas, chọn thời điểm tốt nhất để mua/bán.
- **Cơ chế thử lại mạnh mẽ**: Xử lý lỗi mạng bằng circuit breaker và retry policies.
- **RESTful API**: Quản lý bot từ xa thông qua API đơn giản.
- **Giao diện người dùng**: Frontend để quản lý và giám sát bot.

## Cấu trúc dự án

```
snipebot/
├── common/           # Thư viện chung và utilities
├── snipebot/         # Core logic của bot
├── wallet/           # Quản lý ví an toàn
├── blockchain/       # Kết nối và tương tác blockchain,Quản lý hợp đồng Diamond (EIP-2535)
├── network/          # Quản lý kết nối RPC
├── diamond_manager/  # Quản trị hệ thống
├── frontend/         # Giao diện người dùng web
└── AI/               # Phân tích thị trường bằng AI 
```

## Cài đặt và chạy

### Yêu cầu hệ thống

- Rust 1.68.0+
- Node.js 16+ (cho frontend)
- Database (MongoDB hoặc PostgreSQL)

### Cài đặt

1. **Clone repository:**
   ```bash
   git clone https://github.com/your-username/snipebot.git
   cd snipebot
   ```

2. **Xây dựng project:**
   ```bash
   cargo build --release
   ```

3. **Cài đặt frontend:**
   ```bash
   cd frontend
   npm install
   npm run build
   ```

4. **Cấu hình:**
   Sao chép `config.example.toml` thành `config.toml` và điều chỉnh cài đặt.

5. **Chạy bot:**
   ```bash
   ./target/release/snipebot
   ```

## Kiến trúc hệ thống

### Chain Adapter

Snipebot sử dụng hệ thống ChainAdapter để tương tác với các blockchain khác nhau. Kiến trúc trait-based cho phép dễ dàng mở rộng sang các blockchain mới:

```rust
// Ví dụ về ChainAdapter interface
pub trait ChainAdapter: Send + Sync + 'static {
    fn get_chain_id(&self) -> u64;
    fn get_type(&self) -> &str;
    async fn get_block_number(&self) -> Result<u64, ChainError>;
    async fn get_gas_price(&self) -> Result<U256, ChainError>;
    // Các phương thức khác...
}
```

### Quản lý ví an toàn

WalletManager cung cấp lưu trữ an toàn cho các ví, sử dụng mã hóa mạnh mẽ để bảo vệ private keys:

```rust
// Ví dụ về sử dụng WalletManager
let wallet_config = WalletConfig::default();
let wallet_manager = WalletManager::new(wallet_config)?;

// Tạo ví mới
let (phrase, address) = wallet_manager.create_wallet(None)?;

// Nhập ví từ private key
let address = wallet_manager.import_from_private_key("0x...")?;
```

### Retry Policy

Snipebot sử dụng cơ chế thử lại thông minh để đảm bảo tính ổn định trong môi trường blockchain bất định:

```rust
// Ví dụ về RetryPolicy
let retry_policy = create_default_retry_policy();

// Sử dụng retry cho RPC calls
let result = retry_policy.retry(
    || async { adapter.get_block_number().await },
    &RetryContext::new("get_block", "http://example.com", 1, None),
).await?;
```

### API và giao diện

Snipebot cung cấp REST API để quản lý từ xa:

- `/api/wallet`: Quản lý ví và giao dịch
- `/api/bot`: Kiểm soát hành vi bot
- `/api/admin`: Chức năng quản trị viên
- `/api/market`: Dữ liệu thị trường

## Sử dụng nâng cao

### Tùy chỉnh ChainAdapter

Để hỗ trợ một blockchain mới:

1. Triển khai trait `ChainAdapter` cho blockchain mới
2. Đăng ký adapter với `ChainRegistry`
3. Cấu hình các endpoints và parameters trong cài đặt

### Chiến lược giao dịch tùy chỉnh

Snipebot cho phép tạo chiến lược giao dịch riêng:

1. Tạo struct mới triển khai `TradingStrategy` trait
2. Đăng ký chiến lược với `StrategyManager`
3. Thiết lập parameters và triggers

## Đóng góp

Chúng tôi hoan nghênh mọi đóng góp! Hãy xem [CONTRIBUTING.md](CONTRIBUTING.md) để biết hướng dẫn.

## Giấy phép

Snipebot được phân phối theo giấy phép MIT. Xem file [LICENSE](LICENSE) để biết thêm chi tiết.

## Liên hệ

Để được hỗ trợ hoặc có thắc mắc, vui lòng tạo issue hoặc liên hệ [email@example.com](mailto:email@example.com). 
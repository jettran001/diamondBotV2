# Diamond Chain Workspace

Workspace này tổ chức tất cả các thành phần của Diamond Chain thành một dự án Rust thống nhất.

## Cấu trúc Workspace

Workspace bao gồm các crate sau:

- `common`: Chứa mã chung, kiểu dữ liệu, middleware và tiện ích được sử dụng bởi các crate khác
- `blockchain`: Cung cấp hàm và đối tượng tương tác với blockchain
- `wallet`: Quản lý ví và các giao dịch liên quan đến tài sản
- `snipebot`: Bot giao dịch tự động trên DEX
- `network/common`: Các thành phần chung cho mạng nội bộ
- `network/wasm`: Phiên bản WebAssembly của mã giao tiếp mạng

## Quản lý phụ thuộc

Các phụ thuộc chung được quản lý tại cấp workspace để đảm bảo các phiên bản nhất quán trên tất cả các crate. Điều này giúp tránh các xung đột phiên bản và tối ưu hóa quá trình biên dịch.

## Cách sử dụng

### Biên dịch toàn bộ workspace

```
cargo build
```

### Biên dịch một crate cụ thể

```
cargo build -p diamond_wallet
```

### Chạy tests

```
cargo test
```

### Chạy ứng dụng chính (snipebot)

```
cargo run -p snipebot
```

## Các tính năng sắp tới

- Tích hợp CI/CD
- Thêm tài liệu API
- Thêm các công cụ benchmark
- Tự động hóa kiểm tra lỗi

## Quy tắc đóng góp

Khi đóng góp vào dự án, hãy cân nhắc những điều sau:

1. Sử dụng features từ workspace cho những phụ thuộc chung
2. Tránh tạo bản sao của mã thông qua các crate
3. Đặt mã chung vào crate `common`
4. Tuân thủ quy ước đặt tên và định dạng mã 
# Hướng dẫn di chuyển từ cấu trúc network cũ sang common

Chúng ta đã tích hợp các module `network/common` và `network/server` vào thư mục `common` để tạo một điểm kiểm soát trung tâm và cổng kết nối chính cho các node. Dưới đây là hướng dẫn cách di chuyển từ cấu trúc cũ sang cấu trúc mới.

## Cập nhật Cargo.toml

Trong các crates sử dụng `network/common` hoặc `network/server`, cần cập nhật dependencies để sử dụng `diamond_common`:

```toml
[dependencies]
diamond_common = { workspace = true }
# Xóa diamond_network_common nếu có
```

## Cập nhật imports

### Từ:

```rust
use diamond_network_common::{NetworkConfig, NetworkError, Message};
```

### Đến:

```rust
use diamond_common::{NetworkConfig, NetworkError, Message};
```

Hoặc chi tiết hơn:

```rust
use diamond_common::network::{
    NetworkConfig, NetworkError, Message,
    server::{WebSocketServer, QuicServer}
};
```

## Cập nhật mã trong module blockchain, wallet, snipebot

Nếu các module này sử dụng trực tiếp mã từ `network/common` hoặc `network/server`, cần cập nhật các imports và các đường dẫn tương ứng.

### Ví dụ:

```rust
// Cũ
use crate::network_common::models::Node;

// Mới
use diamond_common::network::models::Node;
// hoặc
use diamond_common::Node;
```

## Kiểm tra lỗi biên dịch

Sau khi cập nhật, cần chạy lệnh sau để kiểm tra lỗi biên dịch:

```bash
cargo check
```

Và sửa các lỗi một cách tương ứng.

## Lưu ý quan trọng

1. Cấu trúc module đã thay đổi, nhưng tên các components vẫn giữ nguyên, do đó chỉ cần cập nhật đường dẫn import.

2. Một số tính năng có thể yêu cầu features. Ví dụ, để sử dụng các tính năng liên quan đến blockchain, cần bật feature "blockchain":

```toml
[dependencies]
diamond_common = { workspace = true, features = ["blockchain"] }
```

3. Các file cấu hình và cấu trúc dữ liệu được giữ nguyên, chỉ có đường dẫn import thay đổi. 
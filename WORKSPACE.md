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

## Sử dụng cache chung

Tất cả các thành phần trong dự án nên sử dụng hệ thống cache thống nhất từ module `common::cache`. Module này cung cấp các triển khai cache khác nhau với một API thống nhất.

### Cách sử dụng

#### 1. Import module cache

```rust
// Import trait chính và các kiểu dữ liệu
use common::cache::{Cache, CacheEntry, CacheConfig};

// Import triển khai cụ thể nếu cần
use common::cache::{BasicCache, LRUCache, JSONCache, RedisCache};
```

#### 2. Triển khai trait `Cache` cho các struct tùy chỉnh

```rust
use async_trait::async_trait;
use common::cache::{Cache, CacheEntry};
use anyhow::Result;
use serde::{Serialize, Deserialize};

#[async_trait]
impl Cache for MyCustomService {
    async fn get_from_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str) -> Result<Option<T>> {
        // Triển khai lấy dữ liệu từ cache
        self.internal_cache.get_from_cache(key).await
    }

    async fn store_in_cache<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(&self, key: &str, value: T, ttl_seconds: u64) -> Result<()> {
        // Triển khai lưu dữ liệu vào cache
        self.internal_cache.store_in_cache(key, value, ttl_seconds).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        // Triển khai xóa dữ liệu khỏi cache
        self.internal_cache.remove(key).await
    }

    async fn clear(&self) -> Result<()> {
        // Triển khai xóa toàn bộ cache
        self.internal_cache.clear().await
    }

    async fn cleanup_cache(&self) -> Result<()> {
        // Triển khai dọn dẹp cache
        self.internal_cache.cleanup_cache().await
    }
}
```

#### 3. Sử dụng các triển khai cache có sẵn

```rust
// BasicCache - Cache đơn giản dựa trên HashMap
let basic_cache = BasicCache::default();

// LRUCache - Cache với cơ chế Least Recently Used
let lru_cache = LRUCache::with_capacity(1000)?;

// JSONCache - Cache đặc biệt cho dữ liệu JSON
let json_cache = JSONCache::new(config);

// RedisCache - Cache sử dụng Redis
let redis_config = CacheConfig {
    redis_url: Some("redis://localhost:6379".to_string()),
    ..CacheConfig::default()
};
let redis_cache = RedisCache::new(redis_config)?;
```

#### 4. Ví dụ lưu và lấy dữ liệu

```rust
// Lưu dữ liệu vào cache với TTL 300 giây
let data = MyData { /* ... */ };
cache.store_in_cache("my_key", data, 300).await?;

// Lấy dữ liệu từ cache
if let Some(data) = cache.get_from_cache::<MyData>("my_key").await? {
    // Xử lý dữ liệu...
} else {
    // Dữ liệu không tồn tại hoặc đã hết hạn
}
```

### Lưu ý quan trọng

- Module `snipebot::cache` đã được đánh dấu là deprecated, vui lòng sử dụng `common::cache` thay thế
- Luôn xử lý lỗi trả về từ các phương thức cache
- Sử dụng TTL phù hợp cho từng loại dữ liệu
- Thực hiện `cleanup_cache()` định kỳ để tránh rò rỉ bộ nhớ

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
5. Sử dụng các module chung từ `common` thay vì tạo triển khai riêng 

## Modules và Imports
- Luôn sử dụng `common::cache` cho các chức năng cache
- Luôn sử dụng `common::middleware` cho các chức năng middleware 
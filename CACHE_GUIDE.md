# Hướng dẫn sử dụng Cache

## Import

```rust
// Import cache từ common
use common::cache::{Cache, CacheConfig, BasicCache};
```

## Giới thiệu
Cache là một thành phần quan trọng để tối ưu hiệu năng ứng dụng DiamondChain. Module này cung cấp 
các API thống nhất để lưu trữ và truy xuất dữ liệu tạm thời với thời gian hết hạn tùy chỉnh.

## Các loại Cache

### BasicCache
`BasicCache` là implementation cơ bản nhất của trait `Cache`, sử dụng `HashMap` để lưu trữ dữ liệu trong bộ nhớ.

```rust
use common::cache::{Cache, BasicCache};

// Tạo cache mới với cấu hình mặc định
let cache = BasicCache::default();

// Lưu trữ giá trị với key
cache.set("my_key", "my_value", 60).await?;

// Lấy giá trị với key
let value = cache.get("my_key").await?;
assert_eq!(value, Some("my_value"));

// Xóa giá trị với key
cache.remove("my_key").await?;

// Kiểm tra giá trị đã bị xóa
let value = cache.get("my_key").await?;
assert_eq!(value, None);
```

### JSONCache
`JSONCache` là một wrapper xung quanh implementation khác của `Cache`, hỗ trợ lưu trữ và truy xuất 
dữ liệu dưới dạng JSON.

```rust
use common::cache::{Cache, BasicCache, JSONCache};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct User {
    id: u64,
    name: String,
}

// Tạo cache mới
let cache = JSONCache::new(BasicCache::default());

// Tạo đối tượng User
let user = User {
    id: 1,
    name: "Alice".to_string(),
};

// Lưu trữ đối tượng User với key
cache.set("user:1", user.clone(), 300).await?;

// Lấy đối tượng User với key
let retrieved_user: Option<User> = cache.get("user:1").await?;
assert_eq!(retrieved_user, Some(user));
```

## Cấu hình Cache

Bạn có thể tùy chỉnh hành vi cache bằng cách sử dụng `CacheConfig`:

```rust
use common::cache::{CacheConfig, BasicCache};

// Tạo cấu hình với thời gian hết hạn mặc định là 5 phút
let config = CacheConfig {
    default_ttl: 300,     // 5 phút
    max_size: 10000,      // 10K phần tử
    cleanup_interval: 3600, // 1 giờ
};

// Tạo cache với cấu hình tùy chỉnh
let cache = BasicCache::new(Some(config));
```

## Auto-cleanup

Cache tự động xóa các mục đã hết hạn khi một thao tác được thực hiện. Ngoài ra, bạn có thể chủ động xóa các mục đã hết hạn:

```rust
use common::cache::{Cache, BasicCache};

let cache = BasicCache::default();

// Xóa tất cả các mục đã hết hạn
cache.cleanup().await?;
```

## Giới hạn kích thước Cache
Để giới hạn kích thước cache, hãy sử dụng cấu hình max_size:

```rust
use common::cache::{CacheConfig, BasicCache};

// Cấu hình với kích thước tối đa
let config = CacheConfig {
    max_size: 1000,
    ..Default::default()
};

let cache = BasicCache::new(Some(config));
// Cache sẽ tự động loại bỏ các entry cũ nhất khi đạt kích thước tối đa
```

## AsyncCache

Cho các trường hợp yêu cầu thread-safety cao với async/await:

```rust
use common::cache::{AsyncCache, CacheConfig};

// Tạo AsyncCache với cấu hình mặc định
let cache = AsyncCache::new(None);

// Hoặc với cấu hình tùy chỉnh
let config = CacheConfig {
    default_ttl: 300,
    max_size: 5000,
    cleanup_interval: 1800,
};
let cache = AsyncCache::new(Some(config));

// Sử dụng như BasicCache nhưng với việc quản lý lock tự động tốt hơn
// cho môi trường async/await
let value = "Async value".to_string();
cache.set("async_key", value.clone(), 120).await?;
let result: Option<String> = cache.get("async_key").await?;
assert_eq!(result, Some(value));
``` 
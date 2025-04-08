# Module Network trong Diamond Common

Module này là trung tâm điều khiển các kết nối mạng cho Diamond Chain. Nó được tích hợp từ `network/common` và `network/server` để tạo một điểm kiểm soát trung tâm và cổng kết nối chính cho các node.

## Cấu trúc

```
common/src/network/
├── server/             # Các thành phần máy chủ
│   ├── websocket_server.rs  # WebSocket server
│   ├── websocket.rs         # WebSocket client
│   ├── quic.rs              # QUIC server/client
│   ├── grpc.rs              # gRPC server
│   ├── redis_service.rs     # Dịch vụ Redis
│   └── mod.rs               # Module exports
├── config.rs           # Cấu hình mạng
├── error.rs            # Xử lý lỗi
├── message.rs          # Định nghĩa message
├── models.rs           # Các model dùng chung
├── types.rs            # Các kiểu dữ liệu
├── utils.rs            # Các tiện ích
└── lib.rs              # Module exports
```

## Cách sử dụng

Để sử dụng các thành phần của module này, bạn chỉ cần import từ `diamond_common`:

```rust
use diamond_common::{
    // Các types chung
    NetworkConfig, NetworkError, NetworkResult, Node, NodeType,
    
    // Các server components
    WebSocketServer, WebSocketClient, QuicServer, QuicClient,
    SnipeBotGrpcService, RedisService
};

// Hoặc import cụ thể từ module network
use diamond_common::network::server::WebSocketServer;
```

## Ví dụ

### Tạo WebSocket Server

```rust
use diamond_common::WebSocketServer;

async fn main() {
    let jwt_secret = "your-secret-key".to_string();
    let ws_server = WebSocketServer::new(jwt_secret);
    
    // Xử lý kết nối mới
    let socket = get_socket_somehow().await;
    ws_server.handle_socket(socket, None).await;
}
```

### Tạo QUIC Server

```rust
use diamond_common::{NetworkConfig, QuicServer};

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfig::default();
    let mut quic_server = QuicServer::new(&config).await?;
    quic_server.start().await?;
    Ok(())
}
```

### Tạo gRPC Server

```rust
use diamond_common::SnipeBotGrpcService;

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grpc_service = SnipeBotGrpcService::new();
    grpc_service.start("0.0.0.0:50051").await?;
    Ok(())
}
```

### Sử dụng Redis Service

```rust
use diamond_common::RedisService;

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let redis_service = RedisService::new("redis://localhost:6379").await?;
    redis_service.set("key", "value").await?;
    let value = redis_service.get("key").await?;
    println!("Value: {}", value);
    Ok(())
}
```

## Lưu ý

1. Các server nên được cấu hình với các tham số phù hợp:
   - WebSocket: JWT secret, max connections, ping interval
   - QUIC: Certificate, private key, max connections
   - gRPC: Port, max connections, timeout
   - Redis: URL, pool size, timeout

2. Các server nên được khởi tạo và quản lý trong một context phù hợp:
   - Sử dụng Arc<RwLock> cho shared state
   - Sử dụng tokio::spawn cho các task độc lập
   - Sử dụng tokio::select cho các event loop

3. Các server nên được shutdown một cách graceful:
   - Gửi shutdown signal
   - Đợi các kết nối đóng
   - Đóng các resource

4. Các server nên được monitor và log:
   - Số lượng kết nối
   - Thời gian phản hồi
   - Lỗi và cảnh báo
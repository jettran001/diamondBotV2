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
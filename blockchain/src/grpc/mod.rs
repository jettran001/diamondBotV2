pub mod service;
pub mod client;

// Thêm QUIC protocol cho truyền dữ liệu nhanh và bảo mật
pub mod quic_transport {
    pub struct QuicTransport {
        endpoint: String,
        certificates: Vec<Certificate>,
    }
    
    impl QuicTransport {
        pub fn new(endpoint: &str) -> Self { /* ... */ }
        pub fn connect(&self) -> Result<QuicConnection> { /* ... */ }
        pub fn listen(&self) -> Result<QuicListener> { /* ... */ }
    }
}

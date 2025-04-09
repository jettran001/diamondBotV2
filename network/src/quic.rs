use quinn::{Endpoint, ServerConfig, TransportConfig, ClientConfig};
use rustls::{Certificate, PrivateKey};
use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::Result;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, error, warn, debug};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::config::Config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const BACKPRESSURE_THRESHOLD: usize = 1000;

pub struct QuicServer {
    endpoint: Endpoint,
    rx_receiver: mpsc::Receiver<Vec<u8>>,
    clients: Arc<Mutex<Vec<quinn::Connection>>>,
    last_heartbeat: Arc<Mutex<Instant>>,
}

pub struct QuicClient {
    endpoint: Endpoint,
    tx_sender: mpsc::Sender<Vec<u8>>,
    connection: Arc<Mutex<Option<quinn::Connection>>>,
    last_heartbeat: Arc<Mutex<Instant>>,
    reconnect_attempts: Arc<Mutex<u32>>,
}

fn generate_self_signed_cert() -> Result<(Vec<Certificate>, PrivateKey)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let key = PrivateKey(cert.serialize_private_key_der());
    let cert = Certificate(cert.serialize_der()?);
    
    Ok((vec![cert], key))
}

impl QuicServer {
    pub async fn new(config: &Config) -> Result<Self> {
        let (certs, key) = generate_self_signed_cert()?;
        
        let mut server_config = ServerConfig::with_single_cert(certs, key)?;
        let mut transport_config = TransportConfig::default();
        transport_config.max_concurrent_uni_streams(100u32.into());
        
        let addr: SocketAddr = format!("0.0.0.0:{}", 4433).parse()?;
        let (endpoint, _server_cert) = Endpoint::server(server_config, addr)?;
        
        let (tx, rx) = mpsc::channel(100);
        
        Ok(Self {
            endpoint,
            rx_receiver: rx,
            clients: Arc::new(Mutex::new(Vec::new())),
            last_heartbeat: Arc::new(Mutex::new(Instant::now())),
        })
    }
    
    pub async fn start(&mut self) -> Result<()> {
        info!("QUIC server đang lắng nghe");
        
        // Spawn heartbeat task
        let clients = self.clients.clone();
        let last_heartbeat = self.last_heartbeat.clone();
        tokio::spawn(async move {
            loop {
                sleep(HEARTBEAT_INTERVAL).await;
                let mut clients = clients.lock().await;
                let mut last_heartbeat = last_heartbeat.lock().await;
                
                // Gửi heartbeat đến tất cả clients
                for client in clients.iter_mut() {
                    if let Err(e) = client.send_datagram(&b"heartbeat"[..]).await {
                        warn!(error = %e, "Lỗi khi gửi heartbeat");
                    }
                }
                
                *last_heartbeat = Instant::now();
            }
        });
        
        // Spawn connection handler
        let clients = self.clients.clone();
        while let Some(incoming_conn) = self.endpoint.accept().await {
            let clients = clients.clone();
            tokio::spawn(async move {
                match incoming_conn.await {
                    Ok(conn) => {
                        info!(remote = %conn.remote_address(), "Kết nối QUIC mới");
                        
                        // Thêm client vào danh sách
                        let mut clients = clients.lock().await;
                        clients.push(conn.clone());
                        
                        // Xử lý connection
                        let quinn::NewConnection { connection, .. } = conn;
                        
                        while let Ok(recv) = connection.accept_uni().await {
                            let clients = clients.clone();
                            tokio::spawn(async move {
                                // Xử lý stream với backpressure
                                let mut buffer = Vec::with_capacity(BACKPRESSURE_THRESHOLD);
                                
                                while let Ok(chunk) = recv.read_chunk(1024, true).await {
                                    buffer.extend_from_slice(&chunk.bytes);
                                    
                                    if buffer.len() >= BACKPRESSURE_THRESHOLD {
                                        // Xử lý buffer đầy
                                        debug!("Buffer đầy, xử lý {} bytes", buffer.len());
                                        // TODO: Xử lý buffer
                                        buffer.clear();
                                    }
                                }
                                
                                // Xử lý buffer còn lại
                                if !buffer.is_empty() {
                                    debug!("Xử lý buffer còn lại: {} bytes", buffer.len());
                                    // TODO: Xử lý buffer
                                }
                            });
                        }
                        
                        // Xóa client khi connection đóng
                        let mut clients = clients.lock().await;
                        if let Some(pos) = clients.iter().position(|c| c.stable_id() == connection.stable_id()) {
                            clients.remove(pos);
                        }
                    },
                    Err(e) => {
                        error!(error = %e, "Lỗi khi thiết lập kết nối QUIC");
                    }
                }
            });
        }
        
        Ok(())
    }
}

impl QuicClient {
    pub async fn new(config: &Config, server_addr: SocketAddr) -> Result<Self> {
        let mut client_config = ClientConfig::new(Arc::new(
            rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
                .with_no_client_auth()
        ));
        
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);
        
        let (tx, _) = mpsc::channel(100);
        
        Ok(Self {
            endpoint,
            tx_sender: tx,
            connection: Arc::new(Mutex::new(None)),
            last_heartbeat: Arc::new(Mutex::new(Instant::now())),
            reconnect_attempts: Arc::new(Mutex::new(0)),
        })
    }
    
    pub async fn connect(&self, server_addr: SocketAddr) -> Result<()> {
        info!(server_addr = %server_addr, "Kết nối đến QUIC server");
        
        let mut reconnect_attempts = self.reconnect_attempts.lock().await;
        *reconnect_attempts = 0;
        
        self.try_connect(server_addr).await?;
        
        // Spawn heartbeat task
        let connection = self.connection.clone();
        let last_heartbeat = self.last_heartbeat.clone();
        tokio::spawn(async move {
            loop {
                sleep(HEARTBEAT_INTERVAL).await;
                let connection = connection.lock().await;
                let mut last_heartbeat = last_heartbeat.lock().await;
                
                if let Some(conn) = connection.as_ref() {
                    if let Err(e) = conn.send_datagram(&b"heartbeat"[..]).await {
                        warn!(error = %e, "Lỗi khi gửi heartbeat");
                    }
                }
                
                *last_heartbeat = Instant::now();
            }
        });
        
        Ok(())
    }
    
    async fn try_connect(&self, server_addr: SocketAddr) -> Result<()> {
        let mut reconnect_attempts = self.reconnect_attempts.lock().await;
        
        while *reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
            match self.endpoint.connect(server_addr, "localhost")?.await {
                Ok(conn) => {
                    info!("Kết nối QUIC thành công");
                    let mut connection = self.connection.lock().await;
                    *connection = Some(conn);
                    *reconnect_attempts = 0;
                    return Ok(());
                },
                Err(e) => {
                    *reconnect_attempts += 1;
                    warn!(attempt = %reconnect_attempts, error = %e, "Lỗi khi kết nối QUIC");
                    
                    if *reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                        sleep(RECONNECT_DELAY * *reconnect_attempts).await;
                    } else {
                        error!("Đã vượt quá số lần thử kết nối tối đa");
                        return Err(e.into());
                    }
                }
            }
        }
        
        Ok(())
    }
}

// QUIC helper
struct SkipServerVerification;

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

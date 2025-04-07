use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use tokio::sync::{Mutex, mpsc};
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Serialize, Deserialize};

type ClientId = String;
type Clients = Arc<Mutex<HashMap<ClientId, mpsc::Sender<Message>>>>;
type RateLimits = Arc<Mutex<HashMap<ClientId, (Instant, u32)>>>;
type AuthenticatedClients = Arc<Mutex<HashSet<ClientId>>>;

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const MAX_MESSAGES_PER_WINDOW: u32 = 100;
const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1MB
const TOKEN_EXPIRY: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
}

pub struct WebSocketServer {
    clients: Clients,
    rate_limits: RateLimits,
    authenticated_clients: AuthenticatedClients,
    jwt_secret: String,
}

impl WebSocketServer {
    pub fn new(jwt_secret: String) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            rate_limits: Arc::new(Mutex::new(HashMap::new())),
            authenticated_clients: Arc::new(Mutex::new(HashSet::new())),
            jwt_secret,
        }
    }
    
    pub fn get_clients(&self) -> Clients {
        self.clients.clone()
    }
    
    // Validate JWT token
    async fn validate_token(&self, token: &str) -> Result<String, String> {
        let validation = Validation::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(self.jwt_secret.as_bytes());
        
        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        ) {
            Ok(token_data) => Ok(token_data.claims.sub),
            Err(e) => Err(format!("Token không hợp lệ: {}", e)),
        }
    }
    
    // Check rate limit
    async fn check_rate_limit(&self, client_id: &str) -> Result<(), String> {
        let mut rate_limits = self.rate_limits.lock().await;
        let now = Instant::now();
        
        if let Some((window_start, count)) = rate_limits.get_mut(client_id) {
            if now.duration_since(*window_start) > RATE_LIMIT_WINDOW {
                *window_start = now;
                *count = 1;
            } else if *count >= MAX_MESSAGES_PER_WINDOW {
                return Err("Đã vượt quá giới hạn message".to_string());
            } else {
                *count += 1;
            }
        } else {
            rate_limits.insert(client_id.to_string(), (now, 1));
        }
        
        Ok(())
    }
    
    // Validate message
    fn validate_message(&self, message: &str) -> Result<(), String> {
        if message.len() > MAX_MESSAGE_SIZE {
            return Err("Message quá lớn".to_string());
        }
        
        // Validate JSON format
        if let Err(e) = serde_json::from_str::<Value>(message) {
            return Err(format!("Message không phải JSON hợp lệ: {}", e));
        }
        
        Ok(())
    }
    
    // Handler cho kết nối websocket mới
    pub async fn handle_socket(
        &self,
        socket: WebSocket,
        auth_token: Option<String>,
    ) {
        let client_id = Uuid::new_v4().to_string();
        info!("Kết nối WebSocket mới: {}", client_id);
        
        // Xác thực nếu có token
        if let Some(token) = auth_token {
            match self.validate_token(&token).await {
                Ok(user_id) => {
                    info!(user_id = %user_id, "Client đã xác thực");
                    let mut authenticated = self.authenticated_clients.lock().await;
                    authenticated.insert(client_id.clone());
                },
                Err(e) => {
                    error!(error = %e, "Xác thực thất bại");
                    return;
                }
            }
        }
        
        // Chia socket thành sender và receiver
        let (mut sender, mut receiver) = socket.split();
        
        // Tạo channel cho client cụ thể này
        let (tx, mut rx) = mpsc::channel::<Message>(100);
        
        // Lưu sender vào clients map
        {
            let mut clients = self.clients.lock().await;
            clients.insert(client_id.clone(), tx);
        }
        
        // Task gửi message đến client
        let send_task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if sender.send(message).await.is_err() {
                    break;
                }
            }
            
            // Đóng connection khi channel được đóng
            let _ = sender.close().await;
        });
        
        // Task nhận message từ client
        let clients_clone = self.clients.clone();
        let client_id_clone = client_id.clone();
        let rate_limits = self.rate_limits.clone();
        let authenticated_clients = self.authenticated_clients.clone();
        
        let receive_task = tokio::spawn(async move {
            while let Some(Ok(message)) = receiver.next().await {
                match message {
                    Message::Text(text) => {
                        debug!("Nhận message từ {}: {}", client_id_clone, text);
                        
                        // Kiểm tra rate limit
                        if let Err(e) = rate_limits.lock().await.check_rate_limit(&client_id_clone).await {
                            warn!(error = %e, "Rate limit vượt quá");
                            continue;
                        }
                        
                        // Validate message
                        if let Err(e) = validate_message(&text) {
                            warn!(error = %e, "Message không hợp lệ");
                            continue;
                        }
                        
                        // Process message
                        if text == "ping" {
                            let mut clients = clients_clone.lock().await;
                            if let Some(sender) = clients.get(&client_id_clone) {
                                if sender.send(Message::Text("pong".to_string())).await.is_err() {
                                    // Xóa client nếu không thể gửi message
                                    clients.remove(&client_id_clone);
                                }
                            }
                        }
                    },
                    Message::Close(_) => {
                        break;
                    },
                    _ => {}
                }
            }
            
            // Xóa client khi connection đóng
            let mut clients = clients_clone.lock().await;
            clients.remove(&client_id_clone);
            
            let mut authenticated = authenticated_clients.lock().await;
            authenticated.remove(&client_id_clone);
            
            let mut rate_limits = rate_limits.lock().await;
            rate_limits.remove(&client_id_clone);
            
            info!("WebSocket đóng: {}", client_id_clone);
        });
        
        // Đợi bất kỳ task nào kết thúc
        tokio::select! {
            _ = send_task => receive_task.abort(),
            _ = receive_task => send_task.abort(),
        };
    }
    
    // Gửi message đến tất cả clients
    pub async fn broadcast(&self, message: Value) {
        let clients = self.clients.lock().await;
        let message_text = serde_json::to_string(&message).unwrap();
        
        for (client_id, tx) in clients.iter() {
            if tx.send(Message::Text(message_text.clone())).await.is_err() {
                error!("Lỗi khi gửi message đến client: {}", client_id);
            }
        }
    }
    
    // Gửi message đến một client cụ thể
    pub async fn send_to_client(&self, client_id: &str, message: Value) -> bool {
        let clients = self.clients.lock().await;
        
        if let Some(tx) = clients.get(client_id) {
            let message_text = serde_json::to_string(&message).unwrap();
            tx.send(Message::Text(message_text)).await.is_ok()
        } else {
            false
        }
    }
    
    // Lấy số lượng client đang kết nối
    pub async fn get_connected_count(&self) -> usize {
        let clients = self.clients.lock().await;
        clients.len()
    }
    
    // Lấy số lượng client đã xác thực
    pub async fn get_authenticated_count(&self) -> usize {
        let authenticated = self.authenticated_clients.lock().await;
        authenticated.len()
    }
}

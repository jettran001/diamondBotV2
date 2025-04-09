use tokio_tungstenite::{
    connect_async, 
    tungstenite::protocol::Message,
    tungstenite::Error as WsError,
};
use futures::{SinkExt, StreamExt};
use url::Url;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use anyhow::Result;
use tracing::{info, error, debug, warn};
use serde::{Serialize, Deserialize};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::config::Config;

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const MESSAGE_QUEUE_SIZE: usize = 1000;

pub struct WebSocketClient {
    url: String,
    connected: Arc<Mutex<bool>>,
    tx_sender: mpsc::Sender<String>,
    rx_receiver: mpsc::Receiver<String>,
    message_queue: Arc<Mutex<Vec<String>>>,
    last_heartbeat: Arc<Mutex<Instant>>,
    reconnect_attempts: Arc<Mutex<u32>>,
}

#[derive(Serialize, Deserialize)]
pub struct BlockchainSubscription {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Vec<String>,
}

impl WebSocketClient {
    pub fn new(url: String) -> Self {
        let (tx, rx) = mpsc::channel(100);
        
        Self {
            url,
            connected: Arc::new(Mutex::new(false)),
            tx_sender: tx,
            rx_receiver: rx,
            message_queue: Arc::new(Mutex::new(Vec::with_capacity(MESSAGE_QUEUE_SIZE))),
            last_heartbeat: Arc::new(Mutex::new(Instant::now())),
            reconnect_attempts: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn get_sender(&self) -> mpsc::Sender<String> {
        self.tx_sender.clone()
    }
    
    pub async fn connect(&self) -> Result<()> {
        let url = Url::parse(&self.url)?;
        let connected = Arc::clone(&self.connected);
        let mut rx = self.rx_receiver.clone();
        let message_queue = Arc::clone(&self.message_queue);
        let last_heartbeat = Arc::clone(&self.last_heartbeat);
        let reconnect_attempts = Arc::clone(&self.reconnect_attempts);
        
        // Reset reconnect attempts
        *reconnect_attempts.lock().await = 0;
        
        tokio::spawn(async move {
            loop {
                match connect_async(url.clone()).await {
                    Ok((mut ws_stream, _)) => {
                        *connected.lock().await = true;
                        info!("WebSocket kết nối thành công");
                        
                        // Reset reconnect attempts
                        *reconnect_attempts.lock().await = 0;
                        
                        // Xử lý outgoing messages
                        let (mut write, mut read) = ws_stream.split();
                        
                        // Spawn heartbeat task
                        let connected = connected.clone();
                        let last_heartbeat = last_heartbeat.clone();
                        let write = write.clone();
                        
                        let heartbeat_task = tokio::spawn(async move {
                            loop {
                                sleep(HEARTBEAT_INTERVAL).await;
                                if *connected.lock().await {
                                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                                        warn!(error = %e, "Lỗi khi gửi heartbeat");
                                        break;
                                    }
                                    *last_heartbeat.lock().await = Instant::now();
                                }
                            }
                        });
                        
                        // Spawn một task để gửi message từ queue
                        let message_queue = message_queue.clone();
                        let write = write.clone();
                        let connected = connected.clone();
                        
                        let send_task = tokio::spawn(async move {
                            while *connected.lock().await {
                                let mut queue = message_queue.lock().await;
                                if !queue.is_empty() {
                                    let message = queue.remove(0);
                                    if let Err(e) = write.send(Message::Text(message)).await {
                                        error!(error = %e, "Lỗi khi gửi message");
                                        break;
                                    }
                                } else {
                                    drop(queue);
                                    sleep(Duration::from_millis(100)).await;
                                }
                            }
                        });
                        
                        // Xử lý incoming messages
                        let connected = connected.clone();
                        let recv_task = tokio::spawn(async move {
                            while *connected.lock().await {
                                match read.next().await {
                                    Some(Ok(msg)) => {
                                        match msg {
                                            Message::Text(text) => {
                                                debug!(message = %text, "Nhận message");
                                                // Xử lý message nhận được ở đây
                                            },
                                            Message::Pong(_) => {
                                                debug!("Nhận Pong");
                                            },
                                            Message::Close(_) => {
                                                info!("WebSocket đóng bởi server");
                                                *connected.lock().await = false;
                                                break;
                                            },
                                            _ => {}
                                        }
                                    },
                                    Some(Err(e)) => {
                                        error!(error = %e, "Lỗi khi nhận message");
                                        break;
                                    },
                                    None => {
                                        info!("WebSocket stream đóng");
                                        *connected.lock().await = false;
                                        break;
                                    }
                                }
                            }
                        });
                        
                        // Đợi một trong các task kết thúc
                        tokio::select! {
                            _ = &mut heartbeat_task => {
                                send_task.abort();
                                recv_task.abort();
                            }
                            _ = &mut send_task => {
                                heartbeat_task.abort();
                                recv_task.abort();
                            }
                            _ = &mut recv_task => {
                                heartbeat_task.abort();
                                send_task.abort();
                            }
                        }
                        
                        *connected.lock().await = false;
                        info!("WebSocket đã ngắt kết nối");
                        
                        // Thử kết nối lại với exponential backoff
                        let mut attempts = reconnect_attempts.lock().await;
                        *attempts += 1;
                        
                        if *attempts < MAX_RECONNECT_ATTEMPTS {
                            let delay = RECONNECT_DELAY * *attempts;
                            warn!(attempt = %attempts, delay = ?delay, "Thử kết nối lại");
                            sleep(delay).await;
                        } else {
                            error!("Đã vượt quá số lần thử kết nối tối đa");
                            break;
                        }
                    },
                    Err(e) => {
                        error!(error = %e, "Lỗi khi kết nối WebSocket");
                        break;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    pub async fn subscribe_to_pending_txs(&self) -> Result<()> {
        let subscription = BlockchainSubscription {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "eth_subscribe".to_string(),
            params: vec!["newPendingTransactions".to_string()],
        };
        
        let msg = serde_json::to_string(&subscription)?;
        
        // Thêm message vào queue
        let mut queue = self.message_queue.lock().await;
        if queue.len() >= MESSAGE_QUEUE_SIZE {
            warn!("Message queue đầy, bỏ qua message");
            return Ok(());
        }
        
        queue.push(msg);
        
        info!("Đã đăng ký lắng nghe pending transactions");
        Ok(())
    }
}

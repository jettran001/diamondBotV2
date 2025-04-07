/// Kiểm tra mempool để tìm giao dịch swap tiềm năng
pub async fn check_mempool(&self) -> Result<Vec<PendingTransaction>, Error> {
    // Tránh spam node với quá nhiều yêu cầu
    if let Some(last_check) = self.last_mempool_check.lock().await.as_ref() {
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration,
            Err(e) => {
                warn!("Lỗi khi lấy thời gian hệ thống: {}", e);
                return Err(Error::TimeError(e.to_string()));
            }
        };
        
        if now - *last_check < Duration::from_secs(MEMPOOL_CHECK_INTERVAL_SECONDS) {
            // Quá sớm để kiểm tra lại
            return Ok(Vec::new());
        }
    }
    
    info!("Kiểm tra mempool cho giao dịch swap");
    
    // Lấy các giao dịch đang chờ xử lý
    let pending_txs = match tokio::time::timeout(
        Duration::from_secs(10), // Timeout 10 giây
        self.provider.get_pending_transactions()
    ).await {
        Ok(Ok(txs)) => txs,
        Ok(Err(e)) => {
            error!("Lỗi khi lấy giao dịch đang chờ xử lý: {}", e);
            return Err(Error::ProviderError(e.to_string()));
        },
        Err(_) => {
            error!("Timeout khi lấy giao dịch đang chờ xử lý");
            return Err(Error::TimeoutError("Lấy giao dịch đang chờ xử lý".to_string()));
        }
    };
    
    // Cập nhật thời gian kiểm tra cuối cùng
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let mut last_check = self.last_mempool_check.lock().await;
            *last_check = Some(duration);
        },
        Err(e) => {
            warn!("Lỗi khi lấy thời gian hệ thống khi cập nhật last_mempool_check: {}", e);
        }
    }
    
    // Phân tích các giao dịch để tìm swap
    let mut results = Vec::new();
    
    for tx in pending_txs {
        // Kiểm tra xem có phải giao dịch swap không
        if let Some(to) = tx.to {
            // Bỏ qua các giao dịch nếu không có dữ liệu input
            if tx.input.len() <= 10 {
                continue;
            }
            
            // Lấy function selector (4 byte đầu tiên)
            let selector = &tx.input.as_ref()[0..4];
            
            // Kiểm tra xem selector có phải là hàm swap hay không
            if self.is_swap_function(selector) {
                // Phân tích dữ liệu đầu vào
                let data = self.parse_swap_data(&tx.input).await;
                
                // Nếu lấy được dữ liệu swap
                if let Some(swap_data) = data {
                    let pending_tx = PendingTransaction {
                        hash: format!("{:?}", tx.hash),
                        from: format!("{:?}", tx.from),
                        to: format!("{:?}", to),
                        gas_price: tx.gas_price.map(|p| p.as_u64()).unwrap_or(0),
                        value: tx.value.as_u64(),
                        data: swap_data,
                        timestamp: match SystemTime::now().duration_since(UNIX_EPOCH) {
                            Ok(duration) => duration.as_secs(),
                            Err(_) => {
                                // Default an toàn khi không lấy được thời gian
                                warn!("Lỗi khi lấy thời gian hệ thống cho timestamp");
                                0
                            }
                        },
                    };
                    
                    results.push(pending_tx);
                }
            }
        }
    }
    
    info!("Tìm thấy {} giao dịch swap tiềm năng trong mempool", results.len());
    Ok(results)
} 
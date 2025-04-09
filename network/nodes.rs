// src/nodes.rs
use reqwest;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiResponse {
    status: String,
    data: serde_json::Value,
}

pub async fn call_wallet(config: &super::config::Config, endpoint: &str) -> Result<ApiResponse, reqwest::Error> {
    let url = format!("{}/{}", config.wallet_url, endpoint);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", config.omni_token))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if response.status().is_success() {
        response.json().await
    } else {
        Err(reqwest::Error::from(response.error_for_status().unwrap_err()))
    }
}

pub async fn call_exchange(config: &super::config::Config, endpoint: &str) -> Result<ApiResponse, reqwest::Error> {
    let url = format!("{}/{}", config.exchange_url, endpoint);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", config.omni_token))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if response.status().is_success() {
        response.json().await
    } else {
        Err(reqwest::Error::from(response.error_for_status().unwrap_err()))
    }
}

pub async fn call_snipebot(config: &super::config::Config, endpoint: &str) -> Result<ApiResponse, reqwest::Error> {
    let url = format!("{}/{}", config.snipebot_url, endpoint);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", config.omni_token))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if response.status().is_success() {
        response.json().await
    } else {
        Err(reqwest::Error::from(response.error_for_status().unwrap_err()))
    }
}
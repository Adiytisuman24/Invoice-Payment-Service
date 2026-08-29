use reqwest::Client;
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::time::Duration;

#[derive(Serialize)]
pub struct PspPaymentRequest {
    pub card_token: String,
}

#[derive(Deserialize, Debug)]
pub struct PspPaymentResponse {
    pub status: String,
    pub failure_code: Option<String>,
    pub psp_ref: Option<Uuid>,
}

#[derive(Clone)]
pub struct PspClient {
    client: Client,
    base_url: String,
}

impl PspClient {
    pub fn new(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5)) // Strict 5-second timeout
            .build()
            .expect("Failed to build reqwest client for PSP");
        Self { client, base_url }
    }

    pub async fn process_payment(&self, card_token: &str) -> Result<PspPaymentResponse, reqwest::Error> {
        let url = format!("{}/payments", self.base_url);
        let req_body = PspPaymentRequest {
            card_token: card_token.to_string(),
        };
        
        let resp = self.client.post(&url)
            .json(&req_body)
            .send()
            .await?;
            
        let psp_resp = resp.json::<PspPaymentResponse>().await?;
        Ok(psp_resp)
    }
}

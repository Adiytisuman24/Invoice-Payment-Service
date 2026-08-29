use sqlx::PgPool;
use reqwest::Client;
use std::time::Duration;
use chrono::{Utc, DateTime};
use uuid::Uuid;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{info, warn, error};
use serde_json::Value;

type HmacSha256 = Hmac<Sha256>;

pub async fn run_webhook_worker(db: PgPool) {
    info!("Starting background Webhook Delivery Worker...");
    let http_client = Client::builder()
        .timeout(Duration::from_secs(5)) // Webhook call timeout
        .build()
        .expect("Failed to build reqwest client for webhook worker");

    loop {
        // Fetch up to 10 pending deliveries where next_attempt_at <= now()
        let result: Result<Vec<PendingDelivery>, sqlx::Error> = sqlx::query_as(
            "SELECT d.id, d.endpoint_id, d.event_id, d.event_type, d.payload, d.status, d.attempts,
                    e.url, e.signing_secret
             FROM webhook_deliveries d
             JOIN webhook_endpoints e ON d.endpoint_id = e.id
             WHERE d.status = 'pending' AND (d.next_attempt_at IS NULL OR d.next_attempt_at <= now())
             ORDER BY d.created_at ASC
             LIMIT 10"
        )
        .fetch_all(&db)
        .await;

        match result {
            Ok(deliveries) => {
                for delivery in deliveries {
                    let db_clone = db.clone();
                    let client_clone = http_client.clone();
                    tokio::spawn(async move {
                        if let Err(e) = deliver_webhook(db_clone, client_clone, delivery).await {
                            error!("Error processing webhook delivery: {:?}", e);
                        }
                    });
                }
            }
            Err(e) => {
                error!("Database error fetching pending webhooks: {:?}", e);
            }
        }

        // Poll database every 2 seconds
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[derive(sqlx::FromRow, Debug)]
struct PendingDelivery {
    id: Uuid,
    endpoint_id: Uuid,
    event_id: Uuid,
    event_type: String,
    payload: Value,
    status: String,
    attempts: i32,
    url: String,
    signing_secret: String,
}

async fn deliver_webhook(
    db: PgPool,
    client: Client,
    delivery: PendingDelivery,
) -> Result<(), anyhow::Error> {
    info!("Attempting delivery of event {} to {}", delivery.event_id, delivery.url);

    let timestamp = Utc::now().timestamp();
    let body_str = serde_json::to_string(&delivery.payload)?;
    
    // Sign payload: timestamp + "." + body
    let signed_payload = format!("{}.{}", timestamp, body_str);
    let mut mac = HmacSha256::new_from_slice(delivery.signing_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(signed_payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Send HTTP POST request
    let resp_result = client.post(&delivery.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Id", delivery.event_id.to_string())
        .header("X-Webhook-Timestamp", timestamp.to_string())
        .header("X-Webhook-Signature", format!("sha256={}", signature))
        .body(body_str)
        .send()
        .await;

    let next_attempt = delivery.attempts + 1;

    match resp_result {
        Ok(resp) if resp.status().is_success() => {
            info!("Successfully delivered event {} to URL {}", delivery.event_id, delivery.url);
            sqlx::query(
                "UPDATE webhook_deliveries
                 SET status = 'delivered', attempts = $1, delivered_at = now(), last_error = NULL
                 WHERE id = $2"
            )
            .bind(next_attempt)
            .bind(delivery.id)
            .execute(&db)
            .await?;
        }
        other => {
            let error_msg = match other {
                Ok(resp) => format!("HTTP error: {}", resp.status()),
                Err(err) => format!("Network error: {}", err),
            };
            warn!("Failed to deliver event {}: {}", delivery.event_id, error_msg);

            // Determine if we retry or exhaust
            let (new_status, next_run) = if next_attempt >= 5 {
                ("exhausted".to_string(), None)
            } else {
                let backoff_secs = match next_attempt {
                    1 => 10,
                    2 => 30,
                    3 => 120, // 2 minutes
                    4 => 600, // 10 minutes
                    _ => 0,
                };
                let run_time = Utc::now() + chrono::Duration::seconds(backoff_secs);
                ("pending".to_string(), Some(run_time))
            };

            sqlx::query(
                "UPDATE webhook_deliveries
                 SET status = $1, attempts = $2, next_attempt_at = $3, last_error = $4
                 WHERE id = $5"
            )
            .bind(new_status)
            .bind(next_attempt)
            .bind(next_run)
            .bind(error_msg)
            .bind(delivery.id)
            .execute(&db)
            .await?;
        }
    }

    Ok(())
}

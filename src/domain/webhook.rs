use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Full endpoint record — returned ONLY at creation time.
/// The `signing_secret` is shown once; it cannot be retrieved via the list endpoint.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct WebhookEndpoint {
    pub id: Uuid,
    pub business_id: Uuid,
    pub url: String,
    pub signing_secret: String,
    pub created_at: DateTime<Utc>,
}

/// Redacted summary — returned by GET /v1/webhook-endpoints.
/// The signing secret is intentionally excluded for security.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct WebhookEndpointSummary {
    pub id: Uuid,
    pub business_id: Uuid,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
#[allow(dead_code)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub event_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

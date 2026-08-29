use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PaymentAttemptStatus {
    Pending,
    Succeeded,
    Failed,
}

impl PaymentAttemptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentAttemptStatus::Pending => "pending",
            PaymentAttemptStatus::Succeeded => "succeeded",
            PaymentAttemptStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(PaymentAttemptStatus::Pending),
            "succeeded" => Ok(PaymentAttemptStatus::Succeeded),
            "failed" => Ok(PaymentAttemptStatus::Failed),
            _ => Err(format!("Unknown payment attempt status: {}", s)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct PaymentAttempt {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub idempotency_key: String,
    pub request_hash: String,
    pub status: String,
    pub failure_code: Option<String>,
    pub psp_ref: Option<Uuid>,
    pub response_status: Option<i32>,
    pub response_body: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct PaymentAttemptResponse {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub status: PaymentAttemptStatus,
    pub failure_code: Option<String>,
    pub psp_ref: Option<Uuid>,
}

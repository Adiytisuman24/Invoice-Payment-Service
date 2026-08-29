use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceState {
    Draft,
    Open,
    Paid,
    Void,
    Uncollectible,
}

impl InvoiceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            InvoiceState::Draft => "draft",
            InvoiceState::Open => "open",
            InvoiceState::Paid => "paid",
            InvoiceState::Void => "void",
            InvoiceState::Uncollectible => "uncollectible",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(InvoiceState::Draft),
            "open" => Ok(InvoiceState::Open),
            "paid" => Ok(InvoiceState::Paid),
            "void" => Ok(InvoiceState::Void),
            "uncollectible" => Ok(InvoiceState::Uncollectible),
            _ => Err(format!("Unknown invoice state: {}", s)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct Invoice {
    pub id: Uuid,
    pub business_id: Uuid,
    pub customer_id: Uuid,
    pub total_cents: i64,
    pub state: String,
    pub due_date: NaiveDate,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct InvoiceItem {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub description: String,
    pub quantity: i32,
    pub unit_amount_cents: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct InvoiceResponse {
    pub id: Uuid,
    pub business_id: Uuid,
    pub customer_id: Uuid,
    pub total_cents: i64,
    pub state: InvoiceState,
    pub due_date: NaiveDate,
    pub created_at: DateTime<Utc>,
    pub items: Vec<InvoiceItem>,
}

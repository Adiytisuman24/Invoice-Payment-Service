use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json,
    Router,
};
use serde::Deserialize;
use uuid::Uuid;
use crate::api::auth::AuthenticatedBusiness;
use crate::api::ApiErrorResponse;
use crate::services::payment::process_invoice_payment;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PayInvoiceRequest {
    pub card_token: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/:id/pay", post(pay_invoice))
}

async fn pay_invoice(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<PayInvoiceRequest>,
) -> Result<(StatusCode, Json<crate::domain::payment::PaymentAttemptResponse>), Response> {
    // Extract and validate Idempotency-Key header
    let idempotency_key = match headers.get("Idempotency-Key").and_then(|val| val.to_str().ok()) {
        Some(key) if !key.trim().is_empty() => key.trim(),
        _ => {
            return Err(ApiErrorResponse::new(
                "validation_error",
                "Idempotency-Key header is required for payment requests",
            )
            .into_response_with_code(StatusCode::BAD_REQUEST));
        }
    };

    if req.card_token.trim().is_empty() {
        return Err(ApiErrorResponse::new(
            "validation_error",
            "card_token is required",
        )
        .into_response_with_code(StatusCode::BAD_REQUEST));
    }

    // Call payment processing service
    let attempt_resp = process_invoice_payment(
        &state,
        auth.business_id,
        id,
        idempotency_key,
        &req.card_token,
    )
    .await?;

    // Return result
    Ok((StatusCode::OK, Json(attempt_resp)))
}

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;
use chrono::Utc;
use serde_json::json;
use crate::api::ApiErrorResponse;
use crate::db::hash_api_key;
use crate::domain::invoice::{Invoice, InvoiceItem, InvoiceState};
use crate::domain::payment::{PaymentAttempt, PaymentAttemptResponse, PaymentAttemptStatus};
use crate::state::AppState;

pub async fn process_invoice_payment(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
    idempotency_key: &str,
    card_token: &str,
) -> Result<PaymentAttemptResponse, Response> {
    // Compute a hash of the request payload (card_token is the full request body)
    let request_hash = hash_api_key(card_token);

    // ── Transaction 1 ─────────────────────────────────────────────────────────
    // Lock the invoice row to serialize concurrent payment requests.
    // Only ONE request holds the lock at a time; all others wait.
    let mut tx = state.db.begin().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to start payment transaction")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // SELECT FOR UPDATE: row-level lock prevents double-charging.
    let invoice: Option<Invoice> = sqlx::query_as(
        "SELECT id, business_id, customer_id, total_cents, state, due_date, created_at
         FROM invoices
         WHERE id = $1 AND business_id = $2
         FOR UPDATE"
    )
    .bind(invoice_id)
    .bind(business_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to query invoice")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let invoice = invoice.ok_or_else(|| {
        ApiErrorResponse::new("invoice_not_found", &format!("Invoice {} not found", invoice_id))
            .into_response_with_code(StatusCode::NOT_FOUND)
    })?;

    // ── Idempotency check (BEFORE state guard) ───────────────────────────────
    // Must happen first: a completed attempt should always replay its stored
    // response, even if the invoice has since moved to a terminal state (e.g.
    // the payment succeeded and the invoice is now PAID).
    let existing: Option<PaymentAttempt> = sqlx::query_as(
        "SELECT id, invoice_id, idempotency_key, request_hash, status,
                failure_code, psp_ref, response_status, response_body,
                created_at, completed_at
         FROM payment_attempts
         WHERE invoice_id = $1 AND idempotency_key = $2"
    )
    .bind(invoice_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to query payment attempts")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if let Some(attempt) = existing {
        // Different payload for same key → hard reject
        if attempt.request_hash != request_hash {
            return Err(ApiErrorResponse::new(
                "idempotency_key_reused",
                "Idempotency-Key was already used with a different request body",
            )
            .into_response_with_code(StatusCode::CONFLICT));
        }

        tx.commit().await.ok();

        // If we already have a stored response body, replay it exactly
        if let Some(body) = attempt.response_body {
            let status_code = attempt.response_status.unwrap_or(200) as u16;
            let status = StatusCode::from_u16(status_code)
                .unwrap_or(StatusCode::OK);
            return Err((status, Json(body)).into_response());
        }

        // Still pending (PSP in-flight or timed-out) — return 202
        let resp = build_attempt_response(&attempt);
        return Err((StatusCode::ACCEPTED, Json(resp)).into_response());
    }

    let current_state = InvoiceState::from_str(&invoice.state).map_err(|_| {
        ApiErrorResponse::new("internal_error", "Invalid invoice state in database")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Guard: invoice must be OPEN to accept payment
    if current_state != InvoiceState::Open {
        return Err(ApiErrorResponse::new(
            "invoice_not_payable",
            &format!("Invoice is in state '{}' and cannot be paid", current_state.as_str()),
        )
        .into_response_with_code(StatusCode::CONFLICT));
    }

    // ── Guard: block if another PENDING attempt already exists ────────────────
    // This prevents a second PSP call while the first is still in-flight.
    let active_pending: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM payment_attempts WHERE invoice_id = $1 AND status = 'pending' LIMIT 1"
    )
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to check pending attempts")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if let Some((pending_id,)) = active_pending {
        tx.commit().await.ok();
        let resp_body = json!({
            "id": pending_id,
            "invoice_id": invoice_id,
            "status": "pending",
            "failure_code": null,
            "psp_ref": null
        });
        return Err((StatusCode::ACCEPTED, Json(resp_body)).into_response());
    }

    // ── Create PENDING attempt and commit ─────────────────────────────────────
    let attempt_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO payment_attempts
             (id, invoice_id, idempotency_key, request_hash, status, created_at)
         VALUES ($1, $2, $3, $4, 'pending', now())"
    )
    .bind(attempt_id)
    .bind(invoice_id)
    .bind(idempotency_key)
    .bind(&request_hash)
    .execute(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to create payment attempt")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Commit Transaction 1 — releases the row lock BEFORE calling the PSP.
    // This is deliberate: we never hold a DB lock while waiting on an external HTTP call.
    tx.commit().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to commit payment initialization")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // ── Call PSP (outside any transaction) ────────────────────────────────────
    let psp_result = state.psp_client.process_payment(card_token).await;

    match psp_result {
        // ── PSP responded (success or decline) ────────────────────────────────
        Ok(psp_resp) => {
            let now = Utc::now();
            let attempt_status = if psp_resp.status == "succeeded" {
                PaymentAttemptStatus::Succeeded
            } else {
                PaymentAttemptStatus::Failed
            };

            // Build the response we will store and return
            let response_payload = PaymentAttemptResponse {
                id: attempt_id,
                invoice_id,
                status: attempt_status,
                failure_code: psp_resp.failure_code.clone(),
                psp_ref: psp_resp.psp_ref,
            };
            let response_body = serde_json::to_value(&response_payload).unwrap_or_default();
            let response_http_status = StatusCode::OK.as_u16() as i32;

            // Fetch items for webhook payload
            let items: Vec<InvoiceItem> = sqlx::query_as(
                "SELECT id, invoice_id, description, quantity, unit_amount_cents
                 FROM invoice_items WHERE invoice_id = $1"
            )
            .bind(invoice_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            // ── Transaction 2: persist result atomically ───────────────────────
            let mut tx2 = state.db.begin().await.map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to start update transaction")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            // Update the payment attempt — store response for future replays
            sqlx::query(
                "UPDATE payment_attempts
                 SET status = $1, failure_code = $2, psp_ref = $3,
                     response_status = $4, response_body = $5, completed_at = $6
                 WHERE id = $7"
            )
            .bind(attempt_status.as_str())
            .bind(&psp_resp.failure_code)
            .bind(psp_resp.psp_ref)
            .bind(response_http_status)
            .bind(&response_body)
            .bind(now)
            .bind(attempt_id)
            .execute(&mut *tx2)
            .await
            .map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to update payment attempt")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            let (event_type, invoice_state_str) = if attempt_status == PaymentAttemptStatus::Succeeded {
                sqlx::query("UPDATE invoices SET state = 'paid' WHERE id = $1")
                    .bind(invoice_id)
                    .execute(&mut *tx2)
                    .await
                    .map_err(|_| {
                        ApiErrorResponse::new("internal_error", "Failed to mark invoice paid")
                            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                    })?;
                ("invoice.paid", "paid")
            } else {
                ("invoice.payment_failed", "open")
            };

            let webhook_payload = json!({
                "id": invoice.id,
                "business_id": invoice.business_id,
                "customer_id": invoice.customer_id,
                "total_cents": invoice.total_cents,
                "state": invoice_state_str,
                "due_date": invoice.due_date,
                "created_at": invoice.created_at,
                "items": items,
                "payment_attempt": {
                    "id": attempt_id,
                    "status": attempt_status.as_str(),
                    "failure_code": psp_resp.failure_code,
                    "psp_ref": psp_resp.psp_ref,
                    "completed_at": now
                }
            });

            enqueue_webhook_events(&mut tx2, business_id, event_type, webhook_payload)
                .await
                .map_err(|_| {
                    ApiErrorResponse::new("internal_error", "Failed to queue webhook")
                        .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                })?;

            tx2.commit().await.map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to commit payment result")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            Ok(response_payload)
        }

        // ── PSP timeout: outcome unknown — leave PENDING ──────────────────────
        // A client-side timeout does NOT mean the PSP did not charge the card.
        // Marking it failed here could trigger a double-charge on retry.
        // In production, a reconciliation job or PSP webhook resolves this.
        Err(e) if e.is_timeout() => {
            tracing::warn!("PSP call timed out for attempt {}", attempt_id);
            let resp_body = json!({
                "id": attempt_id,
                "invoice_id": invoice_id,
                "status": "pending",
                "failure_code": null,
                "psp_ref": null
            });
            Err((StatusCode::ACCEPTED, Json(resp_body)).into_response())
        }

        // ── Network / 5xx error: safe to mark FAILED ──────────────────────────
        // A definitive connection refusal or 500 means the PSP rejected the request
        // before any charge was attempted. Invoice stays OPEN; customer can retry.
        Err(e) => {
            tracing::error!("PSP network error for attempt {}: {}", attempt_id, e);
            let failure_code = "psp_unavailable".to_string();
            let now = Utc::now();

            let response_payload = PaymentAttemptResponse {
                id: attempt_id,
                invoice_id,
                status: PaymentAttemptStatus::Failed,
                failure_code: Some(failure_code.clone()),
                psp_ref: None,
            };
            let response_body = serde_json::to_value(&response_payload).unwrap_or_default();

            let mut tx2 = state.db.begin().await.map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to start failure transaction")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            sqlx::query(
                "UPDATE payment_attempts
                 SET status = 'failed', failure_code = $1,
                     response_status = 200, response_body = $2, completed_at = $3
                 WHERE id = $4"
            )
            .bind(&failure_code)
            .bind(&response_body)
            .bind(now)
            .bind(attempt_id)
            .execute(&mut *tx2)
            .await
            .map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to record PSP failure")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            tx2.commit().await.map_err(|_| {
                ApiErrorResponse::new("internal_error", "Failed to commit PSP failure")
                    .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            })?;

            Ok(response_payload)
        }
    }
}

fn build_attempt_response(attempt: &PaymentAttempt) -> PaymentAttemptResponse {
    let status = PaymentAttemptStatus::from_str(&attempt.status)
        .unwrap_or(PaymentAttemptStatus::Pending);
    PaymentAttemptResponse {
        id: attempt.id,
        invoice_id: attempt.invoice_id,
        status,
        failure_code: attempt.failure_code.clone(),
        psp_ref: attempt.psp_ref,
    }
}

async fn enqueue_webhook_events(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    business_id: Uuid,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), sqlx::Error> {
    let event_id = Uuid::new_v4();

    let endpoints: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM webhook_endpoints WHERE business_id = $1"
    )
    .bind(business_id)
    .fetch_all(&mut **tx)
    .await?;

    for (endpoint_id,) in endpoints {
        let delivery_id = Uuid::new_v4();
        let full_payload = json!({
            "event_id": event_id,
            "event_type": event_type,
            "payload": payload
        });

        sqlx::query(
            "INSERT INTO webhook_deliveries
                 (id, endpoint_id, event_id, event_type, payload, status, attempts, next_attempt_at)
             VALUES ($1, $2, $3, $4, $5, 'pending', 0, now())"
        )
        .bind(delivery_id)
        .bind(endpoint_id)
        .bind(event_id)
        .bind(event_type)
        .bind(full_payload)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

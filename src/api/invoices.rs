use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
    Router,
};
use serde::Deserialize;
use uuid::Uuid;
use chrono::NaiveDate;
use crate::api::auth::AuthenticatedBusiness;
use crate::api::ApiErrorResponse;
use crate::domain::invoice::{Invoice, InvoiceItem, InvoiceResponse, InvoiceState};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateInvoiceItemRequest {
    pub description: String,
    pub quantity: i32,
    pub unit_amount_cents: i64,
}

#[derive(Deserialize)]
pub struct CreateInvoiceRequest {
    pub customer_id: Uuid,
    pub due_date: NaiveDate,
    pub items: Vec<CreateInvoiceItemRequest>,
}

#[derive(Deserialize)]
pub struct ListInvoicesParams {
    pub state: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_invoice).get(list_invoices))
        .route("/:id", get(get_invoice))
        .route("/:id/void", post(void_invoice))
        .route("/:id/uncollectible", post(mark_uncollectible))
}

async fn create_invoice(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Json(req): Json<CreateInvoiceRequest>,
) -> Result<impl IntoResponse, Response> {
    // 1. Validations
    if req.items.is_empty() {
        return Err(ApiErrorResponse::new("validation_error", "Invoice must contain at least one item")
            .into_response_with_code(StatusCode::BAD_REQUEST));
    }

    for item in &req.items {
        if item.description.trim().is_empty() {
            return Err(ApiErrorResponse::new("validation_error", "Item description cannot be empty")
                .into_response_with_code(StatusCode::BAD_REQUEST));
        }
        if item.quantity <= 0 {
            return Err(ApiErrorResponse::new("validation_error", "Item quantity must be greater than 0")
                .into_response_with_code(StatusCode::BAD_REQUEST));
        }
        if item.unit_amount_cents < 0 {
            return Err(ApiErrorResponse::new("validation_error", "Item unit amount cannot be negative")
                .into_response_with_code(StatusCode::BAD_REQUEST));
        }
    }

    // Start database transaction
    let mut tx = state.db.begin().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to start database transaction")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // 2. Verify customer exists and belongs to this business
    let customer_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM customers WHERE id = $1 AND business_id = $2"
    )
    .bind(req.customer_id)
    .bind(auth.business_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to verify customer")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if customer_exists.is_none() {
        return Err(ApiErrorResponse::new("customer_not_found", "Customer not found")
            .into_response_with_code(StatusCode::NOT_FOUND));
    }

    // 3. Compute total_cents
    let mut total_cents: i64 = 0;
    for item in &req.items {
        // Safe integer multiplication checking for overflow
        let item_total = item.unit_amount_cents
            .checked_mul(item.quantity as i64)
            .ok_or_else(|| {
                ApiErrorResponse::new("validation_error", "Invoice total amount calculation overflowed")
                    .into_response_with_code(StatusCode::BAD_REQUEST)
            })?;
        
        total_cents = total_cents.checked_add(item_total).ok_or_else(|| {
            ApiErrorResponse::new("validation_error", "Invoice total amount calculation overflowed")
                .into_response_with_code(StatusCode::BAD_REQUEST)
        })?;
    }

    let invoice_id = Uuid::new_v4();
    let initial_state = InvoiceState::Open.as_str();

    // 4. Insert Invoice
    let invoice: Invoice = sqlx::query_as(
        "INSERT INTO invoices (id, business_id, customer_id, total_cents, state, due_date, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         RETURNING id, business_id, customer_id, total_cents, state, due_date, created_at"
    )
    .bind(invoice_id)
    .bind(auth.business_id)
    .bind(req.customer_id)
    .bind(total_cents)
    .bind(initial_state)
    .bind(req.due_date)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_e| {
        ApiErrorResponse::new("internal_error", "Failed to insert invoice")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // 5. Insert Invoice Items
    let mut inserted_items = Vec::new();
    for item in req.items {
        let item_id = Uuid::new_v4();
        let inserted_item: InvoiceItem = sqlx::query_as(
            "INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_amount_cents)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, invoice_id, description, quantity, unit_amount_cents"
        )
        .bind(item_id)
        .bind(invoice_id)
        .bind(item.description)
        .bind(item.quantity)
        .bind(item.unit_amount_cents)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| {
            ApiErrorResponse::new("internal_error", "Failed to insert invoice items")
                .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
        inserted_items.push(inserted_item);
    }

    tx.commit().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to commit database transaction")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let parsed_state = InvoiceState::from_str(&invoice.state).unwrap();
    let response = InvoiceResponse {
        id: invoice.id,
        business_id: invoice.business_id,
        customer_id: invoice.customer_id,
        total_cents: invoice.total_cents,
        state: parsed_state,
        due_date: invoice.due_date,
        created_at: invoice.created_at,
        items: inserted_items,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

async fn list_invoices(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Query(params): Query<ListInvoicesParams>,
) -> Result<impl IntoResponse, Response> {
    let invoices: Vec<Invoice> = if let Some(ref state_filter) = params.state {
        sqlx::query_as(
            "SELECT id, business_id, customer_id, total_cents, state, due_date, created_at
             FROM invoices
             WHERE business_id = $1 AND state = $2
             ORDER BY created_at DESC"
        )
        .bind(auth.business_id)
        .bind(state_filter.to_lowercase())
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as(
            "SELECT id, business_id, customer_id, total_cents, state, due_date, created_at
             FROM invoices
             WHERE business_id = $1
             ORDER BY created_at DESC"
        )
        .bind(auth.business_id)
        .fetch_all(&state.db)
        .await
    }.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to list invoices")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Json(invoices))
}

async fn get_invoice(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, Response> {
    let invoice: Option<Invoice> = sqlx::query_as(
        "SELECT id, business_id, customer_id, total_cents, state, due_date, created_at
         FROM invoices
         WHERE id = $1 AND business_id = $2"
    )
    .bind(id)
    .bind(auth.business_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to fetch invoice")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let invoice = match invoice {
        Some(inv) => inv,
        None => {
            return Err(ApiErrorResponse::new(
                "invoice_not_found",
                &format!("Invoice with ID {} not found", id),
            )
            .into_response_with_code(StatusCode::NOT_FOUND));
        }
    };

    let items: Vec<InvoiceItem> = sqlx::query_as(
        "SELECT id, invoice_id, description, quantity, unit_amount_cents
         FROM invoice_items
         WHERE invoice_id = $1"
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to fetch invoice items")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let parsed_state = InvoiceState::from_str(&invoice.state).unwrap();
    let response = InvoiceResponse {
        id: invoice.id,
        business_id: invoice.business_id,
        customer_id: invoice.customer_id,
        total_cents: invoice.total_cents,
        state: parsed_state,
        due_date: invoice.due_date,
        created_at: invoice.created_at,
        items,
    };

    Ok(Json(response))
}

async fn void_invoice(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, Response> {
    transition_invoice_state(&state.db, id, auth.business_id, InvoiceState::Void).await
}

async fn mark_uncollectible(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, Response> {
    transition_invoice_state(&state.db, id, auth.business_id, InvoiceState::Uncollectible).await
}

async fn transition_invoice_state(
    db: &sqlx::PgPool,
    invoice_id: Uuid,
    business_id: Uuid,
    target_state: InvoiceState,
) -> Result<Response, Response> {
    let mut tx = db.begin().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to start database transaction")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Lock the invoice row to prevent concurrency race
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
        ApiErrorResponse::new("internal_error", "Failed to query invoice state")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let invoice = match invoice {
        Some(inv) => inv,
        None => {
            return Err(ApiErrorResponse::new(
                "invoice_not_found",
                &format!("Invoice with ID {} not found", invoice_id),
            )
            .into_response_with_code(StatusCode::NOT_FOUND));
        }
    };

    let current_state = InvoiceState::from_str(&invoice.state).map_err(|_| {
        ApiErrorResponse::new("internal_error", "Invalid invoice state in database")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // If already in the target state, treat as idempotent success
    if current_state == target_state {
        tx.commit().await.ok();
        return Ok(StatusCode::OK.into_response());
    }

    // Terminal state transitions rejection rule
    if current_state == InvoiceState::Paid
        || current_state == InvoiceState::Void
        || current_state == InvoiceState::Uncollectible
    {
        return Err(ApiErrorResponse::new(
            "invalid_state_transition",
            &format!("Cannot transition invoice from {} to {}", current_state.as_str(), target_state.as_str()),
        )
        .into_response_with_code(StatusCode::CONFLICT));
    }

    // Update invoice state
    sqlx::query(
        "UPDATE invoices SET state = $1 WHERE id = $2"
    )
    .bind(target_state.as_str())
    .bind(invoice_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to update invoice state")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    tx.commit().await.map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to commit transition transaction")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(StatusCode::OK.into_response())
}

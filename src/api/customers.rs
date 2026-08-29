use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
    Router,
};
use serde::Deserialize;
use uuid::Uuid;
use crate::api::auth::AuthenticatedBusiness;
use crate::api::ApiErrorResponse;
use crate::domain::customer::Customer;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateCustomerRequest {
    pub name: String,
    pub email: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_customer).get(list_customers))
        .route("/:id", get(get_customer))
}

async fn create_customer(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Json(req): Json<CreateCustomerRequest>,
) -> Result<impl IntoResponse, Response> {
    if req.name.trim().is_empty() || req.email.trim().is_empty() {
        return Err(ApiErrorResponse::new("validation_error", "Name and email are required")
            .into_response_with_code(StatusCode::BAD_REQUEST));
    }

    let customer_id = Uuid::new_v4();

    let customer: Customer = sqlx::query_as(
        "INSERT INTO customers (id, business_id, name, email, created_at)
         VALUES ($1, $2, $3, $4, now())
         RETURNING id, business_id, name, email, created_at"
    )
    .bind(customer_id)
    .bind(auth.business_id)
    .bind(req.name)
    .bind(req.email)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        ApiErrorResponse::new("internal_error", "Failed to create customer")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok((StatusCode::CREATED, Json(customer)))
}

async fn list_customers(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
) -> Result<impl IntoResponse, Response> {
    let customers: Vec<Customer> = sqlx::query_as(
        "SELECT id, business_id, name, email, created_at
         FROM customers
         WHERE business_id = $1
         ORDER BY created_at DESC"
    )
    .bind(auth.business_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to list customers")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Json(customers))
}

async fn get_customer(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, Response> {
    let customer: Option<Customer> = sqlx::query_as(
        "SELECT id, business_id, name, email, created_at
         FROM customers
         WHERE id = $1 AND business_id = $2"
    )
    .bind(id)
    .bind(auth.business_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to fetch customer")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    match customer {
        Some(c) => Ok(Json(c)),
        None => Err(ApiErrorResponse::new(
            "customer_not_found",
            &format!("Customer with ID {} not found", id),
        )
        .into_response_with_code(StatusCode::NOT_FOUND)),
    }
}

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json,
    Router,
};
use serde::Deserialize;
use uuid::Uuid;
use crate::api::auth::AuthenticatedBusiness;
use crate::api::ApiErrorResponse;
use crate::domain::webhook::{WebhookEndpoint, WebhookEndpointSummary};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateWebhookEndpointRequest {
    pub url: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(create_webhook_endpoint).get(list_webhook_endpoints))
}

async fn create_webhook_endpoint(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
    Json(req): Json<CreateWebhookEndpointRequest>,
) -> Result<impl IntoResponse, Response> {
    // Basic validation
    if req.url.trim().is_empty() || !req.url.starts_with("http") {
        return Err(ApiErrorResponse::new(
            "validation_error",
            "A valid webhook URL starting with http/https is required",
        )
        .into_response_with_code(StatusCode::BAD_REQUEST));
    }

    let endpoint_id = Uuid::new_v4();
    
    // Generate standard Stripe-like signing secret: whsec_<hex-uuid>
    let random_uuid = Uuid::new_v4();
    let signing_secret = format!("whsec_{}", hex::encode(random_uuid.as_bytes()));

    let endpoint: WebhookEndpoint = sqlx::query_as(
        "INSERT INTO webhook_endpoints (id, business_id, url, signing_secret, created_at)
         VALUES ($1, $2, $3, $4, now())
         RETURNING id, business_id, url, signing_secret, created_at"
    )
    .bind(endpoint_id)
    .bind(auth.business_id)
    .bind(req.url)
    .bind(signing_secret)
    .fetch_one(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to create webhook endpoint")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok((StatusCode::CREATED, Json(endpoint)))
}

async fn list_webhook_endpoints(
    State(state): State<AppState>,
    auth: AuthenticatedBusiness,
) -> Result<impl IntoResponse, Response> {
    // signing_secret is intentionally excluded from the list response.
    // It is only returned at creation time and cannot be retrieved afterwards.
    let endpoints: Vec<WebhookEndpointSummary> = sqlx::query_as(
        "SELECT id, business_id, url, created_at
         FROM webhook_endpoints
         WHERE business_id = $1
         ORDER BY created_at DESC"
    )
    .bind(auth.business_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| {
        ApiErrorResponse::new("internal_error", "Failed to list webhook endpoints")
            .into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Json(endpoints))
}

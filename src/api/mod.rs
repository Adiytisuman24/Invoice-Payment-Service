use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
    Router,
};
use serde::Serialize;

pub mod auth;
pub mod customers;
pub mod invoices;
pub mod payments;
pub mod webhooks;

use crate::state::AppState;

#[derive(Serialize, Clone)]
pub struct ApiErrorDetails {
    pub code: String,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetails,
}

impl ApiErrorResponse {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            error: ApiErrorDetails {
                code: code.to_string(),
                message: message.to_string(),
            },
        }
    }

    pub fn into_response_with_code(self, status: StatusCode) -> Response {
        (status, Json(self)).into_response()
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .nest("/v1/customers", customers::router())
        .nest("/v1/invoices", invoices::router().merge(payments::router()))
        .nest("/v1/webhook-endpoints", webhooks::router())
        .with_state(state)
}

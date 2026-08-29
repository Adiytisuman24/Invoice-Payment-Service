use axum::{
    routing::post,
    Json,
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::sleep;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Deserialize)]
struct PaymentRequest {
    card_token: String,
}

#[derive(Serialize)]
struct PaymentResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    psp_ref: Option<Uuid>,
}

#[tokio::main]
async fn main() {
    // Set up logging
    tracing_subscriber::fmt::init();

    let port = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8081);

    let app = Router::new()
        .route("/payments", post(handle_payment))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("Mock PSP listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn handle_payment(Json(req): Json<PaymentRequest>) -> Result<impl IntoResponse, Response> {
    tracing::info!("Mock PSP received payment request with token: {}", req.card_token);

    match req.card_token.as_str() {
        "tok_success" => {
            sleep(Duration::from_millis(100)).await;
            let resp = PaymentResponse {
                status: "succeeded".to_string(),
                failure_code: None,
                psp_ref: Some(Uuid::new_v4()),
            };
            Ok((StatusCode::OK, Json(resp)))
        }
        "tok_insufficient_funds" => {
            sleep(Duration::from_millis(100)).await;
            let resp = PaymentResponse {
                status: "failed".to_string(),
                failure_code: Some("insufficient_funds".to_string()),
                psp_ref: Some(Uuid::new_v4()),
            };
            Ok((StatusCode::OK, Json(resp)))
        }
        "tok_card_declined" => {
            sleep(Duration::from_millis(100)).await;
            let resp = PaymentResponse {
                status: "failed".to_string(),
                failure_code: Some("card_declined".to_string()),
                psp_ref: Some(Uuid::new_v4()),
            };
            Ok((StatusCode::OK, Json(resp)))
        }
        "tok_timeout" => {
            // Sleep 30 seconds to simulate a slow transaction timeout
            sleep(Duration::from_secs(30)).await;
            let resp = PaymentResponse {
                status: "succeeded".to_string(),
                failure_code: None,
                psp_ref: Some(Uuid::new_v4()),
            };
            Ok((StatusCode::OK, Json(resp)))
        }
        "tok_network_error" => {
            // Return 500 Internal Server Error to simulate network crash
            let err_resp = StatusCode::INTERNAL_SERVER_ERROR.into_response();
            Err(err_resp)
        }
        _ => {
            let resp = PaymentResponse {
                status: "failed".to_string(),
                failure_code: Some("invalid_token".to_string()),
                psp_ref: None,
            };
            Ok((StatusCode::BAD_REQUEST, Json(resp)))
        }
    }
}

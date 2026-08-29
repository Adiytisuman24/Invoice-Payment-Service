use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use uuid::Uuid;
use crate::state::AppState;
use crate::db::hash_api_key;
use crate::api::ApiErrorResponse;

pub struct AuthenticatedBusiness {
    pub business_id: Uuid,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedBusiness
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());

        let api_key = match auth_header {
            Some(header) if header.starts_with("Bearer ") => {
                header["Bearer ".len()..].trim()
            }
            _ => {
                let err = ApiErrorResponse::new("unauthorized", "Missing or malformed Authorization header");
                return Err(err.into_response_with_code(StatusCode::UNAUTHORIZED));
            }
        };

        let key_hash = hash_api_key(api_key);

        let result: Option<(Uuid,)> = sqlx::query_as(
            "SELECT business_id FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL"
        )
        .bind(key_hash)
        .fetch_optional(&app_state.db)
        .await
        .map_err(|_| {
            let err = ApiErrorResponse::new("internal_error", "Database query failed during authentication");
            err.into_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

        match result {
            Some((business_id,)) => Ok(AuthenticatedBusiness { business_id }),
            None => {
                let err = ApiErrorResponse::new("invalid_api_key", "Invalid or revoked API key");
                Err(err.into_response_with_code(StatusCode::UNAUTHORIZED))
            }
        }
    }
}

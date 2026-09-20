use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    /// Optimistic-locking failure — body carries the current row.
    #[error("conflict")]
    Conflict(Value),
    /// Scouting ownership refusal: `scouted_elsewhere` (another session
    /// holds the match) or `scout_lease_expired` (our lease is obsolete and
    /// nobody else holds it). Same 409 status, distinct `error` code, so
    /// clients can tell it from a sequence conflict.
    #[error("{0}")]
    ScoutConflict(&'static str, Value),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("{0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"})),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, json!({"error": "forbidden"})),
            ApiError::NotFound => (StatusCode::NOT_FOUND, json!({"error": "not_found"})),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, json!({"error": m})),
            ApiError::Conflict(current) => (
                StatusCode::CONFLICT,
                json!({"error": "conflict", "current": current}),
            ),
            ApiError::ScoutConflict(code, current) => (
                StatusCode::CONFLICT,
                json!({"error": code, "current": current}),
            ),
            ApiError::Db(e) => {
                tracing::error!("db error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "internal"}))
            }
            ApiError::Internal(m) => {
                tracing::error!("internal error: {m}");
                (StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "internal"}))
            }
        };
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

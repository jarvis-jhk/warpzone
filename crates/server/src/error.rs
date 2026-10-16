//! API errors: every failure is `{"error": code, "message": text}` with a
//! fitting HTTP status.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use warpzone_protocol::api::ErrorBody;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into() }
    }
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "log in first")
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    pub fn limited() -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "too many requests, try again later")
    }
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", message)
    }
    pub fn needs_account() -> Self {
        Self::new(StatusCode::FORBIDDEN, "needs_account", "register a username first")
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        log::error!("internal error: {e}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal server error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.code.into(), message: self.message })).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::internal(e)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::internal(e)
    }
}

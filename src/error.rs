use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub type GatewayResult<T> = Result<T, GatewayError>;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("database migration error")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("redis error")]
    Redis(#[from] fred::error::Error),
    #[error("http client error")]
    Http(#[from] reqwest::Error),
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("insufficient credits")]
    InsufficientCredits,
    #[error("upstream error: {0}")]
    Upstream(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            Self::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Migration(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Redis(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Http(_) => StatusCode::BAD_GATEWAY,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::InsufficientCredits => StatusCode::PAYMENT_REQUIRED,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
        };

        let body = Json(json!({
            "error": self.to_string(),
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

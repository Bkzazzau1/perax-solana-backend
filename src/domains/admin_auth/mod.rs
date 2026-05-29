use std::env;

use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::request::Parts,
    routing::post,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminClaims {
    pub sub: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminLoginRequest {
    pub username: String,
    pub access_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminLoginResponse {
    pub token: String,
    pub username: String,
    pub role: String,
    pub expires_at: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    pub _username: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/api/auth/login", post(login))
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<AdminLoginRequest>,
) -> GatewayResult<Json<AdminLoginResponse>> {
    let admin_username = env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let admin_access_code = env::var("ADMIN_ACCESS_CODE")
        .map_err(|_| GatewayError::Config("ADMIN_ACCESS_CODE is required".to_string()))?;

    let username = payload.username.trim();

    if username != admin_username || payload.access_code != admin_access_code {
        return Err(GatewayError::Unauthorized);
    }

    let expires_at = Utc::now() + Duration::hours(12);

    let claims = AdminClaims {
        sub: username.to_string(),
        role: "admin".to_string(),
        exp: expires_at.timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )
    .map_err(|error| GatewayError::Upstream(format!("admin token signing failed: {error}")))?;

    Ok(Json(AdminLoginResponse {
        token,
        username: username.to_string(),
        role: claims.role,
        expires_at: expires_at.to_rfc3339(),
    }))
}

impl FromRequestParts<AppState> for AuthenticatedAdmin {
    type Rejection = GatewayError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let State(state) = State::<AppState>::from_request_parts(parts, state)
            .await
            .map_err(|_| GatewayError::Unauthorized)?;

        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(GatewayError::Unauthorized)?;

        let token_data = decode::<AdminClaims>(
            token,
            &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| GatewayError::Unauthorized)?;

        if token_data.claims.role != "admin" {
            return Err(GatewayError::Unauthorized);
        }

        Ok(AuthenticatedAdmin {
            _username: token_data.claims.sub,
        })
    }
}

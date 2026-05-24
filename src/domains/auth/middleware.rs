use axum::{
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
};
use uuid::Uuid;

use crate::{domains::auth::service, error::GatewayError, state::AppState};

#[derive(Debug, Clone)]
pub struct AuthenticatedAccount {
    pub account_id: Uuid,
}

impl FromRequestParts<AppState> for AuthenticatedAccount {
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

        service::verify_virtual_key(&state, token).await
    }
}

impl From<StatusCode> for GatewayError {
    fn from(_: StatusCode) -> Self {
        Self::Unauthorized
    }
}

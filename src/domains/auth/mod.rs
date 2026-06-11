use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::GatewayResult, state::AppState};

pub mod middleware;
pub mod service;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailWalletAuthRequest {
    email: String,
    pex_wallet_address: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmailWalletAuthResponse {
    account_id: Uuid,
    email: String,
    pex_wallet_address: String,
    api_key: String,
    key_prefix: String,
    created: bool,
}

async fn signup(
    State(state): State<AppState>,
    Json(request): Json<EmailWalletAuthRequest>,
) -> GatewayResult<Json<EmailWalletAuthResponse>> {
    let session = service::signup_or_login_with_email_wallet(
        &state,
        &request.email,
        &request.pex_wallet_address,
    )
    .await?;

    Ok(Json(EmailWalletAuthResponse {
        account_id: session.account_id,
        email: session.email,
        pex_wallet_address: session.pex_wallet_address,
        api_key: session.api_key,
        key_prefix: session.key_prefix,
        created: session.created,
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<EmailWalletAuthRequest>,
) -> GatewayResult<Json<EmailWalletAuthResponse>> {
    let session =
        service::login_with_email_wallet(&state, &request.email, &request.pex_wallet_address)
            .await?;

    Ok(Json(EmailWalletAuthResponse {
        account_id: session.account_id,
        email: session.email,
        pex_wallet_address: session.pex_wallet_address,
        api_key: session.api_key,
        key_prefix: session.key_prefix,
        created: false,
    }))
}

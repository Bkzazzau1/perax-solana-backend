// src/domains/b2b_gateway/handlers.rs
use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        b2b_gateway::services::{claude, edtech},
    },
    error::GatewayResult,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ProxyPayload {
    #[serde(flatten)]
    pub body: Value,
}

#[derive(Debug, Serialize)]
pub struct ProxyResponse {
    pub provider: &'static str,
    pub accepted: bool,
    pub data: Value,
}

/// Intercepts incoming requests for Claude text generation, checks the user's balance,
/// and returns a continuous HTTP Server-Sent Event (SSE) stream back to the client.
pub async fn proxy_claude_messages(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<ProxyPayload>,
) -> GatewayResult<impl IntoResponse> {
    // Forward directly to the custom SSE streaming utility service
    let stream_response =
        claude::proxy_message_stream(&state, account.account_id, payload.body).await?;

    // Return the response wrapper cleanly down the router line to maintain live connections
    Ok(stream_response)
}

/// Receives document payloads, calculates word metric weights, validates the credit ceiling,
/// and dispatches verified files to our core EdTech scanning suite.
pub async fn scan_edtech_payload(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<ProxyPayload>,
) -> GatewayResult<impl IntoResponse> {
    // Fire our pre-scan engine tool to verify against the Copyleaks engine securely
    let analysis_report = edtech::scan_payload(&state, account.account_id, payload.body).await?;

    // Return the completed analysis metrics payload
    Ok(Json(ProxyResponse {
        provider: "copyleaks",
        accepted: true,
        data: analysis_report,
    }))
}

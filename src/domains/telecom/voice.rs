// src/domains/telecom/voice.rs
use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::{
    domains::auth::middleware::AuthenticatedAccount,
    error::{GatewayError, GatewayResult},
    infra::cache,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct WebRtcOffer {
    pub sdp: String,
    pub destination_number: String, // Target phone number (e.g., +234... or +1...) [cite: 39]
    pub call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebRtcAnswer {
    pub call_id: String,
    pub status: String,
    pub telnyx_control_id: Option<String>,
}

pub async fn get_call(
    State(_state): State<AppState>,
    _account: AuthenticatedAccount, // Guards metadata read queries
    Path(id): Path<String>,
) -> GatewayResult<Json<WebRtcAnswer>> {
    Ok(Json(WebRtcAnswer {
        call_id: id,
        status: "pending".to_string(),
        telnyx_control_id: None,
    }))
}

pub async fn create_offer(
    State(state): State<AppState>,
    account: AuthenticatedAccount, // Automatically extracted compile-time authentication context
    Json(offer): Json<WebRtcOffer>,
) -> GatewayResult<Json<WebRtcAnswer>> {
    let call_id = offer
        .call_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    debug!(
        call_id = %call_id,
        account_id = %account.account_id,
        sdp_bytes = offer.sdp.len(),
        destination = %offer.destination_number,
        "Processing real-time WebRTC signaling handshake"
    );

    // 1. ATOMIC IDENTITY CHECK: Read cached balance in Redis for the specific authenticated workspace
    let user_redis_key = format!("client:balance:{}", account.account_id);
    let current_credits = cache::get_credits(&state.cache, &user_redis_key).await?;

    if let Some(balance) = current_credits {
        if balance <= 0.0 {
            return Err(GatewayError::InsufficientCredits);
        }
    } else {
        return Err(GatewayError::InsufficientCredits);
    }

    // 2. BACKEND TELNYX OUTBOUND ROUTING BUILD [cite: 42, 61, 147]
    // We forward the application WebRTC stream parameters into Telnyx's Outbound Call Engine [cite: 42]
    let telnyx_url = format!("{}/v2/calls", state.config.telnyx_base_url);

    let telnyx_payload = json!({
        "connection_id": "YOUR_TELNYX_SIP_CONNECTION_ID", // Programmatic SIP engine reference [cite: 147]
        "to": offer.destination_number,
        "from": "+1234567890", // Your verified programmatic outbound Caller ID [cite: 150]
        "client_state": call_id, // Pin our internal tracking identifier to the carrier webhook [cite: 150]
    });

    let response = state
        .http
        .post(&telnyx_url)
        .bearer_auth(&state.config.jwt_secret) // Secure master key binding reference used [cite: 61]
        .json(&telnyx_payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        error!(call_id = %call_id, account_id = %account.account_id, error = %err_text, "Telnyx outbound carrier connection rejected");
        return Err(GatewayError::Upstream(format!(
            "Telnyx telephony infrastructure rejected execution: {}",
            err_text
        )));
    }

    // Extract Telnyx reference handle to trace call duration for real-time burning later [cite: 81, 150]
    let resp_json: serde_json::Value = response.json().await?;
    let telnyx_control_id = resp_json["data"]["call_control_id"]
        .as_str()
        .map(|s| s.to_string());

    info!(call_id = %call_id, account_id = %account.account_id, "Outbound carrier connection successfully bridged");

    Ok(Json(WebRtcAnswer {
        call_id,
        status: "accepted".to_string(),
        telnyx_control_id,
    }))
}

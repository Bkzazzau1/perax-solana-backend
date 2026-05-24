// src/domains/telecom/sms.rs
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    domains::auth::middleware::AuthenticatedAccount,
    error::{GatewayError, GatewayResult},
    infra::cache,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct SmsRequest {
    pub to: String,
    pub from: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct SmsResponse {
    pub message_id: String,
    pub routed: bool,
    pub parts_billed: usize,
    pub credits_deducted: f64,
}

pub async fn send_sms(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(request): Json<SmsRequest>,
) -> GatewayResult<Json<SmsResponse>> {
    let body_len = request.body.len();
    if body_len == 0 {
        return Err(GatewayError::Upstream(
            "SMS text content body cannot be empty".to_string(),
        ));
    }

    // 1. Calculate Billing Units based on standard SMS segment lengths (160 chars)
    let parts_billed = ((body_len as f64) / 160.0).ceil() as usize;
    let cost_per_segment = 0.02; // Internal Pera-X service credit cost metric per segment
    let total_sms_cost = (parts_billed as f64) * cost_per_segment;

    let user_redis_key = format!("client:balance:{}", account.account_id);

    // 2. Pre-Send Wallet Credit Check & Atomic Deduction
    let current_credits = cache::get_credits(&state.cache, &user_redis_key).await?;

    match current_credits {
        Some(balance) if balance >= total_sms_cost => {
            cache::increment_credits(&state.cache, &user_redis_key, -total_sms_cost).await?;
        }
        _ => return Err(GatewayError::InsufficientCredits),
    };

    tracing::debug!(
        account_id = %account.account_id,
        to = %request.to,
        parts = parts_billed,
        cost = total_sms_cost,
        "Pre-flight SMS balance debited, dispatching message to downstream carrier"
    );

    // 3. Upstream Telnyx SMS API Target Execution
    let telnyx_sms_url = format!("{}/v2/messages", state.config.telnyx_base_url);
    let telnyx_payload = json!({
        "to": request.to,
        "from": request.from,
        "text": request.body,
    });

    let response = state
        .http
        .post(&telnyx_sms_url)
        .bearer_auth(&state.config.jwt_secret)
        .json(&telnyx_payload)
        .send()
        .await
        .map_err(GatewayError::Http)?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        tracing::error!(account_id = %account.account_id, error = %err_text, "Telnyx SMS gateway delivery rejected");

        // Rollback/Refund Mechanism: Return credits if carrier fails
        cache::increment_credits(&state.cache, &user_redis_key, total_sms_cost).await?;

        return Err(GatewayError::Upstream(format!(
            "Telnyx messaging infrastructure failure: {err_text}"
        )));
    }

    let resp_json: serde_json::Value = response.json().await.map_err(GatewayError::Http)?;
    let message_id = resp_json["data"]["id"]
        .as_str()
        .unwrap_or_else(|| "unknown_carrier_id")
        .to_string();

    Ok(Json(SmsResponse {
        message_id,
        routed: true,
        parts_billed,
        credits_deducted: total_sms_cost,
    }))
}

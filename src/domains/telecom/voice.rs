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
    domains::{auth::middleware::AuthenticatedAccount, pricing},
    error::{GatewayError, GatewayResult},
    infra::cache,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct WebRtcOffer {
    pub sdp: String,
    pub destination_number: String,
    pub call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebRtcAnswer {
    pub call_id: String,
    pub status: String,
    pub telnyx_control_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartCallRequest {
    pub phone_number: String,
    pub destination: String,
    pub is_international: bool,
    pub rate_per_minute: Option<f64>,
    pub credit_balance: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartCallResponse {
    pub call_id: String,
    pub status: String,
    pub phone_number: String,
    pub destination: String,
    pub rate_per_minute: f64,
    pub credit_balance: f64,
    pub estimated_minutes: i64,
    pub reserved_credits: f64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndCallRequest {
    pub call_id: String,
    pub phone_number: String,
    pub duration_seconds: i64,
    pub rate_per_minute: Option<f64>,
    pub credit_balance: f64,
    pub is_international: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndCallResponse {
    pub call_id: String,
    pub status: String,
    pub duration_seconds: i64,
    pub credit_cost: f64,
    pub remaining_credits: f64,
    pub message: String,
}

pub async fn get_call(
    State(_state): State<AppState>,
    _account: AuthenticatedAccount,
    Path(id): Path<String>,
) -> GatewayResult<Json<WebRtcAnswer>> {
    Ok(Json(WebRtcAnswer {
        call_id: id,
        status: "pending".to_string(),
        telnyx_control_id: None,
    }))
}

pub async fn start_call_session(
    State(state): State<AppState>,
    Json(payload): Json<StartCallRequest>,
) -> GatewayResult<Json<StartCallResponse>> {
    let service_code = if payload.is_international {
        "global_call"
    } else {
        "local_call"
    };
    let price = pricing::get_utility_price(&state, service_code).await?;
    let rate_per_minute = price.credit_cost.max(0.0);
    let estimated_minutes = if rate_per_minute > 0.0 {
        (payload.credit_balance / rate_per_minute).floor() as i64
    } else {
        0
    };
    let can_start = !payload.phone_number.trim().is_empty()
        && rate_per_minute > 0.0
        && payload.credit_balance >= rate_per_minute;

    Ok(Json(StartCallResponse {
        call_id: format!("call_{}", Uuid::new_v4()),
        status: if can_start {
            "accepted".to_string()
        } else {
            "rejected".to_string()
        },
        phone_number: payload.phone_number,
        destination: payload.destination,
        rate_per_minute,
        credit_balance: payload.credit_balance,
        estimated_minutes,
        reserved_credits: if can_start { rate_per_minute } else { 0.0 },
        message: if can_start {
            if payload.is_international {
                "Global call session accepted using backend pricing. Credits will be charged by duration.".to_string()
            } else {
                "Local call session accepted using backend pricing. Credits will be charged by duration.".to_string()
            }
        } else {
            "Call rejected. Check phone number or available Credits.".to_string()
        },
    }))
}

pub async fn end_call_session(
    State(state): State<AppState>,
    Json(payload): Json<EndCallRequest>,
) -> GatewayResult<Json<EndCallResponse>> {
    let duration_seconds = payload.duration_seconds.max(0);
    let is_international = payload.is_international.unwrap_or(true);
    let service_code = if is_international {
        "global_call"
    } else {
        "local_call"
    };
    let price = pricing::get_utility_price(&state, service_code).await?;
    let rate_per_minute = price.credit_cost.max(0.0);
    let billed_minutes = (duration_seconds as f64 / 60.0).ceil().max(1.0);
    let credit_cost = billed_minutes * rate_per_minute;
    let remaining_credits = payload.credit_balance - credit_cost;
    let confirmed = !payload.call_id.trim().is_empty() && remaining_credits >= 0.0;

    Ok(Json(EndCallResponse {
        call_id: payload.call_id,
        status: if confirmed {
            "completed".to_string()
        } else {
            "rejected".to_string()
        },
        duration_seconds,
        credit_cost: if confirmed { credit_cost } else { 0.0 },
        remaining_credits,
        message: if confirmed {
            format!(
                "Call to {} completed. Credits deducted using backend pricing.",
                payload.phone_number
            )
        } else {
            "Call completion rejected. Insufficient Credits or invalid call reference.".to_string()
        },
    }))
}

pub async fn create_offer(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
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

    let user_redis_key = format!("client:balance:{}", account.account_id);
    let current_credits = cache::get_credits(&state.cache, &user_redis_key).await?;

    if let Some(balance) = current_credits {
        if balance <= 0.0 {
            return Err(GatewayError::InsufficientCredits);
        }
    } else {
        return Err(GatewayError::InsufficientCredits);
    }

    let telnyx_url = format!("{}/v2/calls", state.config.telnyx_base_url);

    let telnyx_payload = json!({
        "connection_id": "YOUR_TELNYX_SIP_CONNECTION_ID",
        "to": offer.destination_number,
        "from": "+1234567890",
        "client_state": call_id,
    });

    let response = state
        .http
        .post(&telnyx_url)
        .bearer_auth(&state.config.jwt_secret)
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

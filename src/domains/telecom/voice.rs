// src/domains/telecom/voice.rs
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        pricing,
        telecom::billing::{
            credit_balance, debit_credits, log_provider_transaction, round_credits,
        },
    },
    error::{GatewayError, GatewayResult},
    providers::TelnyxClient,
    state::AppState,
};

type HmacSha256 = Hmac<Sha256>;

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

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCallRecord {
    pub id: Uuid,
    pub account_id: Option<Uuid>,
    pub call_id: String,
    pub command_id: Option<String>,
    pub call_control_id: Option<String>,
    pub call_leg_id: Option<String>,
    pub call_session_id: Option<String>,
    pub connection_id: Option<String>,
    pub direction: String,
    pub from_number: String,
    pub to_number: String,
    pub status: String,
    pub telnyx_state: Option<String>,
    pub hangup_cause: Option<String>,
    pub hangup_source: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub answered_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_event_type: Option<String>,
    pub last_webhook_id: Option<String>,
    pub service_code: Option<String>,
    pub rate_per_minute: Option<f64>,
    pub billed_seconds: Option<i32>,
    pub billed_minutes: Option<f64>,
    pub credits_charged: Option<f64>,
    pub billing_status: Option<String>,
    pub billing_ledger_id: Option<Uuid>,
    pub billing_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCallResponse {
    pub call: VoiceCallRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartCallRequest {
    pub phone_number: String,
    pub destination: String,
    pub is_international: bool,
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
pub struct SpeakCallRequest {
    pub text: String,
    pub voice: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCallRequest {
    pub format: Option<String>,
    pub channels: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallActionResponse {
    pub accepted: bool,
    pub action: String,
    pub call_id: String,
    pub provider_response: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericCallActionRequest {
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceWebhookResponse {
    pub accepted: bool,
    pub event_type: String,
    pub webhook_id: Option<String>,
    pub call_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndCallRequest {
    pub call_id: String,
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
    State(state): State<AppState>,
    _account: AuthenticatedAccount,
    Path(id): Path<String>,
) -> GatewayResult<Json<VoiceCallResponse>> {
    let call = find_call(&state, &id).await?;
    Ok(Json(VoiceCallResponse { call }))
}

pub async fn start_call_session(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<StartCallRequest>,
) -> GatewayResult<Json<StartCallResponse>> {
    let service_code = if payload.is_international {
        "global_call"
    } else {
        "local_call"
    };
    let price = pricing::get_utility_price(&state, service_code).await?;
    let rate_per_minute = price.credit_cost.max(0.0);
    let balance = credit_balance(&state, account.account_id).await?;
    let estimated_minutes = if rate_per_minute > 0.0 {
        (balance / rate_per_minute).floor() as i64
    } else {
        0
    };
    let can_start = !payload.phone_number.trim().is_empty()
        && rate_per_minute > 0.0
        && balance >= rate_per_minute;

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
        credit_balance: balance,
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
    account: AuthenticatedAccount,
    Json(payload): Json<EndCallRequest>,
) -> GatewayResult<Json<EndCallResponse>> {
    let call = find_call(&state, &payload.call_id).await?;
    if call.account_id != Some(account.account_id) {
        return Err(GatewayError::Unauthorized);
    }
    let duration_seconds = i64::from(call.billed_seconds.unwrap_or(0).max(0));
    let credit_cost = call.credits_charged.unwrap_or(0.0);
    let remaining_credits = credit_balance(&state, account.account_id).await?;
    let confirmed = call.billing_status.as_deref() == Some("posted");

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
            "Call completed and billed from Telnyx webhook duration.".to_string()
        } else {
            "Call is not finalized yet. Billing waits for Telnyx call.hangup webhook.".to_string()
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
    let command_id = format!("cmd_{}", Uuid::new_v4().simple());

    debug!(
        call_id = %call_id,
        account_id = %account.account_id,
        sdp_bytes = offer.sdp.len(),
        destination = %offer.destination_number,
        "Processing real-time WebRTC signaling handshake"
    );

    let service_code = "global_call";
    let price = pricing::get_utility_price(&state, service_code).await?;
    let rate_per_minute = price.credit_cost.max(0.0);
    let balance = credit_balance(&state, account.account_id).await?;
    if balance < rate_per_minute {
        return Err(GatewayError::InsufficientCredits);
    }

    let from_number = if state.config.telnyx_from_number.trim().is_empty() {
        return Err(GatewayError::Config(
            "TELNYX_FROM_NUMBER is required for outbound voice calls".to_string(),
        ));
    } else {
        state.config.telnyx_from_number.trim().to_string()
    };

    let response = TelnyxClient::new(&state)
        .create_call(
            &offer.destination_number,
            &from_number,
            &call_id,
            &command_id,
        )
        .await?;
    let status = response.status();

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        error!(call_id = %call_id, account_id = %account.account_id, error = %err_text, "Telnyx outbound carrier connection rejected");
        log_provider_transaction(
            &state,
            "create_call",
            Some(account.account_id),
            "telnyx_voice_call",
            &call_id,
            None,
            None,
            Some(status.as_u16()),
            false,
            Some(&err_text),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx telephony infrastructure rejected execution: {}",
            err_text
        )));
    }

    let resp_json: serde_json::Value = response.json().await?;
    let telnyx_control_id = resp_json["data"]["call_control_id"]
        .as_str()
        .map(|s| s.to_string());
    let call_leg_id = resp_json["data"]["call_leg_id"]
        .as_str()
        .map(str::to_string);
    let call_session_id = resp_json["data"]["call_session_id"]
        .as_str()
        .map(str::to_string);

    upsert_outbound_call(
        &state,
        account.account_id,
        &call_id,
        &command_id,
        &from_number,
        &offer.destination_number,
        telnyx_control_id.as_deref(),
        call_leg_id.as_deref(),
        call_session_id.as_deref(),
        service_code,
        rate_per_minute,
        Some(&resp_json),
    )
    .await?;
    log_provider_transaction(
        &state,
        "create_call",
        Some(account.account_id),
        "telnyx_voice_call",
        &call_id,
        None,
        Some(resp_json.clone()),
        Some(status.as_u16()),
        true,
        None,
    )
    .await?;

    info!(call_id = %call_id, account_id = %account.account_id, "Outbound carrier connection successfully bridged");

    Ok(Json(WebRtcAnswer {
        call_id,
        status: "accepted".to_string(),
        telnyx_control_id,
    }))
}

pub async fn receive_voice_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> GatewayResult<Json<VoiceWebhookResponse>> {
    verify_telnyx_webhook(&state, &headers, &body)?;

    let raw_event: Value = serde_json::from_slice(&body)
        .map_err(|_| GatewayError::Upstream("Telnyx webhook body is invalid JSON".to_string()))?;
    let event = normalize_telnyx_event(&raw_event);
    let event_type = event.event_type;
    let webhook_id = event.webhook_id;
    let payload = event.payload;
    let call_id = payload
        .get("client_state")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let call_control_id = payload
        .get("call_control_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let call_leg_id = payload
        .get("call_leg_id")
        .and_then(Value::as_str)
        .or_else(|| raw_event.get("call_leg_id").and_then(Value::as_str))
        .map(str::to_string);
    let call_session_id = payload
        .get("call_session_id")
        .and_then(Value::as_str)
        .or_else(|| raw_event.get("call_session_id").and_then(Value::as_str))
        .map(str::to_string);
    let occurred_at = event.occurred_at.or_else(|| {
        parse_event_time(
            payload
                .get("occurred_at")
                .and_then(Value::as_str)
                .or_else(|| raw_event.get("event_timestamp").and_then(Value::as_str)),
        )
    });

    sqlx::query(
        r#"
        insert into telnyx_voice_events (
            webhook_id,
            event_type,
            call_id,
            call_control_id,
            call_leg_id,
            call_session_id,
            occurred_at,
            payload,
            raw_event
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        on conflict (webhook_id) do update
        set raw_event = excluded.raw_event
        "#,
    )
    .bind(webhook_id.as_deref())
    .bind(&event_type)
    .bind(call_id.as_deref())
    .bind(call_control_id.as_deref())
    .bind(call_leg_id.as_deref())
    .bind(call_session_id.as_deref())
    .bind(occurred_at)
    .bind(&payload)
    .bind(&raw_event)
    .execute(&state.db)
    .await?;

    apply_voice_event_to_call(
        &state,
        &event_type,
        webhook_id.as_deref(),
        call_id.as_deref(),
        call_control_id.as_deref(),
        call_leg_id.as_deref(),
        call_session_id.as_deref(),
        occurred_at,
        &payload,
        &raw_event,
    )
    .await?;

    Ok(Json(VoiceWebhookResponse {
        accepted: true,
        event_type,
        webhook_id,
        call_id,
    }))
}

struct NormalizedTelnyxEvent {
    event_type: String,
    webhook_id: Option<String>,
    occurred_at: Option<DateTime<Utc>>,
    payload: Value,
}

fn normalize_telnyx_event(raw_event: &Value) -> NormalizedTelnyxEvent {
    let event = raw_event
        .get("data")
        .or_else(|| {
            raw_event
                .get("metadata")
                .and_then(|metadata| metadata.get("event"))
        })
        .unwrap_or(raw_event);

    let event_type = event
        .get("event_type")
        .and_then(Value::as_str)
        .or_else(|| raw_event.get("event_type").and_then(Value::as_str))
        .or_else(|| raw_event.get("name").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let webhook_id = raw_event
        .get("webhook_id")
        .and_then(Value::as_str)
        .or_else(|| event.get("id").and_then(Value::as_str))
        .map(str::to_string);
    let occurred_at = parse_event_time(
        event
            .get("occurred_at")
            .and_then(Value::as_str)
            .or_else(|| raw_event.get("event_timestamp").and_then(Value::as_str)),
    );
    let payload = event.get("payload").cloned().unwrap_or(Value::Null);

    NormalizedTelnyxEvent {
        event_type,
        webhook_id,
        occurred_at,
        payload,
    }
}

pub async fn speak_call(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(id): Path<String>,
    Json(request): Json<SpeakCallRequest>,
) -> GatewayResult<Json<CallActionResponse>> {
    let call = find_call(&state, &id).await?;
    if call.account_id != Some(account.account_id) {
        return Err(GatewayError::Unauthorized);
    }
    let call_control_id = call_control_id(&call)?;
    let text = request.text.trim();
    if text.is_empty() {
        return Err(GatewayError::Upstream(
            "text is required for speak action".to_string(),
        ));
    }

    let payload = json!({
        "payload": text,
        "voice": request.voice.unwrap_or_else(|| "female".to_string()),
        "language": request.language.unwrap_or_else(|| "en-US".to_string()),
        "command_id": format!("cmd_{}", Uuid::new_v4().simple()),
    });
    post_call_action(
        &state,
        Some(account.account_id),
        &call.call_id,
        &call_control_id,
        "speak",
        payload,
    )
    .await
}

pub async fn record_call(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(id): Path<String>,
    Json(request): Json<RecordCallRequest>,
) -> GatewayResult<Json<CallActionResponse>> {
    let call = find_call(&state, &id).await?;
    if call.account_id != Some(account.account_id) {
        return Err(GatewayError::Unauthorized);
    }
    let call_control_id = call_control_id(&call)?;
    let payload = json!({
        "format": request.format.unwrap_or_else(|| "mp3".to_string()),
        "channels": request.channels.unwrap_or_else(|| "single".to_string()),
        "command_id": format!("cmd_{}", Uuid::new_v4().simple()),
    });
    post_call_action(
        &state,
        Some(account.account_id),
        &call.call_id,
        &call_control_id,
        "record_start",
        payload,
    )
    .await
}

pub async fn hangup_call(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(id): Path<String>,
) -> GatewayResult<Json<CallActionResponse>> {
    let call = find_call(&state, &id).await?;
    if call.account_id != Some(account.account_id) {
        return Err(GatewayError::Unauthorized);
    }
    let call_control_id = call_control_id(&call)?;
    let payload = json!({
        "command_id": format!("cmd_{}", Uuid::new_v4().simple()),
    });
    post_call_action(
        &state,
        Some(account.account_id),
        &call.call_id,
        &call_control_id,
        "hangup",
        payload,
    )
    .await
}

pub async fn send_call_action(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path((id, action)): Path<(String, String)>,
    Json(request): Json<GenericCallActionRequest>,
) -> GatewayResult<Json<CallActionResponse>> {
    let call = find_call(&state, &id).await?;
    if call.account_id != Some(account.account_id) {
        return Err(GatewayError::Unauthorized);
    }
    let action = normalize_call_action(&action)?;
    let call_control_id = call_control_id(&call)?;
    let mut payload = match request.payload {
        Value::Object(map) => Value::Object(map),
        Value::Null => json!({}),
        _ => {
            return Err(GatewayError::Upstream(
                "call action payload must be a JSON object".to_string(),
            ));
        }
    };
    if let Some(map) = payload.as_object_mut() {
        map.entry("command_id")
            .or_insert_with(|| Value::String(format!("cmd_{}", Uuid::new_v4().simple())));
    }

    post_call_action(
        &state,
        Some(account.account_id),
        &call.call_id,
        &call_control_id,
        &action,
        payload,
    )
    .await
}

async fn post_call_action(
    state: &AppState,
    account_id: Option<Uuid>,
    call_id: &str,
    call_control_id: &str,
    action: &str,
    payload: Value,
) -> GatewayResult<Json<CallActionResponse>> {
    let response = TelnyxClient::new(state)
        .call_action(call_control_id, action, &payload)
        .await?;
    let status = response.status();
    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        log_provider_transaction(
            state,
            action,
            account_id,
            "telnyx_voice_call",
            call_id,
            Some(payload),
            None,
            Some(status.as_u16()),
            false,
            Some(&err_text),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx call action {action} failed: {err_text}"
        )));
    }
    let provider_response: Value = response.json().await?;
    log_provider_transaction(
        state,
        action,
        account_id,
        "telnyx_voice_call",
        call_id,
        Some(payload),
        Some(provider_response.clone()),
        Some(status.as_u16()),
        true,
        None,
    )
    .await?;

    Ok(Json(CallActionResponse {
        accepted: true,
        action: action.to_string(),
        call_id: call_id.to_string(),
        provider_response,
    }))
}

async fn find_call(state: &AppState, id: &str) -> GatewayResult<VoiceCallRecord> {
    sqlx::query_as::<_, VoiceCallRecord>(
        r#"
        select id,
               account_id,
               call_id,
               command_id,
               call_control_id,
               call_leg_id,
               call_session_id,
               connection_id,
               direction,
               from_number,
               to_number,
               status,
               telnyx_state,
               hangup_cause,
               hangup_source,
               started_at,
               answered_at,
               ended_at,
               last_event_type,
               last_webhook_id,
               service_code,
               rate_per_minute::float8 as rate_per_minute,
               billed_seconds,
               billed_minutes::float8 as billed_minutes,
               credits_charged::float8 as credits_charged,
               billing_status,
               billing_ledger_id,
               billing_error,
               created_at,
               updated_at
        from telnyx_voice_calls
        where call_id = $1 or call_control_id = $1
        limit 1
        "#,
    )
    .bind(id.trim())
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("voice call not found".to_string()))
}

async fn upsert_outbound_call(
    state: &AppState,
    account_id: Uuid,
    call_id: &str,
    command_id: &str,
    from_number: &str,
    to_number: &str,
    call_control_id: Option<&str>,
    call_leg_id: Option<&str>,
    call_session_id: Option<&str>,
    service_code: &str,
    rate_per_minute: f64,
    raw_response: Option<&Value>,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        insert into telnyx_voice_calls (
            account_id,
            call_id,
            command_id,
            call_control_id,
            call_leg_id,
            call_session_id,
            connection_id,
            direction,
            from_number,
            to_number,
            status,
            service_code,
            rate_per_minute,
            billing_status,
            last_raw_event
        ) values ($1, $2, $3, $4, $5, $6, $7, 'outgoing', $8, $9, 'initiated', $10, $11, 'pending', $12)
        on conflict (call_id) do update
        set command_id = excluded.command_id,
            call_control_id = coalesce(excluded.call_control_id, telnyx_voice_calls.call_control_id),
            call_leg_id = coalesce(excluded.call_leg_id, telnyx_voice_calls.call_leg_id),
            call_session_id = coalesce(excluded.call_session_id, telnyx_voice_calls.call_session_id),
            service_code = excluded.service_code,
            rate_per_minute = excluded.rate_per_minute,
            billing_status = case
                when telnyx_voice_calls.billing_status = 'not_billed' then excluded.billing_status
                else telnyx_voice_calls.billing_status
            end,
            status = excluded.status,
            last_raw_event = coalesce(excluded.last_raw_event, telnyx_voice_calls.last_raw_event),
            updated_at = now()
        "#,
    )
    .bind(account_id)
    .bind(call_id)
    .bind(command_id)
    .bind(call_control_id)
    .bind(call_leg_id)
    .bind(call_session_id)
    .bind(&state.config.telnyx_connection_id)
    .bind(from_number)
    .bind(to_number)
    .bind(service_code)
    .bind(rate_per_minute)
    .bind(raw_response)
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn finalize_call_billing(state: &AppState, call_id: &str) -> GatewayResult<()> {
    let call = find_call(state, call_id).await?;
    if call.billing_status.as_deref() == Some("posted") {
        return Ok(());
    }

    let Some(account_id) = call.account_id else {
        return Ok(());
    };
    let Some(rate_per_minute) = call.rate_per_minute else {
        return Ok(());
    };

    let started_at = call.started_at.or(call.answered_at);
    let ended_at = call.ended_at;
    let Some(started_at) = started_at else {
        return Ok(());
    };
    let Some(ended_at) = ended_at else {
        return Ok(());
    };

    let duration_seconds = (ended_at - started_at).num_seconds().max(0) as i32;
    let billed_minutes = ((duration_seconds as f64) / 60.0).ceil().max(1.0);
    let credits_charged = round_credits(billed_minutes * rate_per_minute);
    let source_reference = format!("telnyx_voice_call:{}", call.call_id);

    match debit_credits(
        state,
        account_id,
        credits_charged,
        "telnyx_voice_call",
        &source_reference,
        "Telnyx voice call duration billing",
        json!({
            "callId": call.call_id,
            "callControlId": call.call_control_id,
            "durationSeconds": duration_seconds,
            "billedMinutes": billed_minutes,
            "ratePerMinute": rate_per_minute
        }),
    )
    .await
    {
        Ok(debit) => {
            sqlx::query(
                r#"
                update telnyx_voice_calls
                set billed_seconds = $1,
                    billed_minutes = $2,
                    credits_charged = $3,
                    billing_status = 'posted',
                    billing_ledger_id = $4,
                    billing_error = null,
                    updated_at = now()
                where call_id = $5
                "#,
            )
            .bind(duration_seconds)
            .bind(billed_minutes)
            .bind(credits_charged)
            .bind(debit.ledger_id)
            .bind(&call.call_id)
            .execute(&state.db)
            .await?;
        }
        Err(err) => {
            sqlx::query(
                r#"
                update telnyx_voice_calls
                set billed_seconds = $1,
                    billed_minutes = $2,
                    credits_charged = $3,
                    billing_status = 'failed',
                    billing_error = $4,
                    updated_at = now()
                where call_id = $5
                "#,
            )
            .bind(duration_seconds)
            .bind(billed_minutes)
            .bind(credits_charged)
            .bind(err.to_string())
            .bind(&call.call_id)
            .execute(&state.db)
            .await?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_voice_event_to_call(
    state: &AppState,
    event_type: &str,
    webhook_id: Option<&str>,
    call_id: Option<&str>,
    call_control_id: Option<&str>,
    call_leg_id: Option<&str>,
    call_session_id: Option<&str>,
    occurred_at: Option<DateTime<Utc>>,
    payload: &Value,
    raw_event: &Value,
) -> GatewayResult<()> {
    let Some(identifier) = call_id.or(call_control_id) else {
        return Ok(());
    };

    let status = status_for_event(event_type);
    let from_number = payload.get("from").and_then(Value::as_str).unwrap_or("");
    let to_number = payload.get("to").and_then(Value::as_str).unwrap_or("");
    let connection_id = payload.get("connection_id").and_then(Value::as_str);
    let telnyx_state = payload.get("state").and_then(Value::as_str);
    let hangup_cause = payload.get("hangup_cause").and_then(Value::as_str);
    let hangup_source = payload.get("hangup_source").and_then(Value::as_str);
    let started_at = parse_event_time(payload.get("start_time").and_then(Value::as_str));
    let ended_at = parse_event_time(payload.get("end_time").and_then(Value::as_str));

    sqlx::query(
        r#"
        insert into telnyx_voice_calls (
            call_id,
            call_control_id,
            call_leg_id,
            call_session_id,
            connection_id,
            direction,
            from_number,
            to_number,
            status,
            telnyx_state,
            hangup_cause,
            hangup_source,
            started_at,
            answered_at,
            ended_at,
            last_event_type,
            last_webhook_id,
            last_raw_event
        ) values ($1, $2, $3, $4, $5, coalesce($6, 'unknown'), $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        on conflict (call_id) do update
        set call_control_id = coalesce(excluded.call_control_id, telnyx_voice_calls.call_control_id),
            call_leg_id = coalesce(excluded.call_leg_id, telnyx_voice_calls.call_leg_id),
            call_session_id = coalesce(excluded.call_session_id, telnyx_voice_calls.call_session_id),
            connection_id = coalesce(excluded.connection_id, telnyx_voice_calls.connection_id),
            status = excluded.status,
            telnyx_state = coalesce(excluded.telnyx_state, telnyx_voice_calls.telnyx_state),
            hangup_cause = coalesce(excluded.hangup_cause, telnyx_voice_calls.hangup_cause),
            hangup_source = coalesce(excluded.hangup_source, telnyx_voice_calls.hangup_source),
            started_at = coalesce(telnyx_voice_calls.started_at, excluded.started_at),
            answered_at = coalesce(telnyx_voice_calls.answered_at, excluded.answered_at),
            ended_at = coalesce(telnyx_voice_calls.ended_at, excluded.ended_at),
            last_event_type = excluded.last_event_type,
            last_webhook_id = excluded.last_webhook_id,
            last_raw_event = excluded.last_raw_event,
            updated_at = now()
        "#,
    )
    .bind(identifier)
    .bind(call_control_id)
    .bind(call_leg_id)
    .bind(call_session_id)
    .bind(connection_id)
    .bind(payload.get("direction").and_then(Value::as_str))
    .bind(from_number)
    .bind(to_number)
    .bind(status)
    .bind(telnyx_state)
    .bind(hangup_cause)
    .bind(hangup_source)
    .bind(started_at.or(occurred_at))
    .bind(if event_type == "call.answered" {
        occurred_at
    } else {
        None
    })
    .bind(ended_at.or_else(|| {
        if event_type == "call.hangup" {
            occurred_at
        } else {
            None
        }
    }))
    .bind(event_type)
    .bind(webhook_id)
    .bind(raw_event)
    .execute(&state.db)
    .await?;

    if event_type == "call.hangup" {
        finalize_call_billing(state, identifier).await?;
    }

    Ok(())
}

fn call_control_id(call: &VoiceCallRecord) -> GatewayResult<String> {
    call.call_control_id
        .clone()
        .ok_or_else(|| GatewayError::Upstream("call_control_id is not available yet".to_string()))
}

fn normalize_call_action(action: &str) -> GatewayResult<String> {
    let action = action.trim().to_lowercase().replace('-', "_");
    let allowed = matches!(
        action.as_str(),
        "answer"
            | "fork_start"
            | "fork_stop"
            | "hangup"
            | "reject"
            | "transfer"
            | "suppression_start"
            | "suppression_stop"
            | "client_state_update"
            | "bridge"
            | "ai_assistant_start"
            | "ai_assistant_stop"
            | "enqueue"
            | "leave_queue"
            | "gather_using_audio"
            | "gather_using_speak"
            | "gather_using_ai"
            | "gather_stop"
            | "playback_start"
            | "playback_stop"
            | "record_start"
            | "record_stop"
            | "record_pause"
            | "record_resume"
            | "refer"
            | "send_dtmf"
            | "send_sip_info"
            | "speak"
            | "streaming_start"
            | "streaming_stop"
            | "transcription_start"
            | "transcription_stop"
            | "siprec_start"
            | "siprec_stop"
    );

    if allowed {
        Ok(action)
    } else {
        Err(GatewayError::Upstream(format!(
            "unsupported Telnyx call action: {action}"
        )))
    }
}

fn status_for_event(event_type: &str) -> &'static str {
    match event_type {
        "call.initiated" => "initiated",
        "call.answered" => "answered",
        "call.hold" => "held",
        "call.unhold" => "answered",
        "call.hangup" => "completed",
        "call.bridged" => "bridged",
        "call.playback.started" => "playback_started",
        "call.playback.ended" => "playback_ended",
        "call.speak.started" => "speak_started",
        "call.speak.ended" => "speak_ended",
        "call.gather.ended" => "gather_ended",
        "call.recording.saved" => "recorded",
        "call.fork.started" => "fork_started",
        "call.fork.stopped" => "fork_stopped",
        "call.enqueued" => "enqueued",
        "call.dequeued" => "dequeued",
        "streaming.started" => "streaming_started",
        "streaming.stopped" => "streaming_stopped",
        _ => "updated",
    }
}

fn parse_event_time(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

pub(crate) fn verify_telnyx_webhook(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> GatewayResult<()> {
    let public_key = state.config.telnyx_webhook_public_key.trim();
    if !public_key.is_empty() {
        return verify_telnyx_ed25519(public_key, headers, body);
    }

    let secret = state.config.telnyx_webhook_signing_secret.trim();
    if !secret.is_empty() {
        return verify_telnyx_hmac(secret, headers, body);
    }

    if telnyx_webhook_verification_required() {
        return Err(GatewayError::Config(
            "TELNYX_WEBHOOK_PUBLIC_KEY or TELNYX_WEBHOOK_SIGNING_SECRET is required in production"
                .to_string(),
        ));
    }

    Ok(())
}

fn verify_telnyx_ed25519(public_key: &str, headers: &HeaderMap, body: &[u8]) -> GatewayResult<()> {
    let timestamp = required_header(headers, "telnyx-timestamp")?;
    let signature = required_header(headers, "telnyx-signature-ed25519")?;
    let key_bytes = hex_to_array_32(public_key)?;
    let signature_bytes = STANDARD
        .decode(signature)
        .map_err(|_| GatewayError::Unauthorized)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| GatewayError::Unauthorized)?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|_| GatewayError::Unauthorized)?;
    let signed_payload = signed_webhook_payload(timestamp, body);

    verifying_key
        .verify(&signed_payload, &signature)
        .map_err(|_| GatewayError::Unauthorized)
}

fn verify_telnyx_hmac(secret: &str, headers: &HeaderMap, body: &[u8]) -> GatewayResult<()> {
    let timestamp = required_header(headers, "telnyx-timestamp")?;
    let signature = required_header(headers, "telnyx-signature")?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| GatewayError::Upstream("invalid Telnyx webhook secret".to_string()))?;
    mac.update(&signed_webhook_payload(timestamp, body));
    let expected = bytes_to_hex(&mac.finalize().into_bytes());

    if constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        Ok(())
    } else {
        Err(GatewayError::Unauthorized)
    }
}

fn signed_webhook_payload(timestamp: &str, body: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(timestamp.len() + 1 + body.len());
    payload.extend_from_slice(timestamp.as_bytes());
    payload.push(b'|');
    payload.extend_from_slice(body);
    payload
}

fn required_header<'a>(headers: &'a HeaderMap, key: &str) -> GatewayResult<&'a str> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or(GatewayError::Unauthorized)
}

fn hex_to_array_32(value: &str) -> GatewayResult<[u8; 32]> {
    let value = value.trim().trim_start_matches("0x");
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(GatewayError::Config(
            "TELNYX_WEBHOOK_PUBLIC_KEY must be a 32-byte hex public key".to_string(),
        ));
    }

    let mut out = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|_| GatewayError::Unauthorized)?;
        out[index] = u8::from_str_radix(pair, 16).map_err(|_| GatewayError::Unauthorized)?;
    }
    Ok(out)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn telnyx_webhook_verification_required() -> bool {
    std::env::var("APP_ENV")
        .or_else(|_| std::env::var("RUST_ENV"))
        .or_else(|_| std::env::var("ENV"))
        .map(|value| {
            let value = value.trim().to_lowercase();
            value == "production" || value == "prod"
        })
        .unwrap_or(false)
}

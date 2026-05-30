// src/domains/telecom/sms.rs
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        pricing,
        telecom::{
            billing::{
                credit_credits, debit_credits, estimate_telnyx_economics, log_provider_transaction,
                log_provider_transaction_with_economics, round_credits,
                telnyx_sms_estimated_usd_cost,
            },
            voice::verify_telnyx_webhook,
        },
    },
    error::{GatewayError, GatewayResult},
    providers::TelnyxClient,
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
    pub balance_after: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundSmsWebhook {
    pub to: Option<String>,
    pub from: Option<String>,
    pub body: Option<String>,
    pub text: Option<String>,
    pub message_id: Option<String>,
    pub provider_message_id: Option<String>,
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsInboxQuery {
    pub phone_number: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InboundSmsMessage {
    pub id: Uuid,
    pub phone_number: String,
    pub sender: String,
    pub body: String,
    pub provider_message_id: Option<String>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundSmsWebhookResponse {
    pub accepted: bool,
    pub message_id: Uuid,
    pub phone_number: String,
    pub sender: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsInboxResponse {
    pub phone_number: String,
    pub messages: Vec<InboundSmsMessage>,
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

    let parts_billed = ((body_len as f64) / 160.0).ceil() as usize;
    let sms_price = pricing::get_utility_price(&state, "sms_outbound").await?;
    let cost_per_segment = sms_price.credit_cost;
    let total_sms_cost = round_credits((parts_billed as f64) * cost_per_segment);
    let economics =
        estimate_telnyx_economics(total_sms_cost, telnyx_sms_estimated_usd_cost(parts_billed));
    let source_reference = format!("sms_{}", Uuid::new_v4().simple());
    let debit = debit_credits(
        &state,
        account.account_id,
        total_sms_cost,
        "telnyx_sms",
        &source_reference,
        "Outbound Telnyx SMS",
        serde_json::json!({
            "to": request.to,
            "from": request.from,
            "partsBilled": parts_billed,
            "costPerSegment": cost_per_segment
        }),
    )
    .await?;

    tracing::debug!(
        account_id = %account.account_id,
        to = %request.to,
        parts = parts_billed,
        cost = total_sms_cost,
        "Pre-flight SMS balance debited using backend pricing, dispatching message to downstream carrier"
    );

    let response = TelnyxClient::new(&state)
        .send_sms(&request.to, &request.from, &request.body)
        .await?;
    let status = response.status();

    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        tracing::error!(account_id = %account.account_id, error = %err_text, "Telnyx SMS gateway delivery rejected");
        log_provider_transaction(
            &state,
            "send_sms",
            Some(account.account_id),
            "telnyx_sms",
            &source_reference,
            None,
            None,
            Some(status.as_u16()),
            false,
            Some(&err_text),
        )
        .await?;
        credit_credits(
            &state,
            account.account_id,
            total_sms_cost,
            "telnyx_sms_reversal",
            &format!("{source_reference}:provider_rejected"),
            "Reversal for rejected Telnyx SMS",
            serde_json::json!({
                "originalSourceReference": source_reference,
                "providerStatus": status.as_u16(),
                "providerError": err_text.clone()
            }),
        )
        .await?;

        return Err(GatewayError::Upstream(format!(
            "Telnyx messaging infrastructure failure: {err_text}"
        )));
    }

    let resp_json: serde_json::Value = response.json().await.map_err(GatewayError::Http)?;
    let message_id = resp_json["data"]["id"]
        .as_str()
        .unwrap_or("unknown_carrier_id")
        .to_string();
    log_provider_transaction_with_economics(
        &state,
        "send_sms",
        Some(account.account_id),
        "telnyx_sms",
        &source_reference,
        None,
        Some(resp_json.clone()),
        Some(status.as_u16()),
        true,
        None,
        Some(&economics),
    )
    .await?;

    Ok(Json(SmsResponse {
        message_id,
        routed: true,
        parts_billed,
        credits_deducted: total_sms_cost,
        balance_after: debit.balance_after,
    }))
}

pub async fn receive_inbound_sms(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> GatewayResult<Json<InboundSmsWebhookResponse>> {
    verify_telnyx_webhook(&state, &headers, &body)?;
    let payload: InboundSmsWebhook = serde_json::from_slice(&body).map_err(|_| {
        GatewayError::Upstream("Telnyx SMS webhook body is invalid JSON".to_string())
    })?;
    let phone_number = extract_to_number(&payload)?;
    let sender = extract_from_number(&payload)?;
    let body = extract_body(&payload)?;
    let provider_message_id = extract_provider_message_id(&payload);
    let provider_payload = serde_json::to_value(&payload.data).unwrap_or(Value::Null);

    let record = sqlx::query_as::<_, InboundSmsMessage>(
        r#"
        insert into inbound_sms_messages (
            phone_number,
            sender,
            body,
            provider_message_id,
            provider_payload
        )
        values ($1, $2, $3, $4, $5)
        on conflict (provider_message_id) where provider_message_id is not null do update
        set body = excluded.body,
            provider_payload = excluded.provider_payload
        returning id, phone_number, sender, body, provider_message_id, received_at
        "#,
    )
    .bind(phone_number)
    .bind(sender)
    .bind(body)
    .bind(provider_message_id)
    .bind(provider_payload)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(InboundSmsWebhookResponse {
        accepted: true,
        message_id: record.id,
        phone_number: record.phone_number,
        sender: record.sender,
    }))
}

pub async fn get_sms_inbox(
    State(state): State<AppState>,
    _account: AuthenticatedAccount,
    Query(query): Query<SmsInboxQuery>,
) -> GatewayResult<Json<SmsInboxResponse>> {
    let phone_number = normalize_phone_number(&query.phone_number)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let messages = sqlx::query_as::<_, InboundSmsMessage>(
        r#"
        select id, phone_number, sender, body, provider_message_id, received_at
        from inbound_sms_messages
        where phone_number = $1
        order by received_at desc
        limit $2
        "#,
    )
    .bind(&phone_number)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(SmsInboxResponse {
        phone_number,
        messages,
    }))
}

fn extract_to_number(payload: &InboundSmsWebhook) -> GatewayResult<String> {
    if let Some(value) = payload.to.as_deref() {
        return normalize_phone_number(value);
    }

    if let Some(value) = payload
        .data
        .as_ref()
        .and_then(|data| data.get("payload"))
        .and_then(|payload| payload.get("to"))
        .and_then(Value::as_str)
    {
        return normalize_phone_number(value);
    }

    if let Some(value) = payload
        .data
        .as_ref()
        .and_then(|data| data.get("to"))
        .and_then(Value::as_str)
    {
        return normalize_phone_number(value);
    }

    Err(GatewayError::Upstream(
        "Inbound SMS webhook missing recipient number".to_string(),
    ))
}

fn extract_from_number(payload: &InboundSmsWebhook) -> GatewayResult<String> {
    if let Some(value) = payload.from.as_deref() {
        return normalize_phone_number(value);
    }

    if let Some(value) = payload
        .data
        .as_ref()
        .and_then(|data| data.get("payload"))
        .and_then(|payload| payload.get("from"))
        .and_then(Value::as_str)
    {
        return normalize_phone_number(value);
    }

    if let Some(value) = payload
        .data
        .as_ref()
        .and_then(|data| data.get("from"))
        .and_then(Value::as_str)
    {
        return normalize_phone_number(value);
    }

    Err(GatewayError::Upstream(
        "Inbound SMS webhook missing sender number".to_string(),
    ))
}

fn extract_body(payload: &InboundSmsWebhook) -> GatewayResult<String> {
    if let Some(value) = payload.body.as_deref().or(payload.text.as_deref()) {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }

    if let Some(value) = payload
        .data
        .as_ref()
        .and_then(|data| data.get("payload"))
        .and_then(|payload| payload.get("text"))
        .and_then(Value::as_str)
    {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }

    Err(GatewayError::Upstream(
        "Inbound SMS webhook missing message body".to_string(),
    ))
}

fn extract_provider_message_id(payload: &InboundSmsWebhook) -> Option<String> {
    payload
        .provider_message_id
        .clone()
        .or_else(|| payload.message_id.clone())
        .or_else(|| {
            payload
                .data
                .as_ref()
                .and_then(|data| data.get("payload"))
                .and_then(|payload| payload.get("id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| {
            payload
                .data
                .as_ref()
                .and_then(|data| data.get("id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn normalize_phone_number(phone_number: &str) -> GatewayResult<String> {
    let phone_number = phone_number.trim();
    let valid = phone_number.starts_with('+')
        && phone_number.len() <= 32
        && phone_number.chars().skip(1).all(|ch| ch.is_ascii_digit());

    if valid {
        Ok(phone_number.to_string())
    } else {
        Err(GatewayError::Upstream(
            "phone number must be in E.164 format, for example +13125551234".to_string(),
        ))
    }
}

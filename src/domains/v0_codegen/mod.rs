use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domains::{
        pricing,
        telecom::billing::{credit_credits, debit_credits, log_named_provider_transaction},
    },
    error::{GatewayError, GatewayResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v0/status", get(v0_status))
        .route("/v0/chats/quote", post(v0_quote))
        .route("/v0/chats/create", post(v0_create_chat))
        .route("/v0/chats/result/{reference}", get(v0_result))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct V0StatusResponse {
    configured: bool,
    base_url: String,
    api_key_configured: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct V0QuoteRequest {
    mode: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct V0QuoteResponse {
    accepted: bool,
    service_code: String,
    credit_cost: f64,
    mode: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct V0CreateChatRequest {
    account_id: Uuid,
    message: String,
    system: Option<String>,
    chat_privacy: Option<String>,
    model_configuration: Option<Value>,
    ref_id: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct V0GenerationRecord {
    id: Uuid,
    account_id: Uuid,
    request_reference: String,
    v0_chat_id: Option<String>,
    status: String,
    prompt: String,
    mode: String,
    credit_cost: f64,
    request_payload: Value,
    provider_response: Value,
    error_message: Option<String>,
}

async fn v0_status() -> Json<V0StatusResponse> {
    let key = std::env::var("V0_API_KEY").unwrap_or_default();
    Json(V0StatusResponse {
        configured: !key.trim().is_empty(),
        base_url: v0_base_url(),
        api_key_configured: !key.trim().is_empty(),
        message: if key.trim().is_empty() {
            "V0_API_KEY is required before live v0 code generation.".to_string()
        } else {
            "Vercel v0 Platform API is configured for chat generation.".to_string()
        },
    })
}

async fn v0_quote(
    State(state): State<AppState>,
    Json(payload): Json<V0QuoteRequest>,
) -> GatewayResult<Json<V0QuoteResponse>> {
    let credit_cost = pricing::get_utility_price(&state, "v0_code_generation")
        .await?
        .credit_cost;
    Ok(Json(V0QuoteResponse {
        accepted: true,
        service_code: "v0_code_generation".to_string(),
        credit_cost,
        mode: payload.mode.unwrap_or_else(|| "create_chat".to_string()),
        message: "v0 code-generation quote generated. No Credits debited yet.".to_string(),
    }))
}

async fn v0_create_chat(
    State(state): State<AppState>,
    Json(payload): Json<V0CreateChatRequest>,
) -> GatewayResult<Json<V0GenerationRecord>> {
    if payload.message.trim().is_empty() {
        return Err(GatewayError::Upstream(
            "message is required for v0 chat generation".to_string(),
        ));
    }
    if payload.message.chars().count() < 20 {
        return Err(GatewayError::Upstream(
            "message must be at least 20 characters".to_string(),
        ));
    }

    let credit_cost = pricing::get_utility_price(&state, "v0_code_generation")
        .await?
        .credit_cost;
    let reference = payload
        .ref_id
        .clone()
        .unwrap_or_else(|| format!("v0_{}", Uuid::new_v4().simple()));
    if let Some(existing) = find_v0_record(&state, &reference).await? {
        return Ok(Json(existing));
    }
    let request_payload = build_v0_payload(&payload);

    debit_credits(
        &state,
        payload.account_id,
        credit_cost,
        "v0_code_generation",
        &reference,
        "Vercel v0 code generation",
        json!({
            "messageLength": payload.message.chars().count(),
            "chatPrivacy": payload.chat_privacy,
            "hasSystem": payload.system.is_some(),
            "hasModelConfiguration": payload.model_configuration.is_some()
        }),
    )
    .await?;

    let provider_response = match submit_to_v0(&state, request_payload.clone()).await {
        Ok(value) => value,
        Err(err) => {
            let error = err.to_string();
            credit_credits(
                &state,
                payload.account_id,
                credit_cost,
                "v0_code_generation_reversal",
                &format!("{reference}:provider_rejected"),
                "Reversal for rejected v0 code generation",
                json!({ "requestReference": reference, "error": error }),
            )
            .await?;
            log_named_provider_transaction(
                &state,
                "v0",
                "create_chat",
                Some(payload.account_id),
                "v0_code_generation",
                &reference,
                Some(request_payload.clone()),
                None,
                None,
                false,
                Some(&error),
            )
            .await?;
            return Err(err);
        }
    };
    let submitted = provider_response
        .get("submitted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let http_status = provider_response
        .get("httpStatus")
        .and_then(Value::as_u64)
        .map(|value| value as u16);
    if !submitted {
        credit_credits(
            &state,
            payload.account_id,
            credit_cost,
            "v0_code_generation_reversal",
            &format!("{reference}:provider_rejected"),
            "Reversal for rejected v0 code generation",
            json!({ "requestReference": reference, "providerResponse": provider_response }),
        )
        .await?;
        log_named_provider_transaction(
            &state,
            "v0",
            "create_chat",
            Some(payload.account_id),
            "v0_code_generation",
            &reference,
            Some(request_payload.clone()),
            Some(provider_response.clone()),
            http_status,
            false,
            Some("v0 create chat rejected"),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "v0 create chat rejected: {provider_response}"
        )));
    }
    log_named_provider_transaction(
        &state,
        "v0",
        "create_chat",
        Some(payload.account_id),
        "v0_code_generation",
        &reference,
        Some(request_payload.clone()),
        Some(provider_response.clone()),
        http_status,
        true,
        None,
    )
    .await?;
    let status = "submitted";
    let v0_chat_id = extract_v0_chat_id(&provider_response);

    let record = sqlx::query_as::<_, V0GenerationRecord>(
        r#"
        insert into v0_generation_requests (
            account_id, request_reference, v0_chat_id, status, prompt, mode,
            credit_cost, request_payload, provider_response
        ) values ($1,$2,$3,$4,$5,'create_chat',$6,$7,$8)
        on conflict (request_reference) do update set
            provider_response = excluded.provider_response,
            status = excluded.status,
            updated_at = now()
        returning id, account_id, request_reference, v0_chat_id, status, prompt, mode,
                  credit_cost::float8 as credit_cost, request_payload, provider_response, error_message
        "#,
    )
    .bind(payload.account_id)
    .bind(&reference)
    .bind(v0_chat_id)
    .bind(status)
    .bind(payload.message)
    .bind(credit_cost)
    .bind(request_payload)
    .bind(provider_response)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(record))
}

async fn find_v0_record(
    state: &AppState,
    reference: &str,
) -> GatewayResult<Option<V0GenerationRecord>> {
    sqlx::query_as::<_, V0GenerationRecord>(
        r#"
        select id, account_id, request_reference, v0_chat_id, status, prompt, mode,
               credit_cost::float8 as credit_cost, request_payload, provider_response, error_message
        from v0_generation_requests
        where request_reference = $1
        limit 1
        "#,
    )
    .bind(reference)
    .fetch_optional(&state.db)
    .await
    .map_err(Into::into)
}

async fn v0_result(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> GatewayResult<Json<V0GenerationRecord>> {
    let record = sqlx::query_as::<_, V0GenerationRecord>(
        r#"
        select id, account_id, request_reference, v0_chat_id, status, prompt, mode,
               credit_cost::float8 as credit_cost, request_payload, provider_response, error_message
        from v0_generation_requests
        where request_reference = $1 or v0_chat_id = $1
        limit 1
        "#,
    )
    .bind(reference)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("v0 generation request not found".to_string()))?;

    Ok(Json(record))
}

fn build_v0_payload(payload: &V0CreateChatRequest) -> Value {
    let mut body = json!({ "message": payload.message });
    if let Some(system) = &payload.system {
        body["system"] = Value::String(system.clone());
    }
    if let Some(chat_privacy) = &payload.chat_privacy {
        body["chatPrivacy"] = Value::String(chat_privacy.clone());
    }
    if let Some(model_configuration) = &payload.model_configuration {
        body["modelConfiguration"] = model_configuration.clone();
    }
    body
}

async fn submit_to_v0(state: &AppState, payload: Value) -> GatewayResult<Value> {
    let key = std::env::var("V0_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        return Err(GatewayError::Config("V0_API_KEY is required".to_string()));
    }
    let response = state
        .http
        .post(format!("{}/v1/chats", v0_base_url()))
        .bearer_auth(key)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
    Ok(json!({ "submitted": status.is_success(), "httpStatus": status.as_u16(), "body": body }))
}

fn extract_v0_chat_id(value: &Value) -> Option<String> {
    value
        .get("body")
        .and_then(|body| body.get("id").or_else(|| body.get("chatId")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn v0_base_url() -> String {
    std::env::var("V0_BASE_URL").unwrap_or_else(|_| "https://api.v0.dev".to_string())
}

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/payscribe/status", get(payscribe_status))
        .route("/payscribe/data/lookup", get(data_lookup))
        .route("/payscribe/data/vend", post(vend_data))
        .route("/payscribe/requery", get(requery_transaction))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PayscribeStatusResponse {
    configured: bool,
    base_url: String,
    api_key_configured: bool,
    supported_services: Vec<&'static str>,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataLookupParams {
    network: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataVendRequest {
    account_id: Option<Uuid>,
    network: String,
    plan: String,
    recipient: Value,
    ref_id: Option<String>,
    charge_credits: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequeryParams {
    trans_id: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PayscribeTransactionRecord {
    id: Uuid,
    account_id: Option<Uuid>,
    service_type: String,
    provider_reference: String,
    network: Option<String>,
    plan_code: Option<String>,
    recipient: Option<Value>,
    charge_credits: Option<f64>,
    provider_status: String,
    provider_trans_id: Option<String>,
    provider_payload: Value,
    requery_payload: Option<Value>,
}

async fn payscribe_status() -> Json<PayscribeStatusResponse> {
    let base_url = payscribe_base_url();
    let api_key = std::env::var("PAYSCRIBE_API_KEY").unwrap_or_default();
    let configured = !api_key.trim().is_empty();

    Json(PayscribeStatusResponse {
        configured,
        base_url,
        api_key_configured: configured,
        supported_services: vec!["data"],
        message: if configured {
            "Payscribe backend is configured for data vending.".to_string()
        } else {
            "PAYSCRIBE_API_KEY is required before live data vending.".to_string()
        },
    })
}

async fn data_lookup(
    State(state): State<AppState>,
    Query(params): Query<DataLookupParams>,
) -> GatewayResult<Json<Value>> {
    let network = params.network.unwrap_or_default().trim().to_lowercase();
    if !network.is_empty() && !is_supported_data_network(&network) {
        return Err(GatewayError::Upstream("unsupported data network".to_string()));
    }

    let path = if network.is_empty() {
        "/data/lookup".to_string()
    } else {
        format!("/data/lookup?network={network}")
    };
    let response = payscribe_get(&state, &path).await?;
    let status = response.status();
    let body: Value = response.json().await?;

    Ok(Json(json!({
        "accepted": status.is_success(),
        "network": if network.is_empty() { Value::Null } else { Value::String(network) },
        "providerStatus": status.as_u16(),
        "providerResponse": body
    })))
}

async fn vend_data(
    State(state): State<AppState>,
    Json(request): Json<DataVendRequest>,
) -> GatewayResult<Json<PayscribeTransactionRecord>> {
    validate_data_request(&request)?;
    let provider_reference = request.ref_id.clone().unwrap_or_else(|| format!("ps_data_{}", Uuid::new_v4().simple()));

    let payload = json!({
        "network": request.network.trim().to_lowercase(),
        "plan": request.plan.trim(),
        "recipient": request.recipient,
        "ref": provider_reference,
    });

    let response = payscribe_post(&state, "/data/vend", payload.clone()).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    let provider_status = extract_transaction_status(&body, status.as_u16());
    let provider_trans_id = body["message"]["details"]["trans_id"].as_str().map(str::to_string);

    let record = sqlx::query_as::<_, PayscribeTransactionRecord>(
        r#"
        insert into payscribe_transactions (
            account_id, service_type, provider_reference, network, plan_code,
            recipient, charge_credits, provider_status, provider_trans_id, provider_payload
        ) values ($1, 'data', $2, $3, $4, $5, $6, $7, $8, $9)
        on conflict (provider_reference) do update set
            provider_status = excluded.provider_status,
            provider_trans_id = coalesce(excluded.provider_trans_id, payscribe_transactions.provider_trans_id),
            provider_payload = excluded.provider_payload,
            updated_at = now()
        returning id, account_id, service_type, provider_reference, network, plan_code,
                  recipient, charge_credits::float8 as charge_credits, provider_status,
                  provider_trans_id, provider_payload, requery_payload
        "#,
    )
    .bind(request.account_id)
    .bind(&provider_reference)
    .bind(request.network.trim().to_lowercase())
    .bind(request.plan.trim())
    .bind(payload.get("recipient").cloned().unwrap_or(Value::Null))
    .bind(request.charge_credits)
    .bind(provider_status)
    .bind(provider_trans_id)
    .bind(body)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(record))
}

async fn requery_transaction(
    State(state): State<AppState>,
    Query(params): Query<RequeryParams>,
) -> GatewayResult<Json<PayscribeTransactionRecord>> {
    let trans_id = params.trans_id.trim();
    if trans_id.is_empty() {
        return Err(GatewayError::Upstream("transId is required".to_string()));
    }

    let response = payscribe_get(&state, &format!("/requery?trans_id={trans_id}")).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    let provider_status = extract_transaction_status(&body, status.as_u16());

    let record = sqlx::query_as::<_, PayscribeTransactionRecord>(
        r#"
        update payscribe_transactions
        set provider_status = $2,
            requery_payload = $3,
            last_requery_at = now(),
            updated_at = now()
        where provider_reference = $1 or provider_trans_id = $1
        returning id, account_id, service_type, provider_reference, network, plan_code,
                  recipient, charge_credits::float8 as charge_credits, provider_status,
                  provider_trans_id, provider_payload, requery_payload
        "#,
    )
    .bind(trans_id)
    .bind(provider_status)
    .bind(body)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Payscribe transaction not found for requery".to_string()))?;

    Ok(Json(record))
}

fn validate_data_request(request: &DataVendRequest) -> GatewayResult<()> {
    let network = request.network.trim().to_lowercase();
    if !is_supported_data_network(&network) {
        return Err(GatewayError::Upstream("unsupported data network".to_string()));
    }
    if request.plan.trim().is_empty() {
        return Err(GatewayError::Upstream("plan is required".to_string()));
    }
    if request.recipient.is_null() {
        return Err(GatewayError::Upstream("recipient is required".to_string()));
    }
    Ok(())
}

fn is_supported_data_network(network: &str) -> bool {
    matches!(network, "mtn" | "glo" | "airtel" | "9mobile" | "smile" | "dstvshowmax")
}

async fn payscribe_post(state: &AppState, path: &str, payload: Value) -> GatewayResult<reqwest::Response> {
    let token = payscribe_token()?;
    state.http
        .post(format!("{}{}", payscribe_base_url(), path))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .map_err(GatewayError::Http)
}

async fn payscribe_get(state: &AppState, path: &str) -> GatewayResult<reqwest::Response> {
    let token = payscribe_token()?;
    state.http
        .get(format!("{}{}", payscribe_base_url(), path))
        .bearer_auth(token)
        .send()
        .await
        .map_err(GatewayError::Http)
}

fn payscribe_base_url() -> String {
    std::env::var("PAYSCRIBE_BASE_URL").unwrap_or_else(|_| "https://sandbox.payscribe.ng/api/v1".to_string())
}

fn payscribe_token() -> GatewayResult<String> {
    let token = std::env::var("PAYSCRIBE_API_KEY")
        .or_else(|_| std::env::var("PAYSCRIBE_SECRET_KEY"))
        .unwrap_or_default();
    if token.trim().is_empty() {
        return Err(GatewayError::Config("PAYSCRIBE_API_KEY is required".to_string()));
    }
    Ok(token)
}

fn extract_transaction_status(body: &Value, http_status: u16) -> String {
    if let Some(status) = body["message"]["details"]["transaction_status"].as_str() {
        return status.to_string();
    }
    if body.get("status").and_then(Value::as_bool).unwrap_or(false) && http_status == 200 {
        return "processing".to_string();
    }
    format!("http_{http_status}")
}

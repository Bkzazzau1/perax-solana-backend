use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domains::telecom::billing::{credit_credits, debit_credits},
    error::{GatewayError, GatewayResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/payscribe/status", get(payscribe_status))
        .route("/payscribe/data/lookup", get(data_lookup))
        .route("/payscribe/data/quote", get(data_quote))
        .route("/payscribe/data/vend", post(vend_data))
        .route("/payscribe/electricity/status", get(electricity_status))
        .route("/payscribe/electricity/validate", post(validate_electricity_customer))
        .route("/payscribe/electricity/quote", post(electricity_quote))
        .route("/payscribe/electricity/vend", post(vend_electricity))
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ElectricityStatusResponse {
    ready_for_validation: bool,
    ready_for_vending: bool,
    validation_path: String,
    vend_path: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataLookupParams { network: Option<String> }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataQuoteParams { network: String, plan: String }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DataQuoteResponse {
    accepted: bool,
    network: String,
    plan: String,
    plan_amount: f64,
    charge_credits: f64,
    pricing_policy: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataVendRequest {
    account_id: Option<Uuid>,
    network: String,
    plan: String,
    recipient: Value,
    ref_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElectricityValidationRequest {
    disco: Option<String>,
    provider: Option<String>,
    meter_number: Option<String>,
    meter_no: Option<String>,
    meter_type: Option<String>,
    amount: Option<f64>,
    customer_phone: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElectricityQuoteRequest {
    service: String,
    meter_number: String,
    meter_type: String,
    amount: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ElectricityQuoteResponse {
    accepted: bool,
    service: String,
    meter_number: String,
    meter_type: String,
    amount: f64,
    charge_credits: f64,
    validation: Value,
    pricing_policy: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElectricityVendRequest {
    account_id: Uuid,
    meter_number: String,
    meter_type: String,
    amount: f64,
    service: String,
    phone: Option<String>,
    customer_name: String,
    address: Option<String>,
    ref_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequeryParams { trans_id: String }

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
    let configured = !std::env::var("PAYSCRIBE_API_KEY").unwrap_or_default().trim().is_empty();
    Json(PayscribeStatusResponse {
        configured,
        base_url: payscribe_base_url(),
        api_key_configured: configured,
        supported_services: vec!["data", "electricity_validation", "electricity_vend"],
        message: if configured { "Payscribe backend is configured.".to_string() } else { "PAYSCRIBE_API_KEY is required before live Payscribe services.".to_string() },
    })
}

async fn electricity_status() -> Json<ElectricityStatusResponse> {
    Json(ElectricityStatusResponse {
        ready_for_validation: !std::env::var("PAYSCRIBE_API_KEY").unwrap_or_default().trim().is_empty(),
        ready_for_vending: !std::env::var("PAYSCRIBE_API_KEY").unwrap_or_default().trim().is_empty(),
        validation_path: payscribe_electricity_validate_path(),
        vend_path: payscribe_electricity_vend_path(),
        message: "Electricity validation, quote and vending are active. Always validate/quote before vend.".to_string(),
    })
}

async fn data_lookup(State(state): State<AppState>, Query(params): Query<DataLookupParams>) -> GatewayResult<Json<Value>> {
    let network = params.network.unwrap_or_default().trim().to_lowercase();
    if !network.is_empty() && !is_supported_data_network(&network) { return Err(GatewayError::Upstream("unsupported data network".to_string())); }
    let path = if network.is_empty() { "/data/lookup".to_string() } else { format!("/data/lookup?network={network}") };
    let response = payscribe_get(&state, &path).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    Ok(Json(json!({ "accepted": status.is_success(), "network": if network.is_empty() { Value::Null } else { Value::String(network) }, "providerStatus": status.as_u16(), "providerResponse": body })))
}

async fn data_quote(State(state): State<AppState>, Query(params): Query<DataQuoteParams>) -> GatewayResult<Json<DataQuoteResponse>> {
    let network = params.network.trim().to_lowercase();
    let plan = params.plan.trim().to_string();
    if !is_supported_data_network(&network) { return Err(GatewayError::Upstream("unsupported data network".to_string())); }
    if plan.is_empty() { return Err(GatewayError::Upstream("plan is required".to_string())); }
    let lookup = fetch_data_lookup(&state, &network).await?;
    let plan_amount = find_plan_amount(&lookup, &plan).ok_or_else(|| GatewayError::Upstream("selected data plan was not found in Payscribe lookup response".to_string()))?;
    let charge_credits = calculate_data_charge_credits(plan_amount);
    Ok(Json(DataQuoteResponse { accepted: true, network, plan, plan_amount, charge_credits, pricing_policy: json!({ "creditsPerNaira": payscribe_data_credits_per_naira(), "serviceFeeCredits": payscribe_data_service_fee_credits() }) }))
}

async fn validate_electricity_customer(State(state): State<AppState>, Json(request): Json<ElectricityValidationRequest>) -> GatewayResult<Json<Value>> {
    let service = request.disco.clone().or(request.provider.clone()).unwrap_or_default();
    let meter_number = request.meter_number.clone().or(request.meter_no.clone()).unwrap_or_default();
    if service.trim().is_empty() { return Err(GatewayError::Upstream("service/disco/provider is required for electricity validation".to_string())); }
    if meter_number.trim().is_empty() { return Err(GatewayError::Upstream("meterNumber is required for electricity validation".to_string())); }
    let payload = json!({ "meter_number": meter_number, "meter_type": request.meter_type.unwrap_or_else(|| "prepaid".to_string()), "amount": request.amount.unwrap_or(1000.0).to_string(), "service": service });
    let response = payscribe_post(&state, &payscribe_electricity_validate_path(), payload.clone()).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    Ok(Json(json!({ "accepted": status.is_success(), "providerStatus": status.as_u16(), "requestPayload": payload, "providerResponse": body })))
}

async fn electricity_quote(State(state): State<AppState>, Json(request): Json<ElectricityQuoteRequest>) -> GatewayResult<Json<ElectricityQuoteResponse>> {
    validate_electricity_fields(&request.service, &request.meter_number, &request.meter_type, request.amount)?;
    let validation_payload = json!({ "meter_number": request.meter_number, "meter_type": request.meter_type, "amount": request.amount.to_string(), "service": request.service });
    let response = payscribe_post(&state, &payscribe_electricity_validate_path(), validation_payload).await?;
    let status = response.status();
    let validation: Value = response.json().await?;
    if !status.is_success() || !validation.get("status").and_then(Value::as_bool).unwrap_or(false) { return Err(GatewayError::Upstream(format!("Payscribe electricity validation failed: {validation}"))); }
    let charge_credits = calculate_electricity_charge_credits(request.amount);
    Ok(Json(ElectricityQuoteResponse { accepted: true, service: request.service, meter_number: request.meter_number, meter_type: request.meter_type, amount: request.amount, charge_credits, validation, pricing_policy: json!({ "creditsPerNaira": payscribe_electricity_credits_per_naira(), "serviceFeeCredits": payscribe_electricity_service_fee_credits() }) }))
}

async fn vend_data(State(state): State<AppState>, Json(request): Json<DataVendRequest>) -> GatewayResult<Json<PayscribeTransactionRecord>> {
    validate_data_request(&request)?;
    let account_id = request.account_id.ok_or_else(|| GatewayError::Upstream("accountId is required for Payscribe data vending".to_string()))?;
    let network = request.network.trim().to_lowercase();
    let plan = request.plan.trim().to_string();
    let provider_reference = request.ref_id.clone().unwrap_or_else(|| format!("ps_data_{}", Uuid::new_v4().simple()));
    let lookup = fetch_data_lookup(&state, &network).await?;
    let plan_amount = find_plan_amount(&lookup, &plan).ok_or_else(|| GatewayError::Upstream("selected data plan was not found in Payscribe lookup response".to_string()))?;
    let charge_credits = calculate_data_charge_credits(plan_amount);
    debit_credits(&state, account_id, charge_credits, "payscribe_data", &provider_reference, "Payscribe data bundle purchase", json!({ "network": network, "plan": plan, "recipient": request.recipient, "providerReference": provider_reference, "planAmount": plan_amount, "chargeCredits": charge_credits })).await?;
    let payload = json!({ "network": network, "plan": plan, "recipient": request.recipient, "ref": provider_reference });
    let response = payscribe_post(&state, "/data/vend", payload.clone()).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    let provider_status = extract_transaction_status(&body, status.as_u16());
    let provider_trans_id = body["message"]["details"]["trans_id"].as_str().map(str::to_string);
    if !(status.is_success() && body.get("status").and_then(Value::as_bool).unwrap_or(false)) { let _ = credit_credits(&state, account_id, charge_credits, "payscribe_data_reversal", &provider_reference, "Reversal for rejected Payscribe data vend", json!({ "providerResponse": body })).await; }
    Ok(Json(insert_payscribe_transaction(&state, Some(account_id), "data", &provider_reference, Some(&network), Some(&plan), Some(payload.get("recipient").cloned().unwrap_or(Value::Null)), charge_credits, &provider_status, provider_trans_id, body).await?))
}

async fn vend_electricity(State(state): State<AppState>, Json(request): Json<ElectricityVendRequest>) -> GatewayResult<Json<PayscribeTransactionRecord>> {
    validate_electricity_fields(&request.service, &request.meter_number, &request.meter_type, request.amount)?;
    if request.customer_name.trim().is_empty() { return Err(GatewayError::Upstream("customerName is required from electricity validation response".to_string())); }
    let provider_reference = request.ref_id.clone().unwrap_or_else(|| format!("ps_elec_{}", Uuid::new_v4().simple()));
    let charge_credits = calculate_electricity_charge_credits(request.amount);
    debit_credits(&state, request.account_id, charge_credits, "payscribe_electricity", &provider_reference, "Payscribe electricity purchase", json!({ "service": request.service, "meterNumber": request.meter_number, "meterType": request.meter_type, "amount": request.amount, "chargeCredits": charge_credits, "providerReference": provider_reference })).await?;
    let payload = json!({ "meter_number": request.meter_number, "meter_type": request.meter_type, "amount": request.amount, "service": request.service, "phone": request.phone, "customer_name": request.customer_name, "address": request.address, "ref": provider_reference });
    let response = payscribe_post(&state, &payscribe_electricity_vend_path(), payload.clone()).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    let provider_status = extract_transaction_status(&body, status.as_u16());
    let provider_trans_id = body["message"]["details"]["trans_id"].as_str().map(str::to_string);
    if !(status.is_success() && body.get("status").and_then(Value::as_bool).unwrap_or(false)) { let _ = credit_credits(&state, request.account_id, charge_credits, "payscribe_electricity_reversal", &provider_reference, "Reversal for rejected Payscribe electricity vend", json!({ "providerResponse": body })).await; }
    Ok(Json(insert_payscribe_transaction(&state, Some(request.account_id), "electricity", &provider_reference, Some(&request.service), Some(&request.meter_type), Some(payload), charge_credits, &provider_status, provider_trans_id, body).await?))
}

async fn requery_transaction(State(state): State<AppState>, Query(params): Query<RequeryParams>) -> GatewayResult<Json<PayscribeTransactionRecord>> {
    let trans_id = params.trans_id.trim();
    if trans_id.is_empty() { return Err(GatewayError::Upstream("transId is required".to_string())); }
    let response = payscribe_get(&state, &format!("/requery?trans_id={trans_id}")).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    let provider_status = extract_transaction_status(&body, status.as_u16());
    let record = sqlx::query_as::<_, PayscribeTransactionRecord>(r#"update payscribe_transactions set provider_status = $2, requery_payload = $3, last_requery_at = now(), updated_at = now() where provider_reference = $1 or provider_trans_id = $1 returning id, account_id, service_type, provider_reference, network, plan_code, recipient, charge_credits::float8 as charge_credits, provider_status, provider_trans_id, provider_payload, requery_payload"#).bind(trans_id).bind(provider_status).bind(body).fetch_optional(&state.db).await?.ok_or_else(|| GatewayError::Upstream("Payscribe transaction not found for requery".to_string()))?;
    Ok(Json(record))
}

async fn insert_payscribe_transaction(state: &AppState, account_id: Option<Uuid>, service_type: &str, provider_reference: &str, network: Option<&str>, plan_code: Option<&str>, recipient: Option<Value>, charge_credits: f64, provider_status: &str, provider_trans_id: Option<String>, provider_payload: Value) -> GatewayResult<PayscribeTransactionRecord> {
    sqlx::query_as::<_, PayscribeTransactionRecord>(r#"insert into payscribe_transactions (account_id, service_type, provider_reference, network, plan_code, recipient, charge_credits, provider_status, provider_trans_id, provider_payload) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) on conflict (provider_reference) do update set provider_status = excluded.provider_status, provider_trans_id = coalesce(excluded.provider_trans_id, payscribe_transactions.provider_trans_id), provider_payload = excluded.provider_payload, charge_credits = excluded.charge_credits, updated_at = now() returning id, account_id, service_type, provider_reference, network, plan_code, recipient, charge_credits::float8 as charge_credits, provider_status, provider_trans_id, provider_payload, requery_payload"#)
        .bind(account_id).bind(service_type).bind(provider_reference).bind(network).bind(plan_code).bind(recipient).bind(charge_credits).bind(provider_status).bind(provider_trans_id).bind(provider_payload).fetch_one(&state.db).await.map_err(Into::into)
}

fn validate_data_request(request: &DataVendRequest) -> GatewayResult<()> {
    let network = request.network.trim().to_lowercase();
    if !is_supported_data_network(&network) { return Err(GatewayError::Upstream("unsupported data network".to_string())); }
    if request.plan.trim().is_empty() { return Err(GatewayError::Upstream("plan is required".to_string())); }
    if request.recipient.is_null() { return Err(GatewayError::Upstream("recipient is required".to_string())); }
    Ok(())
}

fn validate_electricity_fields(service: &str, meter_number: &str, meter_type: &str, amount: f64) -> GatewayResult<()> {
    if !is_supported_disco(service.trim().to_lowercase().as_str()) { return Err(GatewayError::Upstream("unsupported electricity service/disco".to_string())); }
    if meter_number.trim().is_empty() { return Err(GatewayError::Upstream("meterNumber is required".to_string())); }
    if !matches!(meter_type.trim().to_lowercase().as_str(), "prepaid" | "postpaid") { return Err(GatewayError::Upstream("meterType must be prepaid or postpaid".to_string())); }
    if !amount.is_finite() || amount < 1000.0 { return Err(GatewayError::Upstream("electricity amount must be at least NGN 1,000".to_string())); }
    Ok(())
}

fn is_supported_data_network(network: &str) -> bool { matches!(network, "mtn" | "glo" | "airtel" | "9mobile" | "smile" | "dstvshowmax") }
fn is_supported_disco(service: &str) -> bool { matches!(service, "ikedc" | "ekedc" | "eedc" | "phedc" | "aedc" | "ibedc" | "kedco" | "jed") }

async fn fetch_data_lookup(state: &AppState, network: &str) -> GatewayResult<Value> {
    let response = payscribe_get(state, &format!("/data/lookup?network={network}")).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() { return Err(GatewayError::Upstream(format!("Payscribe data lookup failed: {body}"))); }
    Ok(body)
}

fn payscribe_data_credits_per_naira() -> f64 { env_f64("PAYSCRIBE_DATA_CREDITS_PER_NAIRA", 1.0, true) }
fn payscribe_data_service_fee_credits() -> f64 { env_f64("PAYSCRIBE_DATA_SERVICE_FEE_CREDITS", 0.0, false) }
fn payscribe_electricity_credits_per_naira() -> f64 { env_f64("PAYSCRIBE_ELECTRICITY_CREDITS_PER_NAIRA", 1.0, true) }
fn payscribe_electricity_service_fee_credits() -> f64 { env_f64("PAYSCRIBE_ELECTRICITY_SERVICE_FEE_CREDITS", 0.0, false) }
fn calculate_data_charge_credits(plan_amount: f64) -> f64 { round_2(plan_amount * payscribe_data_credits_per_naira() + payscribe_data_service_fee_credits()) }
fn calculate_electricity_charge_credits(amount: f64) -> f64 { round_2(amount * payscribe_electricity_credits_per_naira() + payscribe_electricity_service_fee_credits()) }
fn env_f64(key: &str, fallback: f64, must_be_positive: bool) -> f64 { std::env::var(key).ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| if must_be_positive { *v > 0.0 } else { *v >= 0.0 }).unwrap_or(fallback) }
fn round_2(value: f64) -> f64 { (value * 100.0).round() / 100.0 }

fn find_plan_amount(value: &Value, plan_code: &str) -> Option<f64> {
    match value { Value::Object(map) => { let has_plan = map.values().any(|item| item.as_str() == Some(plan_code)); if has_plan { for key in ["amount", "price", "fee", "charge"] { if let Some(amount) = map.get(key).and_then(value_to_f64) { return Some(amount); } } } map.values().find_map(|child| find_plan_amount(child, plan_code)) }, Value::Array(items) => items.iter().find_map(|child| find_plan_amount(child, plan_code)), _ => None }
}
fn value_to_f64(value: &Value) -> Option<f64> { value.as_f64().or_else(|| value.as_str()?.parse::<f64>().ok()) }

async fn payscribe_post(state: &AppState, path: &str, payload: Value) -> GatewayResult<reqwest::Response> { let token = payscribe_token()?; state.http.post(format!("{}{}", payscribe_base_url(), path)).bearer_auth(token).json(&payload).send().await.map_err(GatewayError::Http) }
async fn payscribe_get(state: &AppState, path: &str) -> GatewayResult<reqwest::Response> { let token = payscribe_token()?; state.http.get(format!("{}{}", payscribe_base_url(), path)).bearer_auth(token).send().await.map_err(GatewayError::Http) }
fn payscribe_base_url() -> String { std::env::var("PAYSCRIBE_BASE_URL").unwrap_or_else(|_| "https://sandbox.payscribe.ng/api/v1".to_string()) }
fn payscribe_electricity_validate_path() -> String { std::env::var("PAYSCRIBE_ELECTRICITY_VALIDATE_PATH").unwrap_or_else(|_| "/electricity/validate".to_string()) }
fn payscribe_electricity_vend_path() -> String { std::env::var("PAYSCRIBE_ELECTRICITY_VEND_PATH").unwrap_or_else(|_| "/electricity/vend".to_string()) }
fn payscribe_token() -> GatewayResult<String> { let token = std::env::var("PAYSCRIBE_API_KEY").or_else(|_| std::env::var("PAYSCRIBE_SECRET_KEY")).unwrap_or_default(); if token.trim().is_empty() { return Err(GatewayError::Config("PAYSCRIBE_API_KEY is required".to_string())); } Ok(token) }
fn extract_transaction_status(body: &Value, http_status: u16) -> String { if let Some(status) = body["message"]["details"]["transaction_status"].as_str() { return status.to_string(); } if let Some(status) = body["message"]["details"]["status"].as_str() { return status.to_string(); } if body.get("status").and_then(Value::as_bool).unwrap_or(false) && http_status == 200 { return "processing".to_string(); } format!("http_{http_status}") }

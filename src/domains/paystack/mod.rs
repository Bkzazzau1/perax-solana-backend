use axum::{
    Json, Router,
    body::Bytes,
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha512;
use uuid::Uuid;

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

type HmacSha512 = Hmac<Sha512>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/payments/paystack/initialize", post(initialize_transaction))
        .route("/payments/paystack/verify", post(verify_transaction))
        .route("/payments/paystack/webhook", post(receive_webhook))
        .route("/payments/paystack/virtual-account/assign", post(assign_virtual_account))
        .route("/payments/paystack/virtual-account/mine", get(get_virtual_account))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaystackInitializeRequest {
    intent_reference: String,
    email: String,
    callback_url: Option<String>,
    channels: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaystackInitializeResponse {
    accepted: bool,
    intent_reference: String,
    provider_reference: String,
    amount_kobo: i64,
    currency: String,
    authorization_url: Option<String>,
    access_code: Option<String>,
    provider_response: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaystackVerifyRequest {
    intent_reference: Option<String>,
    reference: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaystackAssignVirtualAccountRequest {
    account_id: Uuid,
    email: String,
    first_name: String,
    last_name: String,
    phone: Option<String>,
    preferred_bank: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaystackVirtualAccountQuery {
    account_id: Uuid,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PaymentIntentForPaystack {
    id: Uuid,
    intent_reference: String,
    quote_reference: String,
    funding_method: String,
    provider_reference: Option<String>,
    expected_asset_code: String,
    expected_amount: f64,
    expected_usd_value: f64,
    expected_credits: f64,
    status: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PaystackVirtualAccountRecord {
    id: Uuid,
    account_id: Uuid,
    email: String,
    customer_code: Option<String>,
    account_name: Option<String>,
    account_number: Option<String>,
    bank_name: Option<String>,
    bank_slug: Option<String>,
    currency: String,
    provider_status: String,
    provider_payload: Value,
}

async fn initialize_transaction(
    State(state): State<AppState>,
    Json(request): Json<PaystackInitializeRequest>,
) -> GatewayResult<Json<PaystackInitializeResponse>> {
    let intent = find_intent_by_reference(&state, &request.intent_reference).await?;
    ensure_paystack_supported_intent(&intent)?;

    let provider_reference = intent.provider_reference.clone()
        .unwrap_or_else(|| format!("psk_{}", Uuid::new_v4().simple()));
    let amount_kobo = usd_to_paystack_kobo(&state, intent.expected_amount)?;
    let channels = request.channels.unwrap_or_else(|| {
        if intent.funding_method == "virtual_account" { vec!["bank_transfer".to_string()] } else { vec!["card".to_string()] }
    });

    let payload = json!({
        "email": request.email,
        "amount": amount_kobo,
        "currency": "NGN",
        "reference": provider_reference,
        "channels": channels,
        "callback_url": request.callback_url,
        "metadata": {
            "intentReference": intent.intent_reference,
            "quoteReference": intent.quote_reference,
            "expectedUsdAmount": intent.expected_amount,
            "expectedCredits": intent.expected_credits,
            "fundingMethod": intent.funding_method
        }
    });

    let response = paystack_post(&state, "/transaction/initialize", payload).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    log_paystack_transaction(&state, "initialize_transaction", Some(intent.id), &provider_reference, Some(body.clone()), Some(status.as_u16()), status.is_success(), None).await?;

    if !status.is_success() || !body.get("status").and_then(Value::as_bool).unwrap_or(false) {
        return Err(GatewayError::Upstream(format!("Paystack initialize failed: {body}")));
    }

    sqlx::query("update payment_intents set provider = 'paystack', provider_reference = $2, updated_at = now() where id = $1 and status = 'pending_verification'")
        .bind(intent.id)
        .bind(&provider_reference)
        .execute(&state.db)
        .await?;

    Ok(Json(PaystackInitializeResponse {
        accepted: true,
        intent_reference: intent.intent_reference,
        provider_reference,
        amount_kobo,
        currency: "NGN".to_string(),
        authorization_url: body["data"]["authorization_url"].as_str().map(str::to_string),
        access_code: body["data"]["access_code"].as_str().map(str::to_string),
        provider_response: body,
    }))
}

async fn verify_transaction(
    State(state): State<AppState>,
    Json(request): Json<PaystackVerifyRequest>,
) -> GatewayResult<Json<Value>> {
    verify_paystack_reference(&state, request.intent_reference.as_deref(), &request.reference).await
}

async fn receive_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> GatewayResult<Json<Value>> {
    verify_paystack_webhook(&state, &headers, &body)?;
    let payload: Value = serde_json::from_slice(&body).map_err(|_| GatewayError::Upstream("Paystack webhook JSON is invalid".to_string()))?;
    let event = payload.get("event").and_then(Value::as_str).unwrap_or_default();
    let reference = payload["data"]["reference"].as_str().unwrap_or_default();

    if event != "charge.success" || reference.trim().is_empty() {
        return Ok(Json(json!({ "accepted": true, "processed": false, "event": event })));
    }

    let verified = verify_paystack_reference(&state, None, reference).await?;
    Ok(Json(json!({ "accepted": true, "processed": true, "verification": verified.0 })))
}

async fn assign_virtual_account(
    State(state): State<AppState>,
    Json(request): Json<PaystackAssignVirtualAccountRequest>,
) -> GatewayResult<Json<PaystackVirtualAccountRecord>> {
    let payload = json!({
        "email": request.email,
        "first_name": request.first_name,
        "last_name": request.last_name,
        "phone": request.phone,
        "preferred_bank": request.preferred_bank.unwrap_or_else(|| "wema-bank".to_string()),
        "country": "NG"
    });
    let response = paystack_post(&state, "/dedicated_account/assign", payload).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() || !body.get("status").and_then(Value::as_bool).unwrap_or(false) {
        return Err(GatewayError::Upstream(format!("Paystack virtual account assign failed: {body}")));
    }
    let data = body.get("data").unwrap_or(&Value::Null);
    Ok(Json(upsert_virtual_account(&state, request.account_id, &request.email, data, body.clone()).await?))
}

async fn get_virtual_account(
    State(state): State<AppState>,
    Query(query): Query<PaystackVirtualAccountQuery>,
) -> GatewayResult<Json<PaystackVirtualAccountRecord>> {
    let record = sqlx::query_as::<_, PaystackVirtualAccountRecord>(
        "select id, account_id, email, customer_code, account_name, account_number, bank_name, bank_slug, currency, provider_status, provider_payload from paystack_virtual_accounts where account_id = $1 limit 1",
    )
    .bind(query.account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Paystack virtual account not found".to_string()))?;
    Ok(Json(record))
}

async fn verify_paystack_reference(state: &AppState, intent_reference: Option<&str>, reference: &str) -> GatewayResult<Json<Value>> {
    let response = paystack_get(state, &format!("/transaction/verify/{}", reference.trim())).await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() || !body.get("status").and_then(Value::as_bool).unwrap_or(false) {
        return Err(GatewayError::Upstream(format!("Paystack transaction verify failed: {body}")));
    }
    let data = body.get("data").unwrap_or(&Value::Null);
    if data.get("status").and_then(Value::as_str).unwrap_or_default() != "success" {
        return Err(GatewayError::Upstream("Paystack transaction is not successful".to_string()));
    }
    let provider_reference = data.get("reference").and_then(Value::as_str).unwrap_or(reference).to_string();
    let intent = match intent_reference {
        Some(value) => find_intent_by_reference(state, value).await?,
        None => find_intent_by_provider_reference(state, &provider_reference).await?,
    };
    ensure_paystack_supported_intent(&intent)?;
    validate_paystack_amount(state, &intent, data)?;

    let api_base = std::env::var("PERAX_INTERNAL_API_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let verify_payload = json!({
        "intentReference": intent.intent_reference,
        "provider": "paystack",
        "providerReference": provider_reference,
        "amountPaid": intent.expected_amount,
        "currency": intent.expected_asset_code,
        "status": "successful",
        "rawConfirmation": body
    });
    let response = state.http.post(format!("{api_base}/payments/verify/provider")).json(&verify_payload).send().await?;
    let provider_body: Value = response.json().await?;
    Ok(Json(json!({ "paystackVerified": true, "providerVerification": provider_body })))
}

async fn find_intent_by_reference(state: &AppState, intent_reference: &str) -> GatewayResult<PaymentIntentForPaystack> {
    find_intent(state, "intent_reference = $1", intent_reference.trim()).await
}

async fn find_intent_by_provider_reference(state: &AppState, reference: &str) -> GatewayResult<PaymentIntentForPaystack> {
    find_intent(state, "provider_reference = $1", reference.trim()).await
}

async fn find_intent(state: &AppState, where_clause: &str, value: &str) -> GatewayResult<PaymentIntentForPaystack> {
    let sql = format!("select id, intent_reference, quote_reference, funding_method, provider_reference, expected_asset_code, expected_amount::float8 as expected_amount, expected_usd_value::float8 as expected_usd_value, expected_credits::float8 as expected_credits, status from payment_intents where {where_clause} limit 1");
    sqlx::query_as::<_, PaymentIntentForPaystack>(&sql).bind(value).fetch_optional(&state.db).await?.ok_or_else(|| GatewayError::Upstream("payment intent not found for Paystack flow".to_string()))
}

fn ensure_paystack_supported_intent(intent: &PaymentIntentForPaystack) -> GatewayResult<()> {
    if intent.status != "pending_verification" { return Err(GatewayError::Upstream("payment intent is not pending verification".to_string())); }
    if !matches!(intent.funding_method.as_str(), "card" | "virtual_account") { return Err(GatewayError::Upstream("Paystack only supports card and Nigerian virtual-account deposits".to_string())); }
    Ok(())
}

fn usd_to_paystack_kobo(_state: &AppState, amount_usd: f64) -> GatewayResult<i64> {
    let rate = std::env::var("PAYSTACK_NGN_PER_USD").unwrap_or_else(|_| "1500".to_string()).parse::<f64>().map_err(|_| GatewayError::Config("PAYSTACK_NGN_PER_USD must be numeric".to_string()))?;
    if amount_usd <= 0.0 || rate <= 0.0 { return Err(GatewayError::Config("PAYSTACK_NGN_PER_USD must be positive".to_string())); }
    Ok((amount_usd * rate * 100.0).round() as i64)
}

fn validate_paystack_amount(state: &AppState, intent: &PaymentIntentForPaystack, data: &Value) -> GatewayResult<()> {
    let paid_kobo = data.get("amount").and_then(Value::as_i64).unwrap_or(0);
    let currency = data.get("currency").and_then(Value::as_str).unwrap_or_default();
    if currency != "NGN" { return Err(GatewayError::Upstream("Paystack transaction currency must be NGN".to_string())); }
    if paid_kobo < usd_to_paystack_kobo(state, intent.expected_amount)? { return Err(GatewayError::Upstream("Paystack amount is less than expected quote amount".to_string())); }
    Ok(())
}

fn verify_paystack_webhook(_state: &AppState, headers: &HeaderMap, body: &[u8]) -> GatewayResult<()> {
    let secret = std::env::var("PAYSTACK_SECRET_KEY").unwrap_or_default();
    if secret.trim().is_empty() { return Err(GatewayError::Config("PAYSTACK_SECRET_KEY is required for Paystack webhook verification".to_string())); }
    let signature = headers.get("x-paystack-signature").and_then(|value| value.to_str().ok()).ok_or(GatewayError::Unauthorized)?;
    let mut mac = HmacSha512::new_from_slice(secret.as_bytes()).map_err(|_| GatewayError::Config("invalid PAYSTACK_SECRET_KEY".to_string()))?;
    mac.update(body);
    let expected = bytes_to_hex(&mac.finalize().into_bytes());
    if constant_time_eq(expected.as_bytes(), signature.as_bytes()) { Ok(()) } else { Err(GatewayError::Unauthorized) }
}

async fn paystack_post(state: &AppState, path: &str, payload: Value) -> GatewayResult<reqwest::Response> {
    let secret = std::env::var("PAYSTACK_SECRET_KEY").unwrap_or_default();
    if secret.trim().is_empty() { return Err(GatewayError::Config("PAYSTACK_SECRET_KEY is required".to_string())); }
    let base = std::env::var("PAYSTACK_BASE_URL").unwrap_or_else(|_| "https://api.paystack.co".to_string());
    state.http.post(format!("{base}{path}")).bearer_auth(secret).json(&payload).send().await.map_err(GatewayError::Http)
}

async fn paystack_get(state: &AppState, path: &str) -> GatewayResult<reqwest::Response> {
    let secret = std::env::var("PAYSTACK_SECRET_KEY").unwrap_or_default();
    if secret.trim().is_empty() { return Err(GatewayError::Config("PAYSTACK_SECRET_KEY is required".to_string())); }
    let base = std::env::var("PAYSTACK_BASE_URL").unwrap_or_else(|_| "https://api.paystack.co".to_string());
    state.http.get(format!("{base}{path}")).bearer_auth(secret).send().await.map_err(GatewayError::Http)
}

async fn upsert_virtual_account(state: &AppState, account_id: Uuid, email: &str, data: &Value, provider_payload: Value) -> GatewayResult<PaystackVirtualAccountRecord> {
    let customer_code = data["customer"]["customer_code"].as_str().or_else(|| data["customer_code"].as_str()).map(str::to_string);
    let account_number = data["account_number"].as_str().map(str::to_string);
    let account_name = data["account_name"].as_str().map(str::to_string);
    let bank_name = data["bank"]["name"].as_str().or_else(|| data["bank_name"].as_str()).map(str::to_string);
    let bank_slug = data["bank"]["slug"].as_str().or_else(|| data["bank_slug"].as_str()).map(str::to_string);
    let currency = data["currency"].as_str().unwrap_or("NGN").to_string();
    let provider_status = if data["active"].as_bool().unwrap_or(true) { "active" } else { "inactive" };
    sqlx::query_as::<_, PaystackVirtualAccountRecord>("insert into paystack_virtual_accounts (account_id, email, customer_code, account_name, account_number, bank_name, bank_slug, currency, provider_status, provider_payload) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) on conflict (account_id) do update set email = excluded.email, customer_code = coalesce(excluded.customer_code, paystack_virtual_accounts.customer_code), account_name = coalesce(excluded.account_name, paystack_virtual_accounts.account_name), account_number = coalesce(excluded.account_number, paystack_virtual_accounts.account_number), bank_name = coalesce(excluded.bank_name, paystack_virtual_accounts.bank_name), bank_slug = coalesce(excluded.bank_slug, paystack_virtual_accounts.bank_slug), currency = excluded.currency, provider_status = excluded.provider_status, provider_payload = excluded.provider_payload, updated_at = now() returning id, account_id, email, customer_code, account_name, account_number, bank_name, bank_slug, currency, provider_status, provider_payload")
        .bind(account_id).bind(email).bind(customer_code).bind(account_name).bind(account_number).bind(bank_name).bind(bank_slug).bind(currency).bind(provider_status).bind(provider_payload).fetch_one(&state.db).await.map_err(Into::into)
}

async fn log_paystack_transaction(state: &AppState, action: &str, payment_intent_id: Option<Uuid>, reference: &str, response_payload: Option<Value>, http_status: Option<u16>, success: bool, error_message: Option<&str>) -> GatewayResult<()> {
    sqlx::query("insert into provider_transactions (provider, provider_action, source, source_reference, payment_intent_id, response_payload, http_status, success, error_message) values ('paystack', $1, 'paystack', $2, $3, $4, $5, $6, $7) on conflict (source, source_reference) where source is not null and source_reference is not null do update set response_payload = excluded.response_payload, http_status = excluded.http_status, success = excluded.success, error_message = excluded.error_message")
        .bind(action).bind(reference).bind(payment_intent_id).bind(response_payload).bind(http_status.map(i32::from)).bind(success).bind(error_message).execute(&state.db).await?;
    Ok(())
}

fn bytes_to_hex(bytes: &[u8]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool { if a.len() != b.len() { return false; } a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0 }

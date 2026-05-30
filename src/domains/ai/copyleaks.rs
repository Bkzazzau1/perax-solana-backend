use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
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
        .route("/ai/copyleaks/status", get(copyleaks_status))
        .route("/ai/copyleaks/quote", post(copyleaks_quote))
        .route("/ai/copyleaks/submit", post(copyleaks_submit))
        .route("/ai/copyleaks/webhook", post(copyleaks_webhook))
        .route("/ai/copyleaks/result/{reference}", get(copyleaks_result))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CopyleaksStatusResponse {
    configured: bool,
    base_url: String,
    email_configured: bool,
    api_key_configured: bool,
    webhook_secret_configured: bool,
    mode: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyleaksQuoteRequest {
    scan_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CopyleaksQuoteResponse {
    accepted: bool,
    service_code: String,
    scan_type: String,
    credit_cost: f64,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyleaksSubmitRequest {
    account_id: Uuid,
    title: Option<String>,
    text: String,
    scan_type: Option<String>,
    ref_id: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct CopyleaksScanRecord {
    id: Uuid,
    account_id: Uuid,
    scan_reference: String,
    copyleaks_scan_id: Option<String>,
    scan_type: String,
    status: String,
    credit_cost: f64,
    title: Option<String>,
    submit_response: Value,
    result_payload: Option<Value>,
    error_message: Option<String>,
}

async fn copyleaks_status() -> Json<CopyleaksStatusResponse> {
    let email = std::env::var("COPYLEAKS_EMAIL").unwrap_or_default();
    let api_key = std::env::var("COPYLEAKS_API_KEY").unwrap_or_default();
    let webhook_secret = std::env::var("COPYLEAKS_WEBHOOK_SECRET").unwrap_or_default();
    let configured = !email.trim().is_empty() && !api_key.trim().is_empty();

    Json(CopyleaksStatusResponse {
        configured,
        base_url: copyleaks_base_url(),
        email_configured: !email.trim().is_empty(),
        api_key_configured: !api_key.trim().is_empty(),
        webhook_secret_configured: !webhook_secret.trim().is_empty(),
        mode: if configured {
            "ready"
        } else {
            "configuration_required"
        }
        .to_string(),
        message: if configured {
            "Copyleaks premium scan foundation is configured.".to_string()
        } else {
            "COPYLEAKS_EMAIL and COPYLEAKS_API_KEY are required before live Copyleaks scans."
                .to_string()
        },
    })
}

async fn copyleaks_quote(
    State(state): State<AppState>,
    Json(payload): Json<CopyleaksQuoteRequest>,
) -> GatewayResult<Json<CopyleaksQuoteResponse>> {
    let credit_cost = pricing::get_utility_price(&state, "copyleaks_premium_scan")
        .await?
        .credit_cost;
    let scan_type = normalize_scan_type(payload.scan_type.as_deref());

    Ok(Json(CopyleaksQuoteResponse {
        accepted: true,
        service_code: "copyleaks_premium_scan".to_string(),
        scan_type,
        credit_cost,
        message: "Copyleaks premium scan quote generated. No Credits debited yet.".to_string(),
    }))
}

async fn copyleaks_submit(
    State(state): State<AppState>,
    Json(payload): Json<CopyleaksSubmitRequest>,
) -> GatewayResult<Json<CopyleaksScanRecord>> {
    if payload.text.trim().is_empty() {
        return Err(GatewayError::Upstream(
            "text is required for Copyleaks scan".to_string(),
        ));
    }
    if payload.text.chars().count() < 50 {
        return Err(GatewayError::Upstream(
            "text must be at least 50 characters for a premium scan".to_string(),
        ));
    }

    let scan_type = normalize_scan_type(payload.scan_type.as_deref());
    let credit_cost = pricing::get_utility_price(&state, "copyleaks_premium_scan")
        .await?
        .credit_cost;
    let reference = payload
        .ref_id
        .clone()
        .unwrap_or_else(|| format!("copyleaks_{}", Uuid::new_v4().simple()));
    if let Some(existing) = find_copyleaks_record(&state, &reference).await? {
        return Ok(Json(existing));
    }

    debit_credits(
        &state,
        payload.account_id,
        credit_cost,
        "copyleaks_premium_scan",
        &reference,
        "Copyleaks premium plagiarism scan",
        json!({
            "scanType": scan_type,
            "title": payload.title,
            "textLength": payload.text.chars().count()
        }),
    )
    .await?;

    let scan_id = format!("perax-{reference}");
    let submit_payload = build_submit_payload(&payload, &reference);
    let submit_response = match submit_to_copyleaks(&state, &scan_id, submit_payload.clone()).await
    {
        Ok(value) => value,
        Err(err) => {
            let error = err.to_string();
            credit_credits(
                &state,
                payload.account_id,
                credit_cost,
                "copyleaks_premium_scan_reversal",
                &format!("{reference}:provider_rejected"),
                "Reversal for rejected Copyleaks scan",
                json!({ "scanReference": reference, "scanId": scan_id, "error": error }),
            )
            .await?;
            log_named_provider_transaction(
                &state,
                "copyleaks",
                "submit_scan",
                Some(payload.account_id),
                "copyleaks_premium_scan",
                &reference,
                Some(submit_payload.clone()),
                None,
                None,
                false,
                Some(&error),
            )
            .await?;
            return Err(err);
        }
    };
    let submitted = submit_response
        .get("submitted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let http_status = submit_response
        .get("httpStatus")
        .and_then(Value::as_u64)
        .map(|value| value as u16);
    if !submitted {
        credit_credits(
            &state,
            payload.account_id,
            credit_cost,
            "copyleaks_premium_scan_reversal",
            &format!("{reference}:provider_rejected"),
            "Reversal for rejected Copyleaks scan",
            json!({ "scanReference": reference, "scanId": scan_id, "providerResponse": submit_response }),
        )
        .await?;
        log_named_provider_transaction(
            &state,
            "copyleaks",
            "submit_scan",
            Some(payload.account_id),
            "copyleaks_premium_scan",
            &reference,
            Some(submit_payload.clone()),
            Some(submit_response.clone()),
            http_status,
            false,
            Some("Copyleaks submission rejected"),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Copyleaks submission rejected: {submit_response}"
        )));
    }
    log_named_provider_transaction(
        &state,
        "copyleaks",
        "submit_scan",
        Some(payload.account_id),
        "copyleaks_premium_scan",
        &reference,
        Some(submit_payload.clone()),
        Some(submit_response.clone()),
        http_status,
        true,
        None,
    )
    .await?;

    let status = "submitted";

    let record = sqlx::query_as::<_, CopyleaksScanRecord>(
        r#"
        insert into copyleaks_scans (
            account_id, scan_reference, copyleaks_scan_id, scan_type, status,
            credit_cost, title, submitted_text, submit_payload, submit_response
        ) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        on conflict (scan_reference) do update set
            submit_response = excluded.submit_response,
            status = excluded.status,
            updated_at = now()
        returning id, account_id, scan_reference, copyleaks_scan_id, scan_type, status,
                  credit_cost::float8 as credit_cost, title, submit_response, result_payload, error_message
        "#,
    )
    .bind(payload.account_id)
    .bind(&reference)
    .bind(&scan_id)
    .bind(scan_type)
    .bind(status)
    .bind(credit_cost)
    .bind(payload.title)
    .bind(payload.text)
    .bind(submit_payload)
    .bind(submit_response)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(record))
}

async fn find_copyleaks_record(
    state: &AppState,
    reference: &str,
) -> GatewayResult<Option<CopyleaksScanRecord>> {
    sqlx::query_as::<_, CopyleaksScanRecord>(
        r#"
        select id, account_id, scan_reference, copyleaks_scan_id, scan_type, status,
               credit_cost::float8 as credit_cost, title, submit_response, result_payload, error_message
        from copyleaks_scans
        where scan_reference = $1
        limit 1
        "#,
    )
    .bind(reference)
    .fetch_optional(&state.db)
    .await
    .map_err(Into::into)
}

async fn copyleaks_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> GatewayResult<Json<Value>> {
    verify_webhook(&headers)?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| GatewayError::Upstream("invalid Copyleaks webhook JSON".to_string()))?;
    let scan_id = payload
        .get("scannedDocument")
        .and_then(|v| v.get("scanId"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("scanId").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();

    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let updated = sqlx::query(
        r#"
        update copyleaks_scans
        set status = $2,
            webhook_payload = $3,
            result_payload = coalesce(result_payload, $3),
            completed_at = case when $2 in ('completed','success','failed','error') then now() else completed_at end,
            updated_at = now()
        where copyleaks_scan_id = $1 or scan_reference = $1
        "#,
    )
    .bind(&scan_id)
    .bind(status)
    .bind(payload.clone())
    .execute(&state.db)
    .await?;

    Ok(Json(
        json!({ "accepted": true, "matchedRows": updated.rows_affected(), "scanId": scan_id }),
    ))
}

async fn copyleaks_result(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> GatewayResult<Json<CopyleaksScanRecord>> {
    let record = sqlx::query_as::<_, CopyleaksScanRecord>(
        r#"
        select id, account_id, scan_reference, copyleaks_scan_id, scan_type, status,
               credit_cost::float8 as credit_cost, title, submit_response, result_payload, error_message
        from copyleaks_scans
        where scan_reference = $1 or copyleaks_scan_id = $1
        limit 1
        "#,
    )
    .bind(reference)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Copyleaks scan not found".to_string()))?;

    Ok(Json(record))
}

fn build_submit_payload(payload: &CopyleaksSubmitRequest, reference: &str) -> Value {
    json!({
        "base64": base64_text(&payload.text),
        "filename": payload.title.clone().unwrap_or_else(|| format!("{reference}.txt")),
        "properties": {
            "webhooks": {
                "status": std::env::var("COPYLEAKS_WEBHOOK_URL").unwrap_or_default()
            },
            "includeHtml": false,
            "developerPayload": reference
        }
    })
}

async fn submit_to_copyleaks(
    state: &AppState,
    scan_id: &str,
    payload: Value,
) -> GatewayResult<Value> {
    let token = copyleaks_token(state).await?;
    let url = format!("{}/v3/scans/submit/file/{}", copyleaks_base_url(), scan_id);
    let response = state
        .http
        .put(url)
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Ok(json!({ "submitted": status.is_success(), "httpStatus": status.as_u16(), "body": body }))
}

async fn copyleaks_token(state: &AppState) -> GatewayResult<String> {
    let email = std::env::var("COPYLEAKS_EMAIL").unwrap_or_default();
    let key = std::env::var("COPYLEAKS_API_KEY").unwrap_or_default();
    if email.trim().is_empty() || key.trim().is_empty() {
        return Err(GatewayError::Config(
            "COPYLEAKS_EMAIL and COPYLEAKS_API_KEY are required".to_string(),
        ));
    }
    let response = state
        .http
        .post(format!("{}/v3/account/login/api", copyleaks_base_url()))
        .json(&json!({ "email": email, "key": key }))
        .send()
        .await?;
    let body: Value = response.json().await?;
    body.get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            body.get("accessToken")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| {
            GatewayError::Upstream(format!("Copyleaks token missing from response: {body}"))
        })
}

fn normalize_scan_type(value: Option<&str>) -> String {
    match value.unwrap_or("plagiarism").trim().to_lowercase().as_str() {
        "historical_alignment" | "alignment" => "historical_alignment".to_string(),
        "ai_detection" | "ai" => "ai_detection".to_string(),
        _ => "plagiarism".to_string(),
    }
}

fn verify_webhook(headers: &HeaderMap) -> GatewayResult<()> {
    let configured = std::env::var("COPYLEAKS_WEBHOOK_SECRET").unwrap_or_default();
    if configured.trim().is_empty() {
        return Ok(());
    }
    let provided = headers
        .get("x-perax-webhook-secret")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if provided == configured {
        Ok(())
    } else {
        Err(GatewayError::Unauthorized)
    }
}

fn copyleaks_base_url() -> String {
    std::env::var("COPYLEAKS_BASE_URL").unwrap_or_else(|_| "https://api.copyleaks.com".to_string())
}

fn base64_text(text: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(text.as_bytes())
}

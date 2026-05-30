use axum::{Json, extract::State};
use chrono::{DateTime, Duration, Utc};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        telecom::billing::{round_credits, telnyx_economics_with_cost_source},
    },
    error::{GatewayError, GatewayResult},
    providers::TelnyxClient,
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReportSyncRequest {
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReportSyncResponse {
    pub report_type: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub records_processed: usize,
    pub records_matched: usize,
    pub sync_id: Uuid,
}

pub async fn sync_cdr_usage_report(
    State(state): State<AppState>,
    _account: AuthenticatedAccount,
    Json(request): Json<UsageReportSyncRequest>,
) -> GatewayResult<Json<UsageReportSyncResponse>> {
    let (start_time, end_time) = report_window(request)?;
    Ok(Json(
        sync_cdr_usage_report_once(&state, start_time, end_time).await?,
    ))
}

pub async fn sync_mdr_usage_report(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(request): Json<UsageReportSyncRequest>,
) -> GatewayResult<Json<UsageReportSyncResponse>> {
    let (start_time, end_time) = report_window(request)?;
    Ok(Json(
        sync_mdr_usage_report_once(&state, Some(account.account_id), start_time, end_time).await?,
    ))
}

pub fn spawn_telnyx_usage_report_worker(state: AppState) {
    if !env_bool("TELNYX_USAGE_REPORT_SYNC_ENABLED", false) {
        return;
    }

    tokio::spawn(async move {
        let interval_seconds = env_u64("TELNYX_USAGE_REPORT_SYNC_INTERVAL_SECONDS", 3600).max(300);
        let window_hours = env_i64("TELNYX_USAGE_REPORT_SYNC_WINDOW_HOURS", 24).max(1);

        loop {
            let end_time = Utc::now();
            let start_time = end_time - Duration::hours(window_hours);

            if let Err(err) = sync_cdr_usage_report_once(&state, start_time, end_time).await {
                tracing::error!(error = %err, "Telnyx CDR usage report worker failed");
            }
            if let Err(err) = sync_mdr_usage_report_once(&state, None, start_time, end_time).await {
                tracing::error!(error = %err, "Telnyx MDR usage report worker failed");
            }

            tokio::time::sleep(std::time::Duration::from_secs(interval_seconds)).await;
        }
    });
}

async fn sync_cdr_usage_report_once(
    state: &AppState,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> GatewayResult<UsageReportSyncResponse> {
    let response = TelnyxClient::new(state)
        .sync_cdr_usage_report(&start_time.to_rfc3339(), &end_time.to_rfc3339())
        .await?;
    let status = response.status();
    let payload = response_json_or_status(response).await?;

    if !status.is_success() {
        let sync_id = save_report_sync(
            &state,
            "cdr",
            start_time,
            end_time,
            0,
            0,
            Some(payload.clone()),
            "failed",
            Some(format!("Telnyx CDR usage report sync failed: {payload}")),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx CDR usage report sync failed ({sync_id}): {payload}"
        )));
    }

    let records = report_records(&payload);
    let mut matched = 0usize;
    for record in &records {
        if reconcile_cdr_record(&state, record).await? {
            matched += 1;
        }
    }

    let sync_id = save_report_sync(
        &state,
        "cdr",
        start_time,
        end_time,
        records.len(),
        matched,
        Some(payload),
        "completed",
        None,
    )
    .await?;

    Ok(UsageReportSyncResponse {
        report_type: "cdr".to_string(),
        start_time,
        end_time,
        records_processed: records.len(),
        records_matched: matched,
        sync_id,
    })
}

async fn sync_mdr_usage_report_once(
    state: &AppState,
    account_id: Option<Uuid>,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> GatewayResult<UsageReportSyncResponse> {
    let response = TelnyxClient::new(state)
        .sync_mdr_usage_report(&start_time.to_rfc3339(), &end_time.to_rfc3339())
        .await?;
    let status = response.status();
    let payload = response_json_or_status(response).await?;

    if !status.is_success() {
        let sync_id = save_report_sync(
            &state,
            "mdr",
            start_time,
            end_time,
            0,
            0,
            Some(payload.clone()),
            "failed",
            Some(format!("Telnyx MDR usage report sync failed: {payload}")),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx MDR usage report sync failed ({sync_id}): {payload}"
        )));
    }

    let records = report_records(&payload);
    let records_processed = records.len();
    let sync_id = save_report_sync(
        &state,
        "mdr",
        start_time,
        end_time,
        records_processed,
        0,
        Some(payload.clone()),
        "completed",
        None,
    )
    .await?;

    sqlx::query(
        r#"
        insert into provider_transactions (
            provider,
            provider_action,
            account_id,
            source,
            source_reference,
            request_payload,
            response_payload,
            http_status,
            success,
            provider_cost_source
        ) values ('telnyx', 'sync_mdr_usage_report', $1, 'telnyx_mdr_usage_report', $2, $3, $4, $5, true, 'mdr_usage_report')
        "#,
    )
    .bind(account_id)
    .bind(sync_id.to_string())
    .bind(serde_json::json!({
        "startTime": start_time,
        "endTime": end_time
    }))
    .bind(payload)
    .bind(status.as_u16() as i32)
    .execute(&state.db)
    .await?;

    Ok(UsageReportSyncResponse {
        report_type: "mdr".to_string(),
        start_time,
        end_time,
        records_processed,
        records_matched: 0,
        sync_id,
    })
}

fn report_window(request: UsageReportSyncRequest) -> GatewayResult<(DateTime<Utc>, DateTime<Utc>)> {
    let end_time = request.end_time.unwrap_or_else(Utc::now);
    let start_time = request
        .start_time
        .unwrap_or_else(|| end_time - Duration::hours(24));

    if start_time >= end_time {
        return Err(GatewayError::Upstream(
            "startTime must be before endTime".to_string(),
        ));
    }

    Ok((start_time, end_time))
}

async fn reconcile_cdr_record(state: &AppState, record: &Value) -> GatewayResult<bool> {
    let Some(identifier) = cdr_identifier(record) else {
        return Ok(false);
    };
    let Some(cost_amount) = cost_amount(record) else {
        return Ok(false);
    };
    let cost_currency = cost_currency(record).unwrap_or_else(|| "USD".to_string());
    let credits_charged = existing_voice_credits(state, &identifier)
        .await?
        .unwrap_or(0.0);
    let economics = telnyx_economics_with_cost_source(
        credits_charged,
        cost_amount,
        &cost_currency,
        "cdr_usage_report",
    );

    let result = sqlx::query(
        r#"
        update telnyx_voice_calls
        set estimated_usd_cost = $1,
            provider_cost_currency = $2,
            provider_cost_source = $3,
            margin_credits = $4,
            margin_usd = $5,
            cdr_report_payload = $6,
            cdr_synced_at = now(),
            updated_at = now()
        where call_id = $7
           or call_control_id = $7
           or call_leg_id = $7
           or call_session_id = $7
        "#,
    )
    .bind(economics.estimated_usd_cost)
    .bind(&economics.provider_cost_currency)
    .bind(&economics.provider_cost_source)
    .bind(economics.margin_credits)
    .bind(economics.margin_usd)
    .bind(record)
    .bind(identifier)
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected() > 0)
}

async fn existing_voice_credits(state: &AppState, identifier: &str) -> GatewayResult<Option<f64>> {
    sqlx::query_scalar::<_, Option<f64>>(
        r#"
        select credits_charged::double precision
        from telnyx_voice_calls
        where call_id = $1
           or call_control_id = $1
           or call_leg_id = $1
           or call_session_id = $1
        limit 1
        "#,
    )
    .bind(identifier)
    .fetch_optional(&state.db)
    .await
    .map(|value| value.flatten())
    .map_err(Into::into)
}

fn cdr_identifier(record: &Value) -> Option<String> {
    first_string(
        record,
        &[
            "call_control_id",
            "call_leg_id",
            "call_session_id",
            "call_id",
            "id",
        ],
    )
}

fn report_records(payload: &Value) -> Vec<Value> {
    payload
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            payload
                .get("data")
                .and_then(|data| data.get("records"))
                .and_then(Value::as_array)
                .cloned()
        })
        .or_else(|| payload.get("records").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn cost_amount(record: &Value) -> Option<f64> {
    record
        .get("cost")
        .and_then(|cost| cost.get("amount").or(Some(cost)))
        .and_then(number_value)
        .or_else(|| first_number(record, &["cost_amount", "total_cost", "amount", "price"]))
        .map(round_credits)
}

fn cost_currency(record: &Value) -> Option<String> {
    record
        .get("cost")
        .and_then(|cost| cost.get("currency"))
        .and_then(Value::as_str)
        .or_else(|| {
            record
                .get("currency")
                .and_then(Value::as_str)
                .or_else(|| record.get("cost_currency").and_then(Value::as_str))
        })
        .map(|value| value.to_string())
}

fn first_string(record: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| record.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn first_number(record: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| record.get(*key).and_then(number_value))
}

fn number_value(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
}

fn env_bool(key: &str, fallback: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| matches!(value.trim().to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(fallback)
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn env_i64(key: &str, fallback: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(fallback)
}

async fn save_report_sync(
    state: &AppState,
    report_type: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    records_processed: usize,
    records_matched: usize,
    provider_response: Option<Value>,
    status: &str,
    error_message: Option<String>,
) -> GatewayResult<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into telnyx_usage_report_syncs (
            report_type,
            start_time,
            end_time,
            records_processed,
            records_matched,
            provider_response,
            status,
            error_message
        ) values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning id
        "#,
    )
    .bind(report_type)
    .bind(start_time)
    .bind(end_time)
    .bind(records_processed as i32)
    .bind(records_matched as i32)
    .bind(provider_response)
    .bind(status)
    .bind(error_message)
    .fetch_one(&state.db)
    .await
    .map_err(Into::into)
}

async fn response_json_or_status(response: Response) -> GatewayResult<Value> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(serde_json::json!({ "status": status.as_u16() }));
    }

    Ok(serde_json::from_str(&text).unwrap_or_else(|_| {
        serde_json::json!({
            "status": status.as_u16(),
            "body": text
        })
    }))
}

use axum::{Json, Router, extract::{Query, State}, routing::get};
use chrono::{NaiveDate, Utc, Datelike};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::GatewayResult, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/api/pex/revenue-events", get(list_pex_revenue_events))
        .route("/admin/api/pex/monthly-sell-cap", get(list_monthly_sell_cap))
}

#[derive(Debug, Deserialize)]
struct RevenueEventQuery {
    limit: Option<i64>,
    revenue_month: Option<NaiveDate>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct PexRevenueEventRecord {
    id: Uuid,
    reference_hex: String,
    payer_wallet: Option<String>,
    token_mint: Option<String>,
    trading_company_settlement_account: String,
    trading_company_second_wallet: String,
    pex_received: f64,
    credits_granted: f64,
    immediate_burn_percentage: f64,
    pex_burn_amount: f64,
    pex_remaining_amount: f64,
    burn_status: String,
    burn_tx_signature: Option<String>,
    revenue_month: NaiveDate,
    service_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PexRevenueEventsResponse {
    count: usize,
    events: Vec<PexRevenueEventRecord>,
}

async fn list_pex_revenue_events(
    State(state): State<AppState>,
    Query(query): Query<RevenueEventQuery>,
) -> GatewayResult<Json<PexRevenueEventsResponse>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let revenue_month = query.revenue_month.unwrap_or_else(current_revenue_month);

    let events = sqlx::query_as::<_, PexRevenueEventRecord>(
        r#"
        select
            id,
            reference_hex,
            payer_wallet,
            token_mint,
            trading_company_settlement_account,
            trading_company_second_wallet,
            pex_received::float8 as pex_received,
            credits_granted::float8 as credits_granted,
            immediate_burn_percentage::float8 as immediate_burn_percentage,
            pex_burn_amount::float8 as pex_burn_amount,
            pex_remaining_amount::float8 as pex_remaining_amount,
            burn_status,
            burn_tx_signature,
            revenue_month,
            service_code
        from pex_revenue_events
        where revenue_month = $1
        order by credited_at desc
        limit $2
        "#,
    )
    .bind(revenue_month)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(PexRevenueEventsResponse {
        count: events.len(),
        events,
    }))
}

#[derive(Debug, Deserialize)]
struct SellCapQuery {
    limit: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct MonthlySellCapRecord {
    id: Uuid,
    revenue_month: NaiveDate,
    trading_company_second_wallet: String,
    monthly_revenue_pex: f64,
    monthly_burned_pex: f64,
    monthly_remaining_pex: f64,
    sell_cap_percentage: f64,
    monthly_sell_cap_pex: f64,
    monthly_sold_pex: f64,
    monthly_sell_allowance_remaining_pex: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonthlySellCapResponse {
    count: usize,
    ledgers: Vec<MonthlySellCapRecord>,
}

async fn list_monthly_sell_cap(
    State(state): State<AppState>,
    Query(query): Query<SellCapQuery>,
) -> GatewayResult<Json<MonthlySellCapResponse>> {
    let limit = query.limit.unwrap_or(24).clamp(1, 60);

    let ledgers = sqlx::query_as::<_, MonthlySellCapRecord>(
        r#"
        select
            id,
            revenue_month,
            trading_company_second_wallet,
            monthly_revenue_pex::float8 as monthly_revenue_pex,
            monthly_burned_pex::float8 as monthly_burned_pex,
            monthly_remaining_pex::float8 as monthly_remaining_pex,
            sell_cap_percentage::float8 as sell_cap_percentage,
            monthly_sell_cap_pex::float8 as monthly_sell_cap_pex,
            monthly_sold_pex::float8 as monthly_sold_pex,
            monthly_sell_allowance_remaining_pex::float8 as monthly_sell_allowance_remaining_pex
        from pex_monthly_sell_cap_ledger
        order by revenue_month desc
        limit $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(MonthlySellCapResponse {
        count: ledgers.len(),
        ledgers,
    }))
}

fn current_revenue_month() -> NaiveDate {
    let today = Utc::now().date_naive();
    NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid month start")
}

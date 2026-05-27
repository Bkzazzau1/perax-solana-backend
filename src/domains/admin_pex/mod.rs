use axum::{Json, Router, extract::{Query, State}, routing::{get, post}};
use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/api/pex/revenue-events", get(list_pex_revenue_events))
        .route("/admin/api/pex/monthly-sell-cap", get(list_monthly_sell_cap))
        .route("/admin/api/pex/sell-events", get(list_revenue_token_account_sell_events))
        .route("/admin/api/pex/sell-events/declare", post(declare_revenue_token_account_sale))
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
    #[serde(rename = "tradingCompanyRevenueTokenAccount")]
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
    #[serde(rename = "tradingCompanyRevenueTokenAccount")]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeclareSellEventRequest {
    pex_sell_amount: f64,
    sell_reason: Option<String>,
    revenue_month: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclareSellEventResponse {
    accepted: bool,
    event_id: Option<Uuid>,
    revenue_month: NaiveDate,
    requested_sell_amount: f64,
    monthly_sell_cap_pex: f64,
    monthly_sold_pex: f64,
    monthly_sell_allowance_remaining_pex: f64,
    message: String,
}

async fn declare_revenue_token_account_sale(
    State(state): State<AppState>,
    Json(request): Json<DeclareSellEventRequest>,
) -> GatewayResult<Json<DeclareSellEventResponse>> {
    let revenue_month = request.revenue_month.unwrap_or_else(current_revenue_month);
    let pex_sell_amount = round_token_amount(request.pex_sell_amount.max(0.0));

    if pex_sell_amount <= 0.0 {
        return Err(GatewayError::Upstream(
            "pexSellAmount must be greater than 0".to_string(),
        ));
    }

    let mut tx = state.db.begin().await?;

    let ledger = sqlx::query_as::<_, MonthlySellCapRecord>(
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
        where revenue_month = $1
        for update
        "#,
    )
    .bind(revenue_month)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(ledger) = ledger else {
        return Ok(Json(DeclareSellEventResponse {
            accepted: false,
            event_id: None,
            revenue_month,
            requested_sell_amount: pex_sell_amount,
            monthly_sell_cap_pex: 0.0,
            monthly_sold_pex: 0.0,
            monthly_sell_allowance_remaining_pex: 0.0,
            message: "No PEX revenue ledger exists for this month. Sale declaration rejected.".to_string(),
        }));
    };

    if pex_sell_amount > ledger.monthly_sell_allowance_remaining_pex {
        return Ok(Json(DeclareSellEventResponse {
            accepted: false,
            event_id: None,
            revenue_month,
            requested_sell_amount: pex_sell_amount,
            monthly_sell_cap_pex: ledger.monthly_sell_cap_pex,
            monthly_sold_pex: ledger.monthly_sold_pex,
            monthly_sell_allowance_remaining_pex: ledger.monthly_sell_allowance_remaining_pex,
            message: "Sale declaration rejected because it exceeds the 50% monthly PEX sell cap.".to_string(),
        }));
    }

    let event_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into pex_second_wallet_sell_events (
            revenue_month,
            trading_company_second_wallet,
            pex_sell_amount,
            sell_reason,
            approval_status
        ) values ($1, $2, $3, $4, 'declared')
        returning id
        "#,
    )
    .bind(revenue_month)
    .bind(&state.config.trading_company_second_wallet)
    .bind(pex_sell_amount)
    .bind(request.sell_reason.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        update pex_monthly_sell_cap_ledger
        set
            monthly_sold_pex = monthly_sold_pex + $1,
            monthly_sell_allowance_remaining_pex = monthly_sell_cap_pex - (monthly_sold_pex + $1),
            updated_at = now()
        where revenue_month = $2
        "#,
    )
    .bind(pex_sell_amount)
    .bind(revenue_month)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(DeclareSellEventResponse {
        accepted: true,
        event_id: Some(event_id),
        revenue_month,
        requested_sell_amount: pex_sell_amount,
        monthly_sell_cap_pex: ledger.monthly_sell_cap_pex,
        monthly_sold_pex: ledger.monthly_sold_pex + pex_sell_amount,
        monthly_sell_allowance_remaining_pex: ledger.monthly_sell_allowance_remaining_pex - pex_sell_amount,
        message: "Sale declared within the 50% monthly PEX sell cap. It still requires approval before execution.".to_string(),
    }))
}

#[derive(Debug, Deserialize)]
struct SellEventsQuery {
    limit: Option<i64>,
    revenue_month: Option<NaiveDate>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct RevenueTokenAccountSellEventRecord {
    id: Uuid,
    revenue_month: NaiveDate,
    #[serde(rename = "tradingCompanyRevenueTokenAccount")]
    trading_company_second_wallet: String,
    pex_sell_amount: f64,
    sell_reason: Option<String>,
    approval_status: String,
    tx_signature: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevenueTokenAccountSellEventsResponse {
    count: usize,
    events: Vec<RevenueTokenAccountSellEventRecord>,
}

async fn list_revenue_token_account_sell_events(
    State(state): State<AppState>,
    Query(query): Query<SellEventsQuery>,
) -> GatewayResult<Json<RevenueTokenAccountSellEventsResponse>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let revenue_month = query.revenue_month.unwrap_or_else(current_revenue_month);

    let events = sqlx::query_as::<_, RevenueTokenAccountSellEventRecord>(
        r#"
        select
            id,
            revenue_month,
            trading_company_second_wallet,
            pex_sell_amount::float8 as pex_sell_amount,
            sell_reason,
            approval_status,
            tx_signature
        from pex_second_wallet_sell_events
        where revenue_month = $1
        order by declared_at desc
        limit $2
        "#,
    )
    .bind(revenue_month)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(RevenueTokenAccountSellEventsResponse {
        count: events.len(),
        events,
    }))
}

fn current_revenue_month() -> NaiveDate {
    let today = Utc::now().date_naive();
    NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid month start")
}

fn round_token_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

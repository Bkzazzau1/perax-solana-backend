use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domains::solana::policy::{MarketPolicyInput, calculate_daily_burn_decision},
    error::GatewayResult,
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct RecordPexRevenueInput {
    pub reference_hex: String,
    pub payer_wallet: Option<String>,
    pub token_mint: Option<String>,
    pub pex_received: f64,
    pub credits_granted: f64,
    pub service_code: Option<String>,
    pub raw_event: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PexRevenueEventRecord {
    pub id: Uuid,
    pub reference_hex: String,
    pub pex_received: f64,
    pub credits_granted: f64,
    pub immediate_burn_percentage: f64,
    pub pex_burn_amount: f64,
    pub pex_remaining_amount: f64,
    pub burn_status: String,
    pub revenue_month: NaiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PexMonthlySellCapRecord {
    pub revenue_month: NaiveDate,
    pub monthly_revenue_pex: f64,
    pub monthly_burned_pex: f64,
    pub monthly_remaining_pex: f64,
    pub sell_cap_percentage: f64,
    pub monthly_sell_cap_pex: f64,
    pub monthly_sold_pex: f64,
    pub monthly_sell_allowance_remaining_pex: f64,
}

pub async fn record_pex_revenue_event(
    state: &AppState,
    input: RecordPexRevenueInput,
) -> GatewayResult<PexRevenueEventRecord> {
    let pex_received = input.pex_received.max(0.0);
    let credits_granted = input.credits_granted.max(0.0);
    let legacy_burn_percentage = state.config.pex_immediate_burn_percentage;
    let legacy_pex_burn_amount = round_token_amount(pex_received * (legacy_burn_percentage / 100.0));
    let legacy_pex_remaining_amount = round_token_amount(pex_received - legacy_pex_burn_amount);
    let revenue_month = current_revenue_month();
    let revenue_day = current_revenue_day();

    let trading_company_locked_token_account = &state.config.trading_co_treasury;
    let trading_company_revenue_token_account = &state.config.trading_company_second_wallet;

    let mut tx = state.db.begin().await?;

    let record = sqlx::query_as::<_, PexRevenueEventRecord>(
        r#"
        insert into pex_revenue_events (
            reference_hex,
            payer_wallet,
            token_mint,
            trading_company_settlement_account,
            trading_company_second_wallet,
            pex_received,
            credits_granted,
            immediate_burn_percentage,
            pex_burn_amount,
            pex_remaining_amount,
            burn_status,
            revenue_month,
            revenue_day,
            realized_after_credit,
            service_code,
            raw_event
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'declared', $11, $12, true, $13, $14)
        on conflict (reference_hex) do update
        set
            pex_received = excluded.pex_received,
            credits_granted = excluded.credits_granted,
            immediate_burn_percentage = excluded.immediate_burn_percentage,
            pex_burn_amount = excluded.pex_burn_amount,
            pex_remaining_amount = excluded.pex_remaining_amount,
            trading_company_second_wallet = excluded.trading_company_second_wallet,
            revenue_day = excluded.revenue_day,
            realized_after_credit = true,
            burn_status = case
                when pex_revenue_events.burn_status in ('approved', 'executed') then pex_revenue_events.burn_status
                else excluded.burn_status
            end,
            service_code = coalesce(excluded.service_code, pex_revenue_events.service_code),
            raw_event = coalesce(excluded.raw_event, pex_revenue_events.raw_event),
            updated_at = now()
        returning
            id,
            reference_hex,
            pex_received::float8 as pex_received,
            credits_granted::float8 as credits_granted,
            immediate_burn_percentage::float8 as immediate_burn_percentage,
            pex_burn_amount::float8 as pex_burn_amount,
            pex_remaining_amount::float8 as pex_remaining_amount,
            burn_status,
            revenue_month
        "#,
    )
    .bind(normalize_reference_hex(&input.reference_hex))
    .bind(input.payer_wallet)
    .bind(input.token_mint)
    .bind(trading_company_locked_token_account)
    .bind(trading_company_revenue_token_account)
    .bind(pex_received)
    .bind(credits_granted)
    .bind(legacy_burn_percentage)
    .bind(legacy_pex_burn_amount)
    .bind(legacy_pex_remaining_amount)
    .bind(revenue_month)
    .bind(revenue_day)
    .bind(input.service_code)
    .bind(input.raw_event)
    .fetch_one(&mut *tx)
    .await?;

    upsert_monthly_sell_cap_ledger(
        &mut tx,
        revenue_month,
        trading_company_revenue_token_account,
        pex_received,
        legacy_pex_burn_amount,
        legacy_pex_remaining_amount,
        state.config.pex_monthly_sell_cap_percentage,
    )
    .await?;

    upsert_daily_realized_burn_schedule(
        &mut tx,
        revenue_day,
        trading_company_revenue_token_account,
        pex_received,
        record.id,
    )
    .await?;

    tx.commit().await?;

    Ok(record)
}

pub async fn get_monthly_sell_cap(
    db: &PgPool,
    revenue_month: NaiveDate,
) -> GatewayResult<Option<PexMonthlySellCapRecord>> {
    let record = sqlx::query_as::<_, PexMonthlySellCapRecord>(
        r#"
        select
            revenue_month,
            monthly_revenue_pex::float8 as monthly_revenue_pex,
            monthly_burned_pex::float8 as monthly_burned_pex,
            monthly_remaining_pex::float8 as monthly_remaining_pex,
            sell_cap_percentage::float8 as sell_cap_percentage,
            monthly_sell_cap_pex::float8 as monthly_sell_cap_pex,
            monthly_sold_pex::float8 as monthly_sold_pex,
            monthly_sell_allowance_remaining_pex::float8 as monthly_sell_allowance_remaining_pex
        from pex_monthly_sell_cap_ledger
        where revenue_month = $1
        "#,
    )
    .bind(revenue_month)
    .fetch_optional(db)
    .await?;

    Ok(record)
}

async fn upsert_monthly_sell_cap_ledger(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    revenue_month: NaiveDate,
    revenue_token_account: &str,
    pex_received: f64,
    pex_burned: f64,
    pex_remaining: f64,
    sell_cap_percentage: f64,
) -> GatewayResult<()> {
    let sell_cap_delta = round_token_amount(pex_remaining * (sell_cap_percentage / 100.0));

    sqlx::query(
        r#"
        insert into pex_monthly_sell_cap_ledger (
            revenue_month,
            trading_company_second_wallet,
            monthly_revenue_pex,
            monthly_burned_pex,
            monthly_remaining_pex,
            sell_cap_percentage,
            monthly_sell_cap_pex,
            monthly_sold_pex,
            monthly_sell_allowance_remaining_pex
        ) values ($1, $2, $3, $4, $5, $6, $7, 0, $7)
        on conflict (revenue_month) do update
        set
            trading_company_second_wallet = excluded.trading_company_second_wallet,
            monthly_revenue_pex = pex_monthly_sell_cap_ledger.monthly_revenue_pex + excluded.monthly_revenue_pex,
            monthly_burned_pex = pex_monthly_sell_cap_ledger.monthly_burned_pex + excluded.monthly_burned_pex,
            monthly_remaining_pex = pex_monthly_sell_cap_ledger.monthly_remaining_pex + excluded.monthly_remaining_pex,
            monthly_sell_cap_pex = pex_monthly_sell_cap_ledger.monthly_sell_cap_pex + excluded.monthly_sell_cap_pex,
            monthly_sell_allowance_remaining_pex =
                (pex_monthly_sell_cap_ledger.monthly_sell_cap_pex + excluded.monthly_sell_cap_pex)
                - pex_monthly_sell_cap_ledger.monthly_sold_pex,
            updated_at = now()
        "#,
    )
    .bind(revenue_month)
    .bind(revenue_token_account)
    .bind(pex_received)
    .bind(pex_burned)
    .bind(pex_remaining)
    .bind(sell_cap_percentage)
    .bind(sell_cap_delta)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn upsert_daily_realized_burn_schedule(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    revenue_day: NaiveDate,
    revenue_token_account: &str,
    realized_revenue_pex: f64,
    last_revenue_event_id: Uuid,
) -> GatewayResult<()> {
    let market_input = build_market_policy_input(realized_revenue_pex);
    let decision = calculate_daily_burn_decision(market_input);
    let burn_amount = round_token_amount(realized_revenue_pex * decision.burn_rate);
    let remaining_amount = round_token_amount(realized_revenue_pex - burn_amount);
    let observed_at = Utc::now();
    let decision_id_hex = decision_id_hex_for_day(revenue_day, revenue_token_account);

    sqlx::query(
        r#"
        insert into pex_daily_realized_burns (
            revenue_day,
            trading_company_revenue_account,
            realized_revenue_pex,
            eligible_revenue_amount_pex,
            burn_percentage,
            burn_rate_bps,
            market_health_score,
            burn_amount_pex,
            remaining_revenue_pex,
            decision_id_hex,
            observed_at,
            burn_status,
            last_revenue_event_id
        ) values ($1, $2, $3, $3, $4, $5, $6, $7, $8, $9, $10, 'scheduled', $11)
        on conflict (revenue_day) do update
        set
            trading_company_revenue_account = excluded.trading_company_revenue_account,
            realized_revenue_pex = pex_daily_realized_burns.realized_revenue_pex + excluded.realized_revenue_pex,
            eligible_revenue_amount_pex = pex_daily_realized_burns.eligible_revenue_amount_pex + excluded.eligible_revenue_amount_pex,
            burn_percentage = excluded.burn_percentage,
            burn_rate_bps = excluded.burn_rate_bps,
            market_health_score = excluded.market_health_score,
            burn_amount_pex = round(((pex_daily_realized_burns.eligible_revenue_amount_pex + excluded.eligible_revenue_amount_pex) * excluded.burn_rate_bps / 10000)::numeric, 6),
            remaining_revenue_pex = round(((pex_daily_realized_burns.eligible_revenue_amount_pex + excluded.eligible_revenue_amount_pex) - ((pex_daily_realized_burns.eligible_revenue_amount_pex + excluded.eligible_revenue_amount_pex) * excluded.burn_rate_bps / 10000))::numeric, 6),
            decision_id_hex = excluded.decision_id_hex,
            observed_at = excluded.observed_at,
            burn_status = case
                when pex_daily_realized_burns.burn_status in ('executed', 'cancelled') then pex_daily_realized_burns.burn_status
                else 'scheduled'
            end,
            last_revenue_event_id = excluded.last_revenue_event_id,
            updated_at = now()
        "#,
    )
    .bind(revenue_day)
    .bind(revenue_token_account)
    .bind(realized_revenue_pex)
    .bind(decision.burn_rate_percent)
    .bind(decision.burn_rate_bps as i32)
    .bind(decision.market_health_score_u8 as i32)
    .bind(burn_amount)
    .bind(remaining_amount)
    .bind(decision_id_hex)
    .bind(observed_at)
    .bind(last_revenue_event_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn build_market_policy_input(realized_revenue_pex: f64) -> MarketPolicyInput {
    let trading_company_wallet_score = if realized_revenue_pex >= 1_000_000.0 {
        1.0
    } else if realized_revenue_pex >= 100_000.0 {
        0.7
    } else if realized_revenue_pex >= 10_000.0 {
        0.4
    } else {
        0.2
    };

    MarketPolicyInput {
        market_health_score: 0.70,
        liquidity_score: 0.60,
        utility_usage_score: 0.50,
        holder_pressure_score: 0.50,
        trading_company_wallet_score,
    }
}

fn decision_id_hex_for_day(revenue_day: NaiveDate, revenue_token_account: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("daily-realized-burn:{revenue_day}:{revenue_token_account}").as_bytes());
    let digest = hasher.finalize();
    format!("{:x}", digest)
}

fn current_revenue_month() -> NaiveDate {
    let now = Utc::now().date_naive();
    NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("valid first day of month")
}

fn current_revenue_day() -> NaiveDate {
    Utc::now().date_naive()
}

fn normalize_reference_hex(reference_hex: &str) -> String {
    reference_hex.trim().trim_start_matches("0x").to_lowercase()
}

fn round_token_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

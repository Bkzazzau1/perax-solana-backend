use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domains::solana::revenue_ledger::{RecordPexRevenueInput, record_pex_revenue_event},
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsRequest {
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub pex_balance: Option<f64>,
    pub reference_hex: Option<String>,
    pub payer_wallet: Option<String>,
    pub token_mint: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum CreditFundingMethod {
    Pex,
    Card,
    Stablecoin,
    VirtualAccount,
}

impl CreditFundingMethod {
    fn asset_code(self) -> &'static str {
        match self {
            Self::Pex => "PEX",
            Self::Card => "FIAT_USD",
            Self::Stablecoin => "USDT",
            Self::VirtualAccount => "FIAT_USD",
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct CreditExchangeRateRecord {
    asset_code: String,
    asset_name: String,
    credits_per_unit: f64,
    unit_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PexRevenueLedgerSummary {
    pub event_id: Uuid,
    pub reference_hex: String,
    pub pex_received: f64,
    pub credits_granted: f64,
    pub immediate_burn_percentage: f64,
    pub pex_burn_amount: f64,
    pub pex_remaining_amount: f64,
    pub burn_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsResponse {
    pub accepted: bool,
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub asset_code: String,
    pub asset_required: f64,
    pub pex_required: f64,
    pub remaining_pex: Option<f64>,
    pub credits_per_unit: f64,
    pub status: String,
    pub message: String,
    pub pex_revenue_ledger: Option<PexRevenueLedgerSummary>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/credits/buy", post(buy_credits))
}

async fn buy_credits(
    State(state): State<AppState>,
    Json(payload): Json<BuyCreditsRequest>,
) -> GatewayResult<Json<BuyCreditsResponse>> {
    let credit_amount = payload.credit_amount.max(0.0);
    let asset_code = payload.method.asset_code();
    let rate = get_credit_exchange_rate(&state, asset_code).await?;
    let credits_per_unit = rate.credits_per_unit;

    if credits_per_unit <= 0.0 {
        return Err(GatewayError::Upstream(format!(
            "Invalid Credit exchange rate configured for {asset_code}"
        )));
    }

    let asset_required = credit_amount / credits_per_unit;
    let pex_required = if matches!(payload.method, CreditFundingMethod::Pex) {
        asset_required
    } else {
        0.0
    };
    let remaining_pex = payload.pex_balance.map(|balance| balance - pex_required);
    let accepted = credit_amount > 0.0 && remaining_pex.map(|value| value >= 0.0).unwrap_or(true);

    let mut pex_revenue_ledger = None;
    let mut status = if accepted {
        "pending_settlement".to_string()
    } else {
        "rejected".to_string()
    };
    let mut message = if accepted {
        format!(
            "Credit purchase accepted using backend rate: {} Credits per {}.",
            credits_per_unit, rate.unit_label
        )
    } else {
        "Credit purchase rejected. Check amount or available PEX balance.".to_string()
    };

    if accepted && matches!(payload.method, CreditFundingMethod::Pex) {
        let reference_hex = payload.reference_hex.clone().ok_or_else(|| {
            GatewayError::Upstream(
                "referenceHex is required when buying Credits with PEX".to_string(),
            )
        })?;

        let record = record_pex_revenue_event(
            &state,
            RecordPexRevenueInput {
                reference_hex,
                payer_wallet: payload.payer_wallet.clone(),
                token_mint: payload.token_mint.clone(),
                pex_received: pex_required,
                credits_granted: credit_amount,
                service_code: Some("credits_buy".to_string()),
                raw_event: None,
            },
        )
        .await?;

        status = "credited_and_burn_declared".to_string();
        message = format!(
            "Credits granted. PEX revenue recorded; {}% immediate burn declared and remaining PEX assigned to Trading Company second wallet.",
            record.immediate_burn_percentage
        );

        pex_revenue_ledger = Some(PexRevenueLedgerSummary {
            event_id: record.id,
            reference_hex: record.reference_hex,
            pex_received: record.pex_received,
            credits_granted: record.credits_granted,
            immediate_burn_percentage: record.immediate_burn_percentage,
            pex_burn_amount: record.pex_burn_amount,
            pex_remaining_amount: record.pex_remaining_amount,
            burn_status: record.burn_status,
        });
    }

    Ok(Json(BuyCreditsResponse {
        accepted,
        method: payload.method,
        credit_amount,
        asset_code: asset_code.to_string(),
        asset_required,
        pex_required,
        remaining_pex,
        credits_per_unit,
        status,
        message,
        pex_revenue_ledger,
    }))
}

async fn get_credit_exchange_rate(
    state: &AppState,
    asset_code: &str,
) -> GatewayResult<CreditExchangeRateRecord> {
    sqlx::query_as::<_, CreditExchangeRateRecord>(
        r#"
        select asset_code,
               asset_name,
               credits_per_unit::double precision as credits_per_unit,
               unit_label
        from credit_exchange_rates
        where asset_code = $1 and is_active = true
        limit 1
        "#,
    )
    .bind(asset_code)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        GatewayError::Upstream(format!(
            "No active Credit exchange rate configured for {asset_code}"
        ))
    })
}

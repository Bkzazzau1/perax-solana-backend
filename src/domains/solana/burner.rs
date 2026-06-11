// src/domains/solana/burner.rs

use std::{env, time::Duration};

use tracing::{error, info};
use uuid::Uuid;

use crate::{error::GatewayError, state::AppState};

const DEFAULT_BURN_SOURCE: &str = "OpenMarketPurchase";

pub fn spawn_daily_burner(state: AppState) {
    tokio::spawn(async move {
        let interval = daily_burner_interval();

        info!(
            revenue_account = %state.config.trading_company_second_wallet,
            trading_treasury = %state.config.trading_co_treasury,
            burn_execution_mode = %state.config.burn_execution_mode.as_str(),
            interval_seconds = %interval.as_secs(),
            "Starting PEX automatic system-controlled daily burn worker"
        );

        loop {
            tokio::time::sleep(interval).await;

            info!("Checking automatic daily realized-revenue burn schedule...");

            if let Err(err) = inspect_daily_realized_burn_schedule(&state).await {
                error!(
                    error = %err,
                    "Failed to inspect automatic daily realized-revenue burn schedule"
                );
            }
        }
    });
}

fn daily_burner_interval() -> Duration {
    env::var("PEX_BURNER_INTERVAL_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(24 * 60 * 60))
}

async fn inspect_daily_realized_burn_schedule(state: &AppState) -> Result<(), GatewayError> {
    let scheduled_burns = sqlx::query_as::<_, ScheduledDailyBurn>(
        r#"
        select
            id,
            decision_id_hex,
            eligible_revenue_amount_pex::float8 as eligible_revenue_amount_pex,
            burn_amount_pex::float8 as burn_amount_pex,
            burn_rate_bps,
            market_health_score,
            extract(epoch from observed_at)::bigint as observed_at_unix
        from pex_daily_realized_burns
        where burn_status = 'scheduled'
        order by revenue_day asc
        limit 10
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    if scheduled_burns.is_empty() {
        info!("No scheduled daily realized-revenue burns found.");
        return Ok(());
    }

    for burn in scheduled_burns {
        if burn.burn_amount_pex <= 0.0 || burn.eligible_revenue_amount_pex <= 0.0 {
            mark_burn_failed(state, burn.id, "Zero eligible revenue or zero burn amount").await?;
            continue;
        }

        let params = ContractBurnParams::try_from_scheduled_burn(&burn)?;

        info!(
            burn_id = %burn.id,
            decision_id_hex = %params.decision_id_hex,
            amount_minor_units = %params.amount,
            eligible_revenue_minor_units = %params.eligible_revenue_amount,
            burn_rate_bps = %params.burn_rate_bps,
            market_health_score = %params.market_health_score,
            observed_at = %params.observed_at,
            burn_source = %params.burn_source,
            burn_execution_mode = %state.config.burn_execution_mode.as_str(),
            "Daily burn is ready for automatic execute_conditional_buyback_burn smart-contract execution."
        );

        // Disabled mode means: prepare and log only. No on-chain execution.
        if !state
            .config
            .burn_execution_mode
            .allows_automatic_execution()
        {
            continue;
        }

        mark_burn_submitted(state, burn.id).await?;

        match execute_contract_burn(state, &params).await {
            Ok(result) => {
                mark_burn_executed(state, burn.id, &result.signature, &result.burn_record).await?;

                info!(
                    burn_id = %burn.id,
                    signature = %result.signature,
                    burn_record = %result.burn_record,
                    "Automatic daily conditional buyback burn executed and recorded."
                );
            }
            Err(err) => {
                mark_burn_failed_from_submitted(state, burn.id, &err.to_string()).await?;
                return Err(err);
            }
        }
    }

    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct ScheduledDailyBurn {
    id: Uuid,
    decision_id_hex: Option<String>,
    eligible_revenue_amount_pex: f64,
    burn_amount_pex: f64,
    burn_rate_bps: i32,
    market_health_score: i32,
    observed_at_unix: Option<i64>,
}

#[derive(Debug)]
struct ContractBurnParams {
    decision_id_hex: String,
    amount: u64,
    eligible_revenue_amount: u64,
    burn_rate_bps: u16,
    market_health_score: u8,
    observed_at: i64,
    burn_source: String,
}

#[derive(Debug)]
struct ContractBurnResult {
    signature: String,
    burn_record: String,
}

impl ContractBurnParams {
    fn try_from_scheduled_burn(burn: &ScheduledDailyBurn) -> Result<Self, GatewayError> {
        let decision_id_hex = burn
            .decision_id_hex
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                GatewayError::Config("scheduled burn is missing decision_id_hex".to_string())
            })?
            .trim_start_matches("0x")
            .to_lowercase();

        validate_decision_id_hex(&decision_id_hex)?;

        let amount = pex_to_minor_units(burn.burn_amount_pex);
        let eligible_revenue_amount = pex_to_minor_units(burn.eligible_revenue_amount_pex);
        let burn_rate_bps = burn.burn_rate_bps.clamp(0, 10_000) as u16;
        let market_health_score = burn.market_health_score.clamp(0, 100) as u8;
        let observed_at = burn.observed_at_unix.unwrap_or_default();

        if amount == 0 || eligible_revenue_amount == 0 {
            return Err(GatewayError::Config(
                "burn amount and eligible revenue amount must be greater than zero".to_string(),
            ));
        }

        if observed_at <= 0 {
            return Err(GatewayError::Config(
                "scheduled burn observed_at is missing or invalid".to_string(),
            ));
        }

        Ok(Self {
            decision_id_hex,
            amount,
            eligible_revenue_amount,
            burn_rate_bps,
            market_health_score,
            observed_at,
            burn_source: DEFAULT_BURN_SOURCE.to_string(),
        })
    }
}

async fn execute_contract_burn(
    state: &AppState,
    params: &ContractBurnParams,
) -> Result<ContractBurnResult, GatewayError> {
    let executor_url = env::var("PERAX_SUPPLY_CONTROL_EXECUTOR_URL")
        .or_else(|_| env::var("PERAX_BURN_EXECUTOR_URL"))
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            GatewayError::Config(
                "PERAX_SUPPLY_CONTROL_EXECUTOR_URL is required when BURN_EXECUTION_MODE=automatic"
                    .to_string(),
            )
        })?;

    let executor_url = if executor_url.ends_with("/execute/market-condition-burn")
        || executor_url.ends_with("/execute/conditional-buyback-burn")
    {
        executor_url
    } else {
        format!("{executor_url}/execute/conditional-buyback-burn")
    };

    let payload = serde_json::json!({
        "solanaRpcUrl": state.config.solana_rpc_url,
        "programId": state.config.perax_program_id,
        "statePda": state.config.perax_state_pda,
        "pexMintAddress": state.config.pex_mint_address,
        "tradingCompanyTokenAccount": state.config.trading_co_treasury,
        "tradingCompanyRevenueTokenAccount": state.config.trading_company_second_wallet,
        "burnSource": params.burn_source,
        "decisionIdHex": params.decision_id_hex,
        "amountBaseUnits": params.amount,
        "eligibleRevenueBaseUnits": params.eligible_revenue_amount,
        "burnRateBps": params.burn_rate_bps,
        "marketHealthScore": params.market_health_score,
        "observedAtUnix": params.observed_at,
    });

    let mut request = state.http.post(executor_url).json(&payload);

    if let Ok(token) = env::var("PERAX_SUPPLY_CONTROL_EXECUTOR_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
    }

    let response = request.send().await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await?;

    if !status.is_success() {
        return Err(GatewayError::Upstream(format!(
            "supply-control executor failed with status {status}: {body}"
        )));
    }

    let signature = body
        .get("signature")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            GatewayError::Upstream(
                "supply-control executor response must include signature".to_string(),
            )
        })?;

    let burn_record = body
        .get("burnRecord")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            GatewayError::Upstream(
                "supply-control executor response must include burnRecord".to_string(),
            )
        })?;

    Ok(ContractBurnResult {
        signature,
        burn_record,
    })
}

async fn mark_burn_submitted(state: &AppState, burn_id: Uuid) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update pex_daily_realized_burns
        set burn_status = 'submitted', execution_error = null, updated_at = now()
        where id = $1 and burn_status = 'scheduled'
        "#,
    )
    .bind(burn_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn mark_burn_executed(
    state: &AppState,
    burn_id: Uuid,
    signature: &str,
    burn_record: &str,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update pex_daily_realized_burns
        set burn_status = 'executed',
            onchain_tx_signature = $2,
            onchain_burn_record = $3,
            executed_at = now(),
            execution_error = null,
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(burn_id)
    .bind(signature)
    .bind(burn_record)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn mark_burn_failed_from_submitted(
    state: &AppState,
    burn_id: Uuid,
    reason: &str,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update pex_daily_realized_burns
        set burn_status = 'failed', execution_error = $2, updated_at = now()
        where id = $1 and burn_status = 'submitted'
        "#,
    )
    .bind(burn_id)
    .bind(reason)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn mark_burn_failed(
    state: &AppState,
    burn_id: Uuid,
    reason: &str,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update pex_daily_realized_burns
        set burn_status = 'failed', execution_error = $2, updated_at = now()
        where id = $1
        "#,
    )
    .bind(burn_id)
    .bind(reason)
    .execute(&state.db)
    .await?;
    Ok(())
}

fn pex_to_minor_units(amount: f64) -> u64 {
    let scaled = (amount * 1_000_000.0).round();
    if scaled <= 0.0 { 0 } else { scaled as u64 }
}

fn validate_decision_id_hex(value: &str) -> Result<(), GatewayError> {
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GatewayError::Config(
            "decision_id_hex must be 64 hex characters".to_string(),
        ));
    }
    Ok(())
}

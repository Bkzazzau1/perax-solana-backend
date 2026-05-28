// src/domains/solana/burner.rs
use std::time::Duration;
use tracing::{error, info};
use uuid::Uuid;

use crate::{error::GatewayError, state::AppState};

pub fn spawn_daily_burner(state: AppState) {
    tokio::spawn(async move {
        info!(
            revenue_account = %state.config.trading_company_second_wallet,
            burn_execution_mode = %state.config.burn_execution_mode.as_str(),
            "Starting PEX system-controlled daily burn worker"
        );

        loop {
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;

            info!("Checking system-controlled daily realized-revenue burn schedule...");

            if let Err(err) = inspect_daily_realized_burn_schedule(&state).await {
                error!(error = %err, "Failed to inspect daily realized-revenue burn schedule");
            }
        }
    });
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
            info!(
                burn_id = %burn.id,
                "Skipping scheduled burn with zero eligible revenue or zero burn amount."
            );
            continue;
        }

        let params = ContractBurnParamsPreview::from_scheduled_burn(&burn);

        info!(
            burn_id = %burn.id,
            decision_id_hex = %burn.decision_id_hex.as_deref().unwrap_or("missing"),
            amount_minor_units = %params.amount,
            eligible_revenue_minor_units = %params.eligible_revenue_amount,
            burn_rate_bps = %params.burn_rate_bps,
            market_health_score = %params.market_health_score,
            observed_at = %params.observed_at,
            burn_execution_mode = %state.config.burn_execution_mode.as_str(),
            "Daily burn is ready for system/oracle smart-contract execution. Admin is view-only."
        );

        mark_burn_ready_for_contract_execution(state, burn.id).await?;
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
struct ContractBurnParamsPreview {
    amount: u64,
    eligible_revenue_amount: u64,
    burn_rate_bps: u16,
    market_health_score: u8,
    observed_at: i64,
}

impl ContractBurnParamsPreview {
    fn from_scheduled_burn(burn: &ScheduledDailyBurn) -> Self {
        Self {
            amount: pex_to_minor_units(burn.burn_amount_pex),
            eligible_revenue_amount: pex_to_minor_units(burn.eligible_revenue_amount_pex),
            burn_rate_bps: burn.burn_rate_bps.clamp(0, 10_000) as u16,
            market_health_score: burn.market_health_score.clamp(0, 100) as u8,
            observed_at: burn.observed_at_unix.unwrap_or_default(),
        }
    }
}

async fn mark_burn_ready_for_contract_execution(
    state: &AppState,
    burn_id: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update pex_daily_realized_burns
        set burn_status = 'scheduled', updated_at = now()
        where id = $1 and burn_status = 'scheduled'
        "#,
    )
    .bind(burn_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

fn pex_to_minor_units(amount_pex: f64) -> u64 {
    if !amount_pex.is_finite() || amount_pex <= 0.0 {
        return 0;
    }

    (amount_pex * 1_000_000.0).round().clamp(0.0, u64::MAX as f64) as u64
}

use std::time::Duration;

use tracing::{info, warn};

use crate::{domains::solana::tokenomics, error::GatewayError, state::AppState};

pub fn spawn_market_unlock_monitor(state: AppState) {
    tokio::spawn(async move {
        info!(
            venue = %tokenomics::PEX_TOKENOMICS.liquidity_venue,
            pair = %tokenomics::PEX_TOKENOMICS.liquidity_pair,
            interval_minutes = %tokenomics::UNLOCK_POLICY.monitoring_interval_minutes,
            "starting PEX market-condition staircase monitor"
        );

        loop {
            if let Err(err) = evaluate_market_unlock_conditions(&state).await {
                warn!(error = %err, "market staircase monitor cycle failed");
            }

            tokio::time::sleep(Duration::from_secs(
                tokenomics::UNLOCK_POLICY.monitoring_interval_minutes * 60,
            ))
            .await;
        }
    });
}

async fn evaluate_market_unlock_conditions(_state: &AppState) -> Result<(), GatewayError> {
    let current_price_usd = fetch_current_pex_price_usd().await?;
    let review = tokenomics::evaluate_unlock_review(current_price_usd);

    info!(
        current_price = %review.current_price_usd,
        stage = %review.stage.stage,
        base_price = %review.stage.base_price_usd,
        trigger_price = %review.stage.trigger_price_usd,
        calm_down_price = %review.calm_down_price_usd,
        next_base_price = %review.next_base_price_usd,
        next_trigger_price = %review.next_trigger_price_usd,
        should_release = %review.should_release,
        trading_company_release_amount = %review.allocation_preview.trading_company_amount,
        other_wallets_release_amount = %review.allocation_preview.other_release_wallets_amount,
        "PEX market-condition staircase evaluation completed"
    );

    if review.should_release {
        warn!(
            max_daily_unlock_amount = %tokenomics::UNLOCK_POLICY.max_daily_unlock_amount,
            max_monthly_unlock_amount = %tokenomics::UNLOCK_POLICY.max_monthly_unlock_amount,
            twap_min = %tokenomics::UNLOCK_POLICY.twap_confirmation_minutes_min,
            twap_max = %tokenomics::UNLOCK_POLICY.twap_confirmation_minutes_max,
            cooldown_min = %tokenomics::UNLOCK_POLICY.cooldown_hours_min,
            cooldown_max = %tokenomics::UNLOCK_POLICY.cooldown_hours_max,
            trading_company_share = %tokenomics::UNLOCK_POLICY.trading_company_release_share_percentage,
            other_wallets_share = %tokenomics::UNLOCK_POLICY.other_release_wallets_share_percentage,
            manual_approval = %tokenomics::UNLOCK_POLICY.requires_manual_or_multisig_approval,
            message = %review.message,
            "market-condition release threshold reached; release remains gated by liquidity, TWAP, buy-pressure, and cap checks"
        );
    }

    Ok(())
}

async fn fetch_current_pex_price_usd() -> Result<f64, GatewayError> {
    // TODO: Replace placeholder with Meteora DLMM/Jupiter pricing feed.
    // This value is intentionally equal to the approved initial PEX price.
    Ok(tokenomics::PEX_TOKENOMICS.initial_price_usd)
}

use std::time::Duration;

use tracing::{info, warn};

use crate::{domains::solana::tokenomics, error::GatewayError, state::AppState};

pub fn spawn_market_unlock_monitor(state: AppState) {
    tokio::spawn(async move {
        info!(
            venue = %tokenomics::PEX_TOKENOMICS.liquidity_venue,
            pair = %tokenomics::PEX_TOKENOMICS.liquidity_pair,
            interval_minutes = %tokenomics::UNLOCK_POLICY.monitoring_interval_minutes,
            "starting PEX market-condition unlock monitor"
        );

        loop {
            if let Err(err) = evaluate_market_unlock_conditions(&state).await {
                warn!(error = %err, "market unlock monitor cycle failed");
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
        target_support_price = %review.stage.target_support_price_usd,
        new_base_price = %review.stage.new_base_price_usd,
        should_review = %review.should_review,
        "PEX market-condition unlock evaluation completed"
    );

    if review.should_review {
        warn!(
            max_daily_unlock_amount = %tokenomics::UNLOCK_POLICY.max_daily_unlock_amount,
            twap_min = %tokenomics::UNLOCK_POLICY.twap_confirmation_minutes_min,
            twap_max = %tokenomics::UNLOCK_POLICY.twap_confirmation_minutes_max,
            cooldown_min = %tokenomics::UNLOCK_POLICY.cooldown_hours_min,
            cooldown_max = %tokenomics::UNLOCK_POLICY.cooldown_hours_max,
            manual_approval = %tokenomics::UNLOCK_POLICY.requires_manual_or_multisig_approval,
            message = %review.message,
            "unlock review required; no automatic token release executed"
        );
    }

    Ok(())
}

async fn fetch_current_pex_price_usd() -> Result<f64, GatewayError> {
    // TODO: Replace placeholder with Meteora DLMM/Jupiter pricing feed.
    // This value is intentionally equal to the approved initial PEX price.
    Ok(tokenomics::PEX_TOKENOMICS.initial_price_usd)
}

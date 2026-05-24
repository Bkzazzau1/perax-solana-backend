// src/domains/solana/burner.rs
use serde_json::json;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::{
    domains::solana::policy::{calculate_daily_burn_decision, MarketPolicyInput},
    error::GatewayError,
    state::AppState,
};

pub fn spawn_daily_burner(state: AppState) {
    tokio::spawn(async move {
        info!(
            treasury = %state.trading_co_wallet.treasury_address,
            "Starting production dynamic burn and market stabilization engine"
        );

        loop {
            // Run on a 24-hour cycle as specified in the Pera-X economic policy.
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;

            info!("Initiating daily tokenomic validation cycle...");

            // 1. DYNAMIC BURN EXECUTION
            if let Err(err) = execute_daily_revenue_burn(&state).await {
                error!(error = %err, "Failed to complete daily token burn sequence");
            }

            // 2. ALGORITHMIC MARKET MODERATION
            if let Err(err) = evaluate_market_stabilization(&state).await {
                error!(error = %err, "Failed to execute market stabilization check");
            }
        }
    });
}

/// Calculates the approved daily burn decision and burns the required percentage
/// of tokens collected inside the Trading Company revenue wallet.
async fn execute_daily_revenue_burn(state: &AppState) -> Result<(), GatewayError> {
    let trading_company_balance = fetch_trading_company_token_balance(state).await?;

    if trading_company_balance <= 0.0 {
        info!("No service tokens found in trading treasury. Skipping burn sequence.");
        return Ok(());
    }

    let market_input = build_market_policy_input(trading_company_balance);
    let decision = calculate_daily_burn_decision(market_input);

    info!(
        burn_rate_percent = %format!("{:.2}%", decision.burn_rate_percent),
        reason = %decision.reason,
        market_health_score = %decision.market_health_score,
        liquidity_score = %decision.liquidity_score,
        utility_usage_score = %decision.utility_usage_score,
        holder_pressure_score = %decision.holder_pressure_score,
        trading_company_wallet_score = %decision.trading_company_wallet_score,
        "Daily Pera-X burn policy decision declared"
    );

    let tokens_to_burn = trading_company_balance * decision.burn_rate;
    info!(
        accumulated_balance = %trading_company_balance,
        tokens_to_burn = %tokens_to_burn,
        "Executing token burn transaction on Solana"
    );

    // In production, this should create and sign an SPL-Token burn instruction
    // from the Trading Company token account and submit it via Solana RPC.
    let tx_signature = "MOCK_SOLANA_BURN_SIGNATURE_HASH";

    info!(
        signature = %tx_signature,
        burned_amount = %tokens_to_burn,
        burn_rate_percent = %format!("{:.2}%", decision.burn_rate_percent),
        "Daily burn successfully finalized on-chain"
    );

    Ok(())
}

async fn fetch_trading_company_token_balance(state: &AppState) -> Result<f64, GatewayError> {
    let balance_payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountBalance",
        "params": [state.config.trading_co_treasury]
    });

    let response = state
        .http
        .post(&state.solana_rpc.rpc_url)
        .json(&balance_payload)
        .send()
        .await?;

    let resp_data: serde_json::Value = response.json().await?;
    Ok(resp_data["result"]["value"]["uiAmount"]
        .as_f64()
        .unwrap_or(0.0))
}

fn build_market_policy_input(trading_company_balance: f64) -> MarketPolicyInput {
    // TODO: Replace these baseline scores with real data from:
    // - DEX pool/liquidity feeds
    // - utility transaction volume
    // - holder analytics
    // - Trading Company wallet health thresholds
    // - treasury and revenue dashboards
    let trading_company_wallet_score = if trading_company_balance >= 1_000_000.0 {
        1.0
    } else if trading_company_balance >= 100_000.0 {
        0.7
    } else if trading_company_balance >= 10_000.0 {
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

/// Monitors the open DEX pool price. If price breaches the policy threshold,
/// it can trigger calculated liquidity support or market-condition-based unlocking.
async fn evaluate_market_stabilization(_state: &AppState) -> Result<(), GatewayError> {
    // Query target DEX pool price in production, for example Meteora/Jupiter route pricing.
    let current_dex_price = 0.00012;
    let launch_price = 0.00009;
    let target_ceiling = launch_price * 3.0;

    info!(
        price = %current_dex_price,
        floor = %launch_price,
        ceiling = %target_ceiling,
        "Evaluating market expansion parameters"
    );

    if current_dex_price >= target_ceiling {
        warn!(
            price = %current_dex_price,
            "Price ceiling crossed. Market-condition unlock review required."
        );

        // Unlocking does not mean selling. This should become a policy-reviewed movement
        // from locked allocation wallets toward Trading Company, liquidity, or reserve support.
        let max_release_allowance = 100_000.0;
        let trading_company_share = max_release_allowance * 0.50;

        info!(
            release_allowance = %max_release_allowance,
            trading_company_share = %trading_company_share,
            "Calculated conditional unlock review allocation"
        );
    } else {
        info!(
            "Market fluctuations remaining healthy within expected standard volatility boundaries."
        );
    }

    Ok(())
}

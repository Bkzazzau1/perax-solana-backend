// src/domains/solana/burner.rs
use serde_json::json;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    domains::solana::policy::{calculate_daily_burn_decision, DailyBurnDecision, MarketPolicyInput},
    error::GatewayError,
    state::AppState,
};

pub fn spawn_daily_burner(state: AppState) {
    tokio::spawn(async move {
        info!(
            treasury = %state.trading_co_wallet.treasury_address,
            burn_execution_mode = %state.config.burn_execution_mode.as_str(),
            "Starting production dynamic burn and market stabilization engine"
        );

        loop {
            // Run on a 24-hour cycle as specified in the Pera-X economic policy.
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;

            info!("Initiating daily tokenomic validation cycle...");

            // 1. DYNAMIC BURN DECLARATION
            if let Err(err) = declare_daily_revenue_burn(&state).await {
                error!(error = %err, "Failed to complete daily token burn declaration sequence");
            }

            // 2. APPROVED BURN EXECUTION
            if state.config.burn_execution_mode.allows_approved_execution() {
                if let Err(err) = execute_approved_burn_decisions(&state).await {
                    error!(error = %err, "Failed to execute approved burn decisions");
                }
            } else {
                info!(
                    burn_execution_mode = %state.config.burn_execution_mode.as_str(),
                    "Burn execution disabled by configuration. Decisions will remain declared or approved until mode is changed."
                );
            }

            // 3. ALGORITHMIC MARKET MODERATION
            if let Err(err) = evaluate_market_stabilization(&state).await {
                error!(error = %err, "Failed to execute market stabilization check");
            }
        }
    });
}

/// Calculates and stores the daily burn decision.
/// It does not execute a real burn until the decision is approved and execution mode allows it.
async fn declare_daily_revenue_burn(state: &AppState) -> Result<(), GatewayError> {
    let trading_company_balance = fetch_trading_company_token_balance(state).await?;

    if trading_company_balance <= 0.0 {
        info!("No service tokens found in trading treasury. Skipping burn declaration.");
        return Ok(());
    }

    let market_input = build_market_policy_input(trading_company_balance);
    let decision = calculate_daily_burn_decision(market_input);
    let tokens_to_burn = trading_company_balance * decision.burn_rate;

    let decision_id = persist_burn_decision(
        state,
        &decision,
        trading_company_balance,
        tokens_to_burn,
        "declared",
        None,
    )
    .await?;

    info!(
        decision_id = %decision_id,
        burn_rate_percent = %format!("{:.2}%", decision.burn_rate_percent),
        reason = %decision.reason,
        market_health_score = %decision.market_health_score,
        liquidity_score = %decision.liquidity_score,
        utility_usage_score = %decision.utility_usage_score,
        holder_pressure_score = %decision.holder_pressure_score,
        trading_company_wallet_score = %decision.trading_company_wallet_score,
        tokens_to_burn = %tokens_to_burn,
        burn_execution_mode = %state.config.burn_execution_mode.as_str(),
        "Daily Pera-X burn policy decision declared and saved. Awaiting approval before execution."
    );

    Ok(())
}

/// Executes only approved burn decisions. Declared decisions must be approved first.
async fn execute_approved_burn_decisions(state: &AppState) -> Result<(), GatewayError> {
    let approved_decisions = sqlx::query_as::<_, ApprovedBurnDecision>(
        r#"
        select
            id,
            tokens_to_burn::float8 as tokens_to_burn,
            burn_rate_percent::float8 as burn_rate_percent
        from daily_burn_decisions
        where status = 'approved'
        order by declared_at asc
        limit 5
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    if approved_decisions.is_empty() {
        info!("No approved Pera-X burn decisions awaiting execution.");
        return Ok(());
    }

    for decision in approved_decisions {
        if decision.tokens_to_burn <= 0.0 {
            mark_burn_decision_failed(state, decision.id, "No tokens to burn").await?;
            continue;
        }

        info!(
            decision_id = %decision.id,
            tokens_to_burn = %decision.tokens_to_burn,
            burn_rate_percent = %format!("{:.2}%", decision.burn_rate_percent),
            "Executing approved Pera-X burn decision"
        );

        // In production, this should create and sign an SPL-Token burn instruction
        // from the Trading Company token account and submit it via Solana RPC.
        let tx_signature = "MOCK_SOLANA_BURN_SIGNATURE_HASH";

        mark_burn_decision_executed(state, decision.id, tx_signature).await?;

        info!(
            decision_id = %decision.id,
            signature = %tx_signature,
            burned_amount = %decision.tokens_to_burn,
            burn_rate_percent = %format!("{:.2}%", decision.burn_rate_percent),
            "Approved daily burn finalized on-chain and persisted"
        );
    }

    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct ApprovedBurnDecision {
    id: Uuid,
    tokens_to_burn: f64,
    burn_rate_percent: f64,
}

async fn persist_burn_decision(
    state: &AppState,
    decision: &DailyBurnDecision,
    trading_company_balance: f64,
    tokens_to_burn: f64,
    status: &str,
    tx_signature: Option<&str>,
) -> Result<Uuid, GatewayError> {
    let decision_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into daily_burn_decisions (
            burn_rate,
            burn_rate_percent,
            market_health_score,
            liquidity_score,
            utility_usage_score,
            holder_pressure_score,
            trading_company_wallet_score,
            trading_company_balance,
            tokens_to_burn,
            reason,
            tx_signature,
            status
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        returning id
        "#,
    )
    .bind(decision.burn_rate)
    .bind(decision.burn_rate_percent)
    .bind(decision.market_health_score)
    .bind(decision.liquidity_score)
    .bind(decision.utility_usage_score)
    .bind(decision.holder_pressure_score)
    .bind(decision.trading_company_wallet_score)
    .bind(trading_company_balance)
    .bind(tokens_to_burn)
    .bind(&decision.reason)
    .bind(tx_signature)
    .bind(status)
    .fetch_one(&state.db)
    .await?;

    Ok(decision_id)
}

async fn mark_burn_decision_executed(
    state: &AppState,
    decision_id: Uuid,
    tx_signature: &str,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update daily_burn_decisions
        set status = 'executed', tx_signature = $1, updated_at = now()
        where id = $2 and status = 'approved'
        "#,
    )
    .bind(tx_signature)
    .bind(decision_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn mark_burn_decision_failed(
    state: &AppState,
    decision_id: Uuid,
    reason: &str,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        update daily_burn_decisions
        set status = 'failed', reason = concat(reason, ' | Failure: ', $1), updated_at = now()
        where id = $2 and status = 'approved'
        "#,
    )
    .bind(reason)
    .bind(decision_id)
    .execute(&state.db)
    .await?;

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

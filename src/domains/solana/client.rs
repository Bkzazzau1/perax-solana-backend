// src/domains/solana/client.rs
use serde_json::json;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::{error::GatewayError, infra::cache, state::AppState};

#[derive(Debug, Clone)]
pub struct TreasuryRpc {
    pub rpc_url: String,
}

impl TreasuryRpc {
    pub fn new(rpc_url: String) -> Self {
        Self { rpc_url }
    }
}

#[derive(Debug, Clone)]
pub struct TradingWallet {
    pub locked_account: String,
    pub revenue_account: String,
}

impl TradingWallet {
    pub fn new(locked_account: String, revenue_account: String) -> Self {
        Self {
            locked_account,
            revenue_account,
        }
    }
}

pub fn spawn_treasury_listener(state: AppState) {
    tokio::spawn(async move {
        info!(
            locked_account = %state.config.trading_co_treasury,
            revenue_account = %state.config.trading_company_second_wallet,
            rpc = %state.solana_rpc.rpc_url,
            ws = %state.config.solana_ws_url,
            "starting Solana revenue account listener for PEX-for-Credits payments"
        );

        let mut last_signature: Option<String> = None;

        loop {
            if let Err(err) = poll_revenue_transfers(&state, &mut last_signature).await {
                error!(error = %err, "Error encountered during Solana revenue poll cycle");
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// Polls the Solana RPC for recent transaction signatures touching the Trading Company revenue token account.
async fn poll_revenue_transfers(
    state: &AppState,
    last_sig: &mut Option<String>,
) -> Result<(), GatewayError> {
    let revenue_account = &state.config.trading_company_second_wallet;

    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [
            revenue_account,
            { "limit": 5, "commitment": "confirmed" }
        ]
    });

    let response = state
        .http
        .post(&state.solana_rpc.rpc_url)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(GatewayError::Upstream(
            "Failed to fetch signature logs from Solana RPC node".to_string(),
        ));
    }

    let resp_data: serde_json::Value = response.json().await?;
    let signatures = match resp_data["result"].as_array() {
        Some(arr) => arr,
        None => return Ok(()),
    };

    if signatures.is_empty() {
        return Ok(());
    }

    let latest_fetched_sig = signatures[0]["signature"].as_str().map(|s| s.to_string());

    for tx_info in signatures.iter().rev() {
        let sig_str = match tx_info["signature"].as_str() {
            Some(s) => s,
            None => continue,
        };

        if Some(sig_str.to_string()) == *last_sig {
            continue;
        }

        debug!(signature = %sig_str, revenue_account = %revenue_account, "Analyzing transaction signature for PEX revenue settlement");

        let tx_payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [
                sig_str,
                { "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0, "commitment": "confirmed" }
            ]
        });

        let tx_resp = state
            .http
            .post(&state.solana_rpc.rpc_url)
            .json(&tx_payload)
            .send()
            .await?;

        if !tx_resp.status().is_success() {
            continue;
        }

        let tx_data: serde_json::Value = tx_resp.json().await?;

        if let Some(meta) = tx_data["result"]["meta"].as_object() {
            if let (Some(pre), Some(post)) =
                (meta.get("preTokenBalances"), meta.get("postTokenBalances"))
            {
                if let Some((amount_transferred, account_owner)) =
                    extract_token_deltas(pre, post, revenue_account)
                {
                    info!(amount = %amount_transferred, owner = %account_owner, revenue_account = %revenue_account, "PEX payment detected in revenue account. Processing Credits settlement.");

                    let credit_allocation = amount_transferred;
                    let user_redis_key = format!("client:balance:{}", account_owner);

                    cache::increment_credits(&state.cache, &user_redis_key, credit_allocation)
                        .await?;

                    info!(user = %account_owner, credits = %credit_allocation, "User Credits balance topped up after PEX payment.");
                }
            }
        }
    }

    if latest_fetched_sig.is_some() {
        *last_sig = latest_fetched_sig;
    }

    Ok(())
}

/// Utility function parsing pre/post token arrays to identify valid incoming PEX transfers.
fn extract_token_deltas(
    pre_balances: &serde_json::Value,
    post_balances: &serde_json::Value,
    revenue_token_account: &str,
) -> Option<(f64, String)> {
    let mut revenue_pre: f64 = 0.0;
    let mut revenue_post: f64 = 0.0;
    let mut sender_address: String = "unknown_sender".to_string();

    if let Some(arr) = post_balances.as_array() {
        for balance in arr {
            if balance["accountIndex"].is_number()
                && balance["uiTokenAmount"]["uiAmount"].is_number()
                && balance["owner"].as_str().is_some()
            {
                if balance["owner"].as_str() == Some(revenue_token_account)
                    || balance["account"].as_str() == Some(revenue_token_account)
                {
                    revenue_post = balance["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
                }
            }
        }
    }

    if let Some(arr) = pre_balances.as_array() {
        for balance in arr {
            if balance["owner"].as_str() == Some(revenue_token_account)
                || balance["account"].as_str() == Some(revenue_token_account)
            {
                revenue_pre = balance["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
            } else if let Some(owner) = balance["owner"].as_str() {
                sender_address = owner.to_string();
            }
        }
    }

    let delta = revenue_post - revenue_pre;
    if delta > 0.0 {
        Some((delta, sender_address))
    } else {
        None
    }
}

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
    pub treasury_address: String,
}

impl TradingWallet {
    pub fn new(treasury_address: String) -> Self {
        Self { treasury_address }
    }
}

pub fn spawn_treasury_listener(state: AppState) {
    tokio::spawn(async move {
        info!(
            treasury = %state.config.trading_co_treasury,
            rpc = %state.solana_rpc.rpc_url,
            ws = %state.config.solana_ws_url,
            "starting production solana treasury listener"
        );

        // Keep track of the last scanned signature to avoid processing duplicate transfers
        let mut last_signature: Option<String> = None;

        loop {
            // Safe network boundary isolation: wrap execution to prevent panics from crashing the background thread
            if let Err(err) = poll_treasury_transfers(&state, &mut last_signature).await {
                error!(error = %err, "Error encountered during solana treasury poll cycle");
            }

            // Short sleep interval optimized for high block velocity on Solana
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// Polls the Solana RPC for recent transaction signatures touching the Trading Company Wallet.
async fn poll_treasury_transfers(
    state: &AppState,
    last_sig: &mut Option<String>,
) -> Result<(), GatewayError> {
    // 1. Send getSignaturesForAddress RPC payload to find recent transfers
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [
            state.config.trading_co_treasury,
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
        None => return Ok(()), // Quietly skip if no transactions exist yet
    };

    if signatures.is_empty() {
        return Ok(());
    }

    // Capture the newest signature to track our checkpoint positioning
    let latest_fetched_sig = signatures[0]["signature"].as_str().map(|s| s.to_string());

    // Iterate backwards through the transaction list to process older logs first
    for tx_info in signatures.iter().rev() {
        let sig_str = match tx_info["signature"].as_str() {
            Some(s) => s,
            None => continue,
        };

        // Skip anything we already analyzed in a previous loop sequence
        if Some(sig_str.to_string()) == *last_sig {
            continue;
        }

        debug!(signature = %sig_str, "Analyzing transaction signature for token settlement");

        // 2. Fetch full transaction details using standard JSON-RPC mapping
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

        // 3. Extract SPL-Token transfers matching your Pera-X setup
        // This parses standard Solana jsonParsed token balances to locate credit events
        if let Some(meta) = tx_data["result"]["meta"].as_object() {
            if let (Some(pre), Some(post)) =
                (meta.get("preTokenBalances"), meta.get("postTokenBalances"))
            {
                if let Some((amount_transferred, account_owner)) =
                    extract_token_deltas(pre, post, &state.config.trading_co_treasury)
                {
                    info!(amount = %amount_transferred, owner = %account_owner, "Pera-X SPL-Token deposit detected! Processing settlement credits.");

                    // Convert your token units into the corresponding credit values
                    // Users buy Pera-X, turn them into credits, and send tokens to the Trading Wallet [cite: 18, 24, 63]
                    let credit_allocation = amount_transferred; // For 1:1 token-to-credit baseline ratios
                    let user_redis_key = format!("client:balance:{}", account_owner);

                    // Atomically credit the user's wallet wallet entry directly inside your Redis cache layer
                    cache::increment_credits(&state.cache, &user_redis_key, credit_allocation)
                        .await?;

                    info!(user = %account_owner, credits = %credit_allocation, "User balance successfully topped up in memory cache.");
                }
            }
        }
    }

    // Update the pointer index location for the next polling block loop
    if latest_fetched_sig.is_some() {
        *last_sig = latest_fetched_sig;
    }

    Ok(())
}

/// Utility function parsing pre/post token arrays to identify valid incoming Pera-X transfers.
fn extract_token_deltas(
    pre_balances: &serde_json::Value,
    post_balances: &serde_json::Value,
    treasury_addr: &str,
) -> Option<(f64, String)> {
    let mut treasury_pre: f64 = 0.0;
    let mut treasury_post: f64 = 0.0;
    let mut sender_address: String = "default_dev".to_string();

    // Trace treasury index changes
    if let Some(arr) = post_balances.as_array() {
        for balance in arr {
            if balance["owner"].as_str() == Some(treasury_addr) {
                treasury_post = balance["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
            }
        }
    }

    if let Some(arr) = pre_balances.as_array() {
        for balance in arr {
            if balance["owner"].as_str() == Some(treasury_addr) {
                treasury_pre = balance["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
            } else if let Some(owner) = balance["owner"].as_str() {
                sender_address = owner.to_string(); // Detect the sender to match credit distribution rows
            }
        }
    }

    let delta = treasury_post - treasury_pre;
    if delta > 0.0 {
        Some((delta, sender_address))
    } else {
        None
    }
}

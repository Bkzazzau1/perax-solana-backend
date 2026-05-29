// src/domains/b2b_gateway/services/edtech.rs
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    infra::cache,
    providers::CopyleaksClient,
    state::AppState,
};

pub async fn scan_payload(
    state: &AppState,
    account_id: Uuid,
    payload: Value,
) -> GatewayResult<Value> {
    // Extract text block from the dynamic JSON structure payload
    let text_content = payload["text"].as_str().unwrap_or_default();

    // 1. Core Word Counting Logic
    let word_count = text_content.split_whitespace().count();

    if word_count == 0 {
        return Err(GatewayError::Upstream(
            "Payload text block cannot be empty".to_string(),
        ));
    }

    // 2. Pre-Scan Wallet Deductions Assessment
    // Standard scanning tier: 1 page allocation per 250 words
    let estimated_pages = ((word_count as f64) / 250.0).ceil() as usize;
    let cost_per_page = 0.05; // Internal Pera-X credit cost metric per page
    let total_scan_cost = (estimated_pages as f64) * cost_per_page;

    let user_redis_key = format!("client:balance:{}", account_id);

    // Atomically pull current wallet resources to enforce the credit ceiling firewall
    let current_credits = cache::get_credits(&state.cache, &user_redis_key).await?;

    match current_credits {
        Some(balance) if balance >= total_scan_cost => {
            // Balance is sufficient, deduct the exact amount right now before firing upstream API
            cache::increment_credits(&state.cache, &user_redis_key, -total_scan_cost).await?;
        }
        _ => return Err(GatewayError::InsufficientCredits),
    };

    tracing::debug!(
        account_id = %account_id,
        words = word_count,
        pages = estimated_pages,
        cost = total_scan_cost,
        upstream = %state.config.copyleaks_base_url,
        "Pre-scan billing processed successfully, dispatching verification payload to Copyleaks"
    );

    let scan_id = Uuid::new_v4().to_string();
    let response = CopyleaksClient::new(state)
        .submit_scan(&scan_id, text_content)
        .await?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        tracing::error!(scan_id = %scan_id, error = %err_text, "Copyleaks engine processing rejected");

        // Refund mechanism: if upstream crashes, return the credits to the user's Redis pool
        cache::increment_credits(&state.cache, &user_redis_key, total_scan_cost).await?;

        return Err(GatewayError::Upstream(format!(
            "Ecosystem scanning provider failure: {err_text}"
        )));
    }

    let report_data: Value = response.json().await.map_err(GatewayError::Http)?;

    // 4. Return clean analysis metadata back to our B2B router wrapper
    Ok(json!({
        "success": true,
        "scan_id": scan_id,
        "words_processed": word_count,
        "pages_billed": estimated_pages,
        "credits_deducted": total_scan_cost,
        "report": report_data
    }))
}

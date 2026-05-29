// src/domains/b2b_gateway/services/claude.rs
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    infra::cache,
    providers::AnthropicClient,
    state::AppState,
};

pub async fn proxy_message_stream(
    state: &AppState,
    account_id: Uuid,
    payload: Value,
) -> GatewayResult<Response> {
    // 1. Pre-Flight Security Check against our Redis memory firewall
    if !cache::account_has_credits(&state.cache, account_id).await? {
        return Err(GatewayError::InsufficientCredits);
    }

    let initial_estimate = estimate_tokens(&payload);
    tracing::debug!(
        account_id = %account_id,
        estimated_input_tokens = initial_estimate,
        upstream = %state.config.claude_base_url,
        "Claude AI proxy request validated, initiating upstream network pipe"
    );

    // 2. Prepare the outbound request using the Anthropic provider credential.
    let response = AnthropicClient::new(state)
        .stream_messages(&payload)
        .await?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        tracing::error!(account_id = %account_id, upstream_error = %err_text, "Anthropic API rejected request execution");
        return Err(GatewayError::Upstream(format!(
            "Claude provider error: {err_text}"
        )));
    }

    // Capture the Byte Stream from our HTTP pool connection handle
    let raw_byte_stream = response.bytes_stream();
    let cache_client = state.cache.clone();
    let user_redis_key = format!("client:balance:{}", account_id);

    // 3. Create an Asynchronous Transform Loop using mapping streams
    let processed_stream = raw_byte_stream.map(move |chunk_result| {
        match chunk_result {
            Ok(bytes) => {
                let chunk_str = String::from_utf8_lossy(&bytes);

                // Count newly streaming words roughly to evaluate live cost deductions
                let incremental_tokens = chunk_str.split_whitespace().count();

                if incremental_tokens > 0 {
                    let cache_store = cache_client.clone();
                    let redis_key = user_redis_key.clone();

                    // We execute this asynchronously within the stream iterator using a Tokio task block
                    tokio::spawn(async move {
                        // Deduct credits dynamically proportional to usage processing rates
                        let token_cost_factor = 0.00001; // Internal credit mapping calculation scalar
                        let cost = (incremental_tokens as f64) * token_cost_factor;

                        let _ = cache::increment_credits(&cache_store, &redis_key, -cost).await;
                    });
                }

                // Yield the text chunks safely down the router line
                Ok::<_, std::io::Error>(axum::body::Bytes::from(bytes))
            }
            Err(err) => {
                tracing::error!(error = %err, "Data stream failure encountered mid-transmission");
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    err,
                ))
            }
        }
    });

    // 4. Return a streaming response wrapper back to the Axum Router
    // This allows web clients to parse Server-Sent Events (SSE) immediately chunk by chunk
    Ok(Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .body(axum::body::Body::from_stream(processed_stream))
        .unwrap()
        .into_response())
}

fn estimate_tokens(payload: &Value) -> usize {
    payload.to_string().split_whitespace().count().max(1)
}

use axum::{Json, Router, extract::{Query, State}, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pyth/status", get(pyth_status))
        .route("/pyth/latest", get(pyth_latest))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PythStatusResponse {
    configured: bool,
    price_service_url: String,
    known_symbols: Vec<&'static str>,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythLatestQuery {
    feed_id: Option<String>,
    symbol: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PythLatestResponse {
    accepted: bool,
    feed_id: String,
    symbol: Option<String>,
    price: Option<f64>,
    confidence: Option<f64>,
    exponent: Option<i32>,
    publish_time: Option<i64>,
    provider_response: Value,
}

async fn pyth_status() -> Json<PythStatusResponse> {
    Json(PythStatusResponse {
        configured: true,
        price_service_url: pyth_price_service_url(),
        known_symbols: vec!["SOL", "BTC", "ETH", "USDC", "PEX"],
        message: "Pyth Hermes price service is configured for latest price reads.".to_string(),
    })
}

async fn pyth_latest(
    State(state): State<AppState>,
    Query(query): Query<PythLatestQuery>,
) -> GatewayResult<Json<PythLatestResponse>> {
    let symbol = query.symbol.as_deref().map(|value| value.trim().to_uppercase()).filter(|value| !value.is_empty());
    let feed_id = query.feed_id
        .as_deref()
        .map(clean_feed_id)
        .filter(|value| !value.is_empty())
        .or_else(|| symbol.as_deref().and_then(resolve_symbol_feed))
        .ok_or_else(|| GatewayError::Upstream("feedId or supported symbol is required".to_string()))?;

    let url = format!("{}/v2/updates/price/latest?ids[]={}&parsed=true", pyth_price_service_url(), feed_id);
    let response = state.http.get(url).send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() {
        return Err(GatewayError::Upstream(format!("Pyth latest price request failed: {body}")));
    }

    let parsed = body.get("parsed").and_then(Value::as_array).and_then(|items| items.first()).cloned().unwrap_or(Value::Null);
    let price_obj = parsed.get("price").unwrap_or(&Value::Null);
    let exponent = price_obj.get("expo").and_then(Value::as_i64).map(|v| v as i32);
    let raw_price = price_obj.get("price").and_then(value_to_f64);
    let raw_conf = price_obj.get("conf").and_then(value_to_f64);
    let price = match (raw_price, exponent) {
        (Some(value), Some(expo)) => Some(value * 10_f64.powi(expo)),
        _ => None,
    };
    let confidence = match (raw_conf, exponent) {
        (Some(value), Some(expo)) => Some(value * 10_f64.powi(expo)),
        _ => None,
    };
    let publish_time = price_obj.get("publish_time").and_then(Value::as_i64);

    sqlx::query("insert into pyth_price_snapshots (feed_id, symbol, price, confidence, exponent, publish_time, provider_payload) values ($1,$2,$3,$4,$5,$6,$7)")
        .bind(&feed_id)
        .bind(symbol.clone())
        .bind(price)
        .bind(confidence)
        .bind(exponent)
        .bind(publish_time)
        .bind(body.clone())
        .execute(&state.db)
        .await?;

    Ok(Json(PythLatestResponse {
        accepted: true,
        feed_id,
        symbol,
        price,
        confidence,
        exponent,
        publish_time,
        provider_response: body,
    }))
}

fn resolve_symbol_feed(symbol: &str) -> Option<String> {
    match symbol {
        "SOL" => env_feed("PYTH_SOL_PRICE_FEED_ID"),
        "BTC" => env_feed("PYTH_BTC_PRICE_FEED_ID"),
        "ETH" => env_feed("PYTH_ETH_PRICE_FEED_ID"),
        "USDC" => env_feed("PYTH_USDC_PRICE_FEED_ID"),
        "PEX" => env_feed("PYTH_PEX_PRICE_FEED_ID"),
        _ => None,
    }
}

fn env_feed(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|value| clean_feed_id(&value)).filter(|value| !value.is_empty() && !value.starts_with("replace-"))
}

fn clean_feed_id(value: &str) -> String {
    value.trim().trim_start_matches("0x").to_string()
}

fn pyth_price_service_url() -> String {
    std::env::var("PYTH_PRICE_SERVICE_URL").unwrap_or_else(|_| "https://hermes.pyth.network".to_string())
}

fn value_to_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse::<f64>().ok())
}

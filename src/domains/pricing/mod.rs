use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UtilityPriceRecord {
    pub service_code: String,
    pub service_name: String,
    pub category: String,
    pub credit_cost: f64,
    pub billing_unit: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityPricingResponse {
    pub pricing: Vec<UtilityPriceRecord>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CreditExchangeRateRecord {
    pub asset_code: String,
    pub asset_name: String,
    pub credits_per_unit: f64,
    pub unit_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditExchangeRatesResponse {
    pub rates: Vec<CreditExchangeRateRecord>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pricing/utilities", get(list_utility_pricing))
        .route(
            "/pricing/utilities/{service_code}",
            get(get_utility_pricing),
        )
        .route("/pricing/credit-rates", get(list_credit_exchange_rates))
}

pub async fn list_utility_pricing(
    State(state): State<AppState>,
) -> GatewayResult<Json<UtilityPricingResponse>> {
    let pricing = sqlx::query_as::<_, UtilityPriceRecord>(
        r#"
        select service_code,
               service_name,
               category,
               credit_cost::double precision as credit_cost,
               billing_unit
        from utility_pricing_settings
        where is_active = true
        order by category asc, service_name asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(UtilityPricingResponse { pricing }))
}

pub async fn get_utility_pricing(
    State(state): State<AppState>,
    Path(service_code): Path<String>,
) -> GatewayResult<Json<UtilityPriceRecord>> {
    let price = get_utility_price(&state, &service_code).await?;
    Ok(Json(price))
}

pub async fn list_credit_exchange_rates(
    State(state): State<AppState>,
) -> GatewayResult<Json<CreditExchangeRatesResponse>> {
    let rates = sqlx::query_as::<_, CreditExchangeRateRecord>(
        r#"
        select asset_code,
               asset_name,
               credits_per_unit::double precision as credits_per_unit,
               unit_label
        from credit_exchange_rates
        where is_active = true
        order by asset_code asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(CreditExchangeRatesResponse { rates }))
}

pub async fn get_utility_price(
    state: &AppState,
    service_code: &str,
) -> GatewayResult<UtilityPriceRecord> {
    sqlx::query_as::<_, UtilityPriceRecord>(
        r#"
        select service_code,
               service_name,
               category,
               credit_cost::double precision as credit_cost,
               billing_unit
        from utility_pricing_settings
        where service_code = $1 and is_active = true
        limit 1
        "#,
    )
    .bind(service_code)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        GatewayError::Upstream(format!(
            "No active pricing configured for service: {service_code}"
        ))
    })
}

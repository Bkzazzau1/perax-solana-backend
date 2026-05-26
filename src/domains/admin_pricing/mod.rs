use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/api/pricing/utilities", get(list_utility_prices))
        .route(
            "/admin/api/pricing/utilities/{service_code}",
            patch(update_utility_price),
        )
        .route("/admin/api/pricing/credit-rates", get(list_credit_rates))
        .route(
            "/admin/api/pricing/credit-rates/{asset_code}",
            patch(update_credit_rate),
        )
        .route("/admin/api/telecom/number-pricing", get(list_number_prices))
        .route(
            "/admin/api/telecom/number-pricing/{id}",
            patch(update_number_price),
        )
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UtilityPriceAdminRecord {
    pub service_code: String,
    pub service_name: String,
    pub category: String,
    pub credit_cost: f64,
    pub billing_unit: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUtilityPriceRequest {
    pub service_name: Option<String>,
    pub category: Option<String>,
    pub credit_cost: Option<f64>,
    pub billing_unit: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityPricesAdminResponse {
    pub pricing: Vec<UtilityPriceAdminRecord>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CreditRateAdminRecord {
    pub asset_code: String,
    pub asset_name: String,
    pub credits_per_unit: f64,
    pub unit_label: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCreditRateRequest {
    pub asset_name: Option<String>,
    pub credits_per_unit: Option<f64>,
    pub unit_label: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditRatesAdminResponse {
    pub rates: Vec<CreditRateAdminRecord>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NumberPriceAdminRecord {
    pub id: Uuid,
    pub country: String,
    pub number_type: String,
    pub setup_fee_credits: f64,
    pub monthly_fee_credits: f64,
    pub annual_fee_credits: f64,
    pub currency: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNumberPriceRequest {
    pub country: Option<String>,
    pub number_type: Option<String>,
    pub setup_fee_credits: Option<f64>,
    pub monthly_fee_credits: Option<f64>,
    pub annual_fee_credits: Option<f64>,
    pub currency: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberPricesAdminResponse {
    pub pricing: Vec<NumberPriceAdminRecord>,
}

async fn list_utility_prices(
    State(state): State<AppState>,
) -> GatewayResult<Json<UtilityPricesAdminResponse>> {
    let pricing = sqlx::query_as::<_, UtilityPriceAdminRecord>(
        r#"
        select service_code,
               service_name,
               category,
               credit_cost::double precision as credit_cost,
               billing_unit,
               is_active
        from utility_pricing_settings
        order by category asc, service_name asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(UtilityPricesAdminResponse { pricing }))
}

async fn update_utility_price(
    State(state): State<AppState>,
    Path(service_code): Path<String>,
    Json(payload): Json<UpdateUtilityPriceRequest>,
) -> GatewayResult<Json<UtilityPriceAdminRecord>> {
    if let Some(cost) = payload.credit_cost {
        validate_non_negative(cost, "creditCost")?;
    }

    let updated = sqlx::query_as::<_, UtilityPriceAdminRecord>(
        r#"
        update utility_pricing_settings
        set service_name = coalesce($2, service_name),
            category = coalesce($3, category),
            credit_cost = coalesce($4, credit_cost),
            billing_unit = coalesce($5, billing_unit),
            is_active = coalesce($6, is_active),
            updated_at = now()
        where service_code = $1
        returning service_code,
                  service_name,
                  category,
                  credit_cost::double precision as credit_cost,
                  billing_unit,
                  is_active
        "#,
    )
    .bind(service_code)
    .bind(clean_optional_text(payload.service_name))
    .bind(clean_optional_text(payload.category))
    .bind(payload.credit_cost)
    .bind(clean_optional_text(payload.billing_unit))
    .bind(payload.is_active)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Utility pricing record not found.".to_string()))?;

    Ok(Json(updated))
}

async fn list_credit_rates(
    State(state): State<AppState>,
) -> GatewayResult<Json<CreditRatesAdminResponse>> {
    let rates = sqlx::query_as::<_, CreditRateAdminRecord>(
        r#"
        select asset_code,
               asset_name,
               credits_per_unit::double precision as credits_per_unit,
               unit_label,
               is_active
        from credit_exchange_rates
        order by asset_code asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(CreditRatesAdminResponse { rates }))
}

async fn update_credit_rate(
    State(state): State<AppState>,
    Path(asset_code): Path<String>,
    Json(payload): Json<UpdateCreditRateRequest>,
) -> GatewayResult<Json<CreditRateAdminRecord>> {
    if let Some(rate) = payload.credits_per_unit {
        validate_positive(rate, "creditsPerUnit")?;
    }

    let updated = sqlx::query_as::<_, CreditRateAdminRecord>(
        r#"
        update credit_exchange_rates
        set asset_name = coalesce($2, asset_name),
            credits_per_unit = coalesce($3, credits_per_unit),
            unit_label = coalesce($4, unit_label),
            is_active = coalesce($5, is_active),
            updated_at = now()
        where asset_code = $1
        returning asset_code,
                  asset_name,
                  credits_per_unit::double precision as credits_per_unit,
                  unit_label,
                  is_active
        "#,
    )
    .bind(asset_code)
    .bind(clean_optional_text(payload.asset_name))
    .bind(payload.credits_per_unit)
    .bind(clean_optional_text(payload.unit_label))
    .bind(payload.is_active)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Credit exchange rate not found.".to_string()))?;

    Ok(Json(updated))
}

async fn list_number_prices(
    State(state): State<AppState>,
) -> GatewayResult<Json<NumberPricesAdminResponse>> {
    let pricing = sqlx::query_as::<_, NumberPriceAdminRecord>(
        r#"
        select id,
               country,
               number_type,
               setup_fee_credits::double precision as setup_fee_credits,
               monthly_fee_credits::double precision as monthly_fee_credits,
               annual_fee_credits::double precision as annual_fee_credits,
               currency,
               is_active
        from number_pricing_settings
        order by country asc, number_type asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(NumberPricesAdminResponse { pricing }))
}

async fn update_number_price(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateNumberPriceRequest>,
) -> GatewayResult<Json<NumberPriceAdminRecord>> {
    if let Some(value) = payload.setup_fee_credits {
        validate_non_negative(value, "setupFeeCredits")?;
    }
    if let Some(value) = payload.monthly_fee_credits {
        validate_non_negative(value, "monthlyFeeCredits")?;
    }
    if let Some(value) = payload.annual_fee_credits {
        validate_non_negative(value, "annualFeeCredits")?;
    }

    let updated = sqlx::query_as::<_, NumberPriceAdminRecord>(
        r#"
        update number_pricing_settings
        set country = coalesce($2, country),
            number_type = coalesce($3, number_type),
            setup_fee_credits = coalesce($4, setup_fee_credits),
            monthly_fee_credits = coalesce($5, monthly_fee_credits),
            annual_fee_credits = coalesce($6, annual_fee_credits),
            currency = coalesce($7, currency),
            is_active = coalesce($8, is_active),
            updated_at = now()
        where id = $1
        returning id,
                  country,
                  number_type,
                  setup_fee_credits::double precision as setup_fee_credits,
                  monthly_fee_credits::double precision as monthly_fee_credits,
                  annual_fee_credits::double precision as annual_fee_credits,
                  currency,
                  is_active
        "#,
    )
    .bind(id)
    .bind(clean_optional_text(payload.country))
    .bind(clean_optional_text(payload.number_type))
    .bind(payload.setup_fee_credits)
    .bind(payload.monthly_fee_credits)
    .bind(payload.annual_fee_credits)
    .bind(clean_optional_text(payload.currency))
    .bind(payload.is_active)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Number pricing record not found.".to_string()))?;

    Ok(Json(updated))
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn validate_positive(value: f64, field: &str) -> GatewayResult<()> {
    if value > 0.0 {
        Ok(())
    } else {
        Err(GatewayError::Upstream(format!(
            "{field} must be greater than zero"
        )))
    }
}

fn validate_non_negative(value: f64, field: &str) -> GatewayResult<()> {
    if value >= 0.0 {
        Ok(())
    } else {
        Err(GatewayError::Upstream(format!(
            "{field} cannot be negative"
        )))
    }
}

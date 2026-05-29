use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domains::admin_auth::AuthenticatedAdmin,
    error::{GatewayError, GatewayResult},
    state::AppState,
};

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
        .route("/admin/api/pricing/credit-policy", get(get_credit_policy))
        .route(
            "/admin/api/pricing/credit-policy",
            patch(update_credit_policy),
        )
        .route("/admin/api/pricing/promo-codes", get(list_promo_codes))
        .route("/admin/api/pricing/promo-codes", post(create_promo_code))
        .route(
            "/admin/api/pricing/promo-codes/{code}",
            patch(update_promo_code),
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
pub struct CreditPolicyAdminRecord {
    pub id: Uuid,
    pub policy_key: String,
    pub credits_per_usd: f64,
    pub default_discount_percentage: f64,
    pub pex_discount_percentage: f64,
    pub fiat_discount_percentage: f64,
    pub stablecoin_discount_percentage: f64,
    pub virtual_account_discount_percentage: f64,
    pub pex_price_usd: f64,
    pub pex_price_source: String,
    pub fiat_revenue_burn_percentage: f64,
    pub stablecoin_revenue_burn_percentage: f64,
    pub pex_immediate_burn_percentage: f64,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCreditPolicyRequest {
    pub credits_per_usd: Option<f64>,
    pub default_discount_percentage: Option<f64>,
    pub pex_discount_percentage: Option<f64>,
    pub fiat_discount_percentage: Option<f64>,
    pub stablecoin_discount_percentage: Option<f64>,
    pub virtual_account_discount_percentage: Option<f64>,
    pub pex_price_usd: Option<f64>,
    pub pex_price_source: Option<String>,
    pub fiat_revenue_burn_percentage: Option<f64>,
    pub stablecoin_revenue_burn_percentage: Option<f64>,
    pub pex_immediate_burn_percentage: Option<f64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PromoCodeAdminRecord {
    pub id: Uuid,
    pub code: String,
    pub description: Option<String>,
    pub discount_percentage: f64,
    pub max_uses: Option<i32>,
    pub used_count: i32,
    pub min_credit_amount: f64,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoCodesAdminResponse {
    pub promo_codes: Vec<PromoCodeAdminRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePromoCodeRequest {
    pub code: String,
    pub description: Option<String>,
    pub discount_percentage: f64,
    pub max_uses: Option<i32>,
    pub min_credit_amount: Option<f64>,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePromoCodeRequest {
    pub description: Option<String>,
    pub discount_percentage: Option<f64>,
    pub max_uses: Option<i32>,
    pub min_credit_amount: Option<f64>,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: Option<bool>,
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
    _admin: AuthenticatedAdmin,
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
    _admin: AuthenticatedAdmin,
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
    _admin: AuthenticatedAdmin,
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
    _admin: AuthenticatedAdmin,
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

async fn get_credit_policy(
    _admin: AuthenticatedAdmin,
    State(state): State<AppState>,
) -> GatewayResult<Json<CreditPolicyAdminRecord>> {
    let policy = fetch_credit_policy(&state).await?;
    Ok(Json(policy))
}

async fn update_credit_policy(
    _admin: AuthenticatedAdmin,
    State(state): State<AppState>,
    Json(payload): Json<UpdateCreditPolicyRequest>,
) -> GatewayResult<Json<CreditPolicyAdminRecord>> {
    if let Some(value) = payload.credits_per_usd {
        validate_positive(value, "creditsPerUsd")?;
    }
    validate_optional_percentage(
        payload.default_discount_percentage,
        "defaultDiscountPercentage",
    )?;
    validate_optional_percentage(payload.pex_discount_percentage, "pexDiscountPercentage")?;
    validate_optional_percentage(payload.fiat_discount_percentage, "fiatDiscountPercentage")?;
    validate_optional_percentage(
        payload.stablecoin_discount_percentage,
        "stablecoinDiscountPercentage",
    )?;
    validate_optional_percentage(
        payload.virtual_account_discount_percentage,
        "virtualAccountDiscountPercentage",
    )?;
    validate_optional_percentage(
        payload.fiat_revenue_burn_percentage,
        "fiatRevenueBurnPercentage",
    )?;
    validate_optional_percentage(
        payload.stablecoin_revenue_burn_percentage,
        "stablecoinRevenueBurnPercentage",
    )?;
    validate_optional_percentage(
        payload.pex_immediate_burn_percentage,
        "pexImmediateBurnPercentage",
    )?;
    if let Some(value) = payload.pex_price_usd {
        validate_positive(value, "pexPriceUsd")?;
    }

    let updated = sqlx::query_as::<_, CreditPolicyAdminRecord>(
        r#"
        update credit_pricing_policy
        set credits_per_usd = coalesce($1, credits_per_usd),
            default_discount_percentage = coalesce($2, default_discount_percentage),
            pex_discount_percentage = coalesce($3, pex_discount_percentage),
            fiat_discount_percentage = coalesce($4, fiat_discount_percentage),
            stablecoin_discount_percentage = coalesce($5, stablecoin_discount_percentage),
            virtual_account_discount_percentage = coalesce($6, virtual_account_discount_percentage),
            pex_price_usd = coalesce($7, pex_price_usd),
            pex_price_source = coalesce($8, pex_price_source),
            fiat_revenue_burn_percentage = coalesce($9, fiat_revenue_burn_percentage),
            stablecoin_revenue_burn_percentage = coalesce($10, stablecoin_revenue_burn_percentage),
            pex_immediate_burn_percentage = coalesce($11, pex_immediate_burn_percentage),
            is_active = coalesce($12, is_active),
            updated_at = now()
        where policy_key = 'default'
        returning id,
                  policy_key,
                  credits_per_usd::double precision as credits_per_usd,
                  default_discount_percentage::double precision as default_discount_percentage,
                  pex_discount_percentage::double precision as pex_discount_percentage,
                  fiat_discount_percentage::double precision as fiat_discount_percentage,
                  stablecoin_discount_percentage::double precision as stablecoin_discount_percentage,
                  virtual_account_discount_percentage::double precision as virtual_account_discount_percentage,
                  pex_price_usd::double precision as pex_price_usd,
                  pex_price_source,
                  fiat_revenue_burn_percentage::double precision as fiat_revenue_burn_percentage,
                  stablecoin_revenue_burn_percentage::double precision as stablecoin_revenue_burn_percentage,
                  pex_immediate_burn_percentage::double precision as pex_immediate_burn_percentage,
                  is_active
        "#,
    )
    .bind(payload.credits_per_usd)
    .bind(payload.default_discount_percentage)
    .bind(payload.pex_discount_percentage)
    .bind(payload.fiat_discount_percentage)
    .bind(payload.stablecoin_discount_percentage)
    .bind(payload.virtual_account_discount_percentage)
    .bind(payload.pex_price_usd)
    .bind(clean_optional_text(payload.pex_price_source))
    .bind(payload.fiat_revenue_burn_percentage)
    .bind(payload.stablecoin_revenue_burn_percentage)
    .bind(payload.pex_immediate_burn_percentage)
    .bind(payload.is_active)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Default credit policy record not found.".to_string()))?;

    Ok(Json(updated))
}

async fn fetch_credit_policy(state: &AppState) -> GatewayResult<CreditPolicyAdminRecord> {
    sqlx::query_as::<_, CreditPolicyAdminRecord>(
        r#"
        select id,
               policy_key,
               credits_per_usd::double precision as credits_per_usd,
               default_discount_percentage::double precision as default_discount_percentage,
               pex_discount_percentage::double precision as pex_discount_percentage,
               fiat_discount_percentage::double precision as fiat_discount_percentage,
               stablecoin_discount_percentage::double precision as stablecoin_discount_percentage,
               virtual_account_discount_percentage::double precision as virtual_account_discount_percentage,
               pex_price_usd::double precision as pex_price_usd,
               pex_price_source,
               fiat_revenue_burn_percentage::double precision as fiat_revenue_burn_percentage,
               stablecoin_revenue_burn_percentage::double precision as stablecoin_revenue_burn_percentage,
               pex_immediate_burn_percentage::double precision as pex_immediate_burn_percentage,
               is_active
        from credit_pricing_policy
        where policy_key = 'default'
        limit 1
        "#,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Default credit policy record not found.".to_string()))
}

async fn list_promo_codes(
    _admin: AuthenticatedAdmin,
    State(state): State<AppState>,
) -> GatewayResult<Json<PromoCodesAdminResponse>> {
    let promo_codes = sqlx::query_as::<_, PromoCodeAdminRecord>(
        r#"
        select id,
               code,
               description,
               discount_percentage::double precision as discount_percentage,
               max_uses,
               used_count,
               min_credit_amount::double precision as min_credit_amount,
               starts_at,
               expires_at,
               is_active
        from promo_codes
        order by created_at desc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(PromoCodesAdminResponse { promo_codes }))
}

async fn create_promo_code(
    _admin: AuthenticatedAdmin,
    State(state): State<AppState>,
    Json(payload): Json<CreatePromoCodeRequest>,
) -> GatewayResult<Json<PromoCodeAdminRecord>> {
    let code = normalize_code(&payload.code)?;
    validate_percentage(payload.discount_percentage, "discountPercentage")?;
    if let Some(max_uses) = payload.max_uses {
        validate_positive_i32(max_uses, "maxUses")?;
    }
    if let Some(min_credit_amount) = payload.min_credit_amount {
        validate_non_negative(min_credit_amount, "minCreditAmount")?;
    }
    validate_time_window(payload.starts_at, payload.expires_at)?;

    let created = sqlx::query_as::<_, PromoCodeAdminRecord>(
        r#"
        insert into promo_codes (
            code,
            description,
            discount_percentage,
            max_uses,
            min_credit_amount,
            starts_at,
            expires_at,
            is_active
        ) values ($1, $2, $3, $4, $5, $6, $7, coalesce($8, true))
        returning id,
                  code,
                  description,
                  discount_percentage::double precision as discount_percentage,
                  max_uses,
                  used_count,
                  min_credit_amount::double precision as min_credit_amount,
                  starts_at,
                  expires_at,
                  is_active
        "#,
    )
    .bind(code)
    .bind(clean_optional_text(payload.description))
    .bind(payload.discount_percentage)
    .bind(payload.max_uses)
    .bind(payload.min_credit_amount.unwrap_or(0.0))
    .bind(payload.starts_at)
    .bind(payload.expires_at)
    .bind(payload.is_active)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(created))
}

async fn update_promo_code(
    _admin: AuthenticatedAdmin,
    State(state): State<AppState>,
    Path(code): Path<String>,
    Json(payload): Json<UpdatePromoCodeRequest>,
) -> GatewayResult<Json<PromoCodeAdminRecord>> {
    let code = normalize_code(&code)?;
    validate_optional_percentage(payload.discount_percentage, "discountPercentage")?;
    if let Some(max_uses) = payload.max_uses {
        validate_positive_i32(max_uses, "maxUses")?;
    }
    if let Some(min_credit_amount) = payload.min_credit_amount {
        validate_non_negative(min_credit_amount, "minCreditAmount")?;
    }
    validate_time_window(payload.starts_at, payload.expires_at)?;

    let updated = sqlx::query_as::<_, PromoCodeAdminRecord>(
        r#"
        update promo_codes
        set description = coalesce($2, description),
            discount_percentage = coalesce($3, discount_percentage),
            max_uses = coalesce($4, max_uses),
            min_credit_amount = coalesce($5, min_credit_amount),
            starts_at = coalesce($6, starts_at),
            expires_at = coalesce($7, expires_at),
            is_active = coalesce($8, is_active),
            updated_at = now()
        where code = $1
        returning id,
                  code,
                  description,
                  discount_percentage::double precision as discount_percentage,
                  max_uses,
                  used_count,
                  min_credit_amount::double precision as min_credit_amount,
                  starts_at,
                  expires_at,
                  is_active
        "#,
    )
    .bind(code)
    .bind(clean_optional_text(payload.description))
    .bind(payload.discount_percentage)
    .bind(payload.max_uses)
    .bind(payload.min_credit_amount)
    .bind(payload.starts_at)
    .bind(payload.expires_at)
    .bind(payload.is_active)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Promo code not found.".to_string()))?;

    Ok(Json(updated))
}

async fn list_number_prices(
    _admin: AuthenticatedAdmin,
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
    _admin: AuthenticatedAdmin,
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

fn normalize_code(code: &str) -> GatewayResult<String> {
    let normalized = code.trim().to_uppercase();
    if normalized.is_empty() {
        return Err(GatewayError::Upstream(
            "promo code cannot be empty".to_string(),
        ));
    }
    Ok(normalized)
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

fn validate_positive_i32(value: i32, field: &str) -> GatewayResult<()> {
    if value > 0 {
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

fn validate_percentage(value: f64, field: &str) -> GatewayResult<()> {
    if (0.0..=100.0).contains(&value) {
        Ok(())
    } else {
        Err(GatewayError::Upstream(format!(
            "{field} must be between 0 and 100"
        )))
    }
}

fn validate_optional_percentage(value: Option<f64>, field: &str) -> GatewayResult<()> {
    if let Some(value) = value {
        validate_percentage(value, field)?;
    }
    Ok(())
}

fn validate_time_window(
    starts_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> GatewayResult<()> {
    if let (Some(start), Some(end)) = (starts_at, expires_at) {
        if end <= start {
            return Err(GatewayError::Upstream(
                "expiresAt must be after startsAt".to_string(),
            ));
        }
    }
    Ok(())
}

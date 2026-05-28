use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::{GatewayError, GatewayResult}, state::AppState};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreditFundingMethod {
    Pex,
    Card,
    Stablecoin,
    VirtualAccount,
}

impl CreditFundingMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pex => "pex",
            Self::Card => "card",
            Self::Stablecoin => "stablecoin",
            Self::VirtualAccount => "virtual_account",
        }
    }

    pub fn asset_code(self) -> &'static str {
        match self {
            Self::Pex => "PEX",
            Self::Card => "FIAT_USD",
            Self::Stablecoin => "USDC",
            Self::VirtualAccount => "FIAT_USD",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CreditPricingPolicyRecord {
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
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PromoCodeRecord {
    pub code: String,
    pub discount_percentage: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditQuote {
    pub quote_id: Uuid,
    pub quote_reference: String,
    pub funding_method: CreditFundingMethod,
    pub asset_code: String,
    pub requested_credits: f64,
    pub discount_percentage: f64,
    pub promo_code: Option<String>,
    pub final_credits: f64,
    pub usd_value: f64,
    pub pex_price_usd: Option<f64>,
    pub pex_required: f64,
    pub fiat_required: f64,
    pub burn_percentage: f64,
    pub burn_usd_value: f64,
    pub pex_price_source: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct BuildCreditQuoteInput {
    pub funding_method: CreditFundingMethod,
    pub requested_credits: f64,
    pub promo_code: Option<String>,
    pub idempotency_key: Option<String>,
}

pub async fn build_credit_quote(
    state: &AppState,
    input: BuildCreditQuoteInput,
) -> GatewayResult<CreditQuote> {
    let requested_credits = round_amount(input.requested_credits.max(0.0));
    if requested_credits <= 0.0 {
        return Err(GatewayError::Upstream("requested credits must be greater than zero".to_string()));
    }

    if let Some(key) = input.idempotency_key.as_deref() {
        if let Some(existing) = find_quote_by_idempotency_key(state, key).await? {
            return Ok(existing);
        }
    }

    let policy = get_active_credit_policy(state).await?;
    if policy.credits_per_usd <= 0.0 || policy.pex_price_usd <= 0.0 {
        return Err(GatewayError::Upstream("invalid credit pricing policy".to_string()));
    }

    let promo = match input.promo_code.as_deref() {
        Some(code) if !code.trim().is_empty() => get_valid_promo_code(state, code, requested_credits).await?,
        _ => None,
    };

    let method_discount = method_discount_percentage(&policy, input.funding_method);
    let promo_discount = promo.as_ref().map(|item| item.discount_percentage).unwrap_or(0.0);
    let discount_percentage = clamp_percentage(policy.default_discount_percentage + method_discount + promo_discount);
    let final_credits = round_amount(requested_credits * (1.0 - discount_percentage / 100.0));
    let usd_value = round_usd(final_credits / policy.credits_per_usd);

    let (pex_required, fiat_required, pex_price_usd, burn_percentage) = match input.funding_method {
        CreditFundingMethod::Pex => (
            round_amount(usd_value / policy.pex_price_usd),
            0.0,
            Some(policy.pex_price_usd),
            policy.pex_immediate_burn_percentage,
        ),
        CreditFundingMethod::Card | CreditFundingMethod::VirtualAccount => (
            0.0,
            usd_value,
            None,
            policy.fiat_revenue_burn_percentage,
        ),
        CreditFundingMethod::Stablecoin => (
            0.0,
            usd_value,
            None,
            policy.stablecoin_revenue_burn_percentage,
        ),
    };

    let burn_usd_value = round_usd(usd_value * (burn_percentage / 100.0));
    let quote_reference = format!("quote_{}", Uuid::new_v4().simple());

    let quote = sqlx::query_as::<_, CreditQuoteRow>(
        r#"
        insert into credit_purchase_quotes (
            quote_reference,
            funding_method,
            requested_credits,
            discount_percentage,
            promo_code,
            final_credits,
            usd_value,
            pex_price_usd,
            pex_required,
            fiat_required,
            burn_percentage,
            burn_usd_value,
            status,
            idempotency_key
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'quoted', $13)
        returning
            id,
            quote_reference,
            funding_method,
            requested_credits::float8 as requested_credits,
            discount_percentage::float8 as discount_percentage,
            promo_code,
            final_credits::float8 as final_credits,
            usd_value::float8 as usd_value,
            pex_price_usd::float8 as pex_price_usd,
            pex_required::float8 as pex_required,
            fiat_required::float8 as fiat_required,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            status
        "#,
    )
    .bind(&quote_reference)
    .bind(input.funding_method.as_str())
    .bind(requested_credits)
    .bind(discount_percentage)
    .bind(promo.as_ref().map(|item| item.code.clone()))
    .bind(final_credits)
    .bind(usd_value)
    .bind(pex_price_usd)
    .bind(pex_required)
    .bind(fiat_required)
    .bind(burn_percentage)
    .bind(burn_usd_value)
    .bind(input.idempotency_key)
    .fetch_one(&state.db)
    .await?;

    Ok(row_to_quote(quote, input.funding_method, input.funding_method.asset_code(), policy.pex_price_source))
}

pub async fn get_credit_quote_by_reference(
    state: &AppState,
    quote_reference: &str,
) -> GatewayResult<CreditQuote> {
    let row = sqlx::query_as::<_, CreditQuoteRow>(
        r#"
        select
            id,
            quote_reference,
            funding_method,
            requested_credits::float8 as requested_credits,
            discount_percentage::float8 as discount_percentage,
            promo_code,
            final_credits::float8 as final_credits,
            usd_value::float8 as usd_value,
            pex_price_usd::float8 as pex_price_usd,
            pex_required::float8 as pex_required,
            fiat_required::float8 as fiat_required,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            status
        from credit_purchase_quotes
        where quote_reference = $1
        limit 1
        "#,
    )
    .bind(quote_reference.trim())
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("credit quote not found".to_string()))?;

    let method = parse_funding_method(&row.funding_method)
        .ok_or_else(|| GatewayError::Upstream("stored quote has invalid funding method".to_string()))?;
    Ok(row_to_quote(row, method, method.asset_code(), "stored_quote".to_string()))
}

pub async fn mark_credit_quote_accepted(
    state: &AppState,
    quote_reference: &str,
) -> GatewayResult<()> {
    let result = sqlx::query(
        r#"
        update credit_purchase_quotes
        set status = 'accepted', updated_at = now()
        where quote_reference = $1 and status = 'quoted'
        "#,
    )
    .bind(quote_reference.trim())
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(GatewayError::Upstream(
            "credit quote is not available for acceptance or was already used".to_string(),
        ));
    }

    Ok(())
}

pub async fn mark_credit_quote_credited(
    state: &AppState,
    quote_reference: &str,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        update credit_purchase_quotes
        set status = 'credited', updated_at = now()
        where quote_reference = $1 and status in ('quoted', 'accepted')
        "#,
    )
    .bind(quote_reference.trim())
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn get_active_credit_policy(state: &AppState) -> GatewayResult<CreditPricingPolicyRecord> {
    sqlx::query_as::<_, CreditPricingPolicyRecord>(
        r#"
        select
            credits_per_usd::float8 as credits_per_usd,
            default_discount_percentage::float8 as default_discount_percentage,
            pex_discount_percentage::float8 as pex_discount_percentage,
            fiat_discount_percentage::float8 as fiat_discount_percentage,
            stablecoin_discount_percentage::float8 as stablecoin_discount_percentage,
            virtual_account_discount_percentage::float8 as virtual_account_discount_percentage,
            pex_price_usd::float8 as pex_price_usd,
            pex_price_source,
            fiat_revenue_burn_percentage::float8 as fiat_revenue_burn_percentage,
            stablecoin_revenue_burn_percentage::float8 as stablecoin_revenue_burn_percentage,
            pex_immediate_burn_percentage::float8 as pex_immediate_burn_percentage
        from credit_pricing_policy
        where policy_key = 'default' and is_active = true
        limit 1
        "#,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("active credit pricing policy not configured".to_string()))
}

async fn get_valid_promo_code(
    state: &AppState,
    code: &str,
    requested_credits: f64,
) -> GatewayResult<Option<PromoCodeRecord>> {
    let normalized = code.trim().to_uppercase();
    let promo = sqlx::query_as::<_, PromoCodeRecord>(
        r#"
        select code,
               discount_percentage::float8 as discount_percentage
        from promo_codes
        where upper(code) = $1
          and is_active = true
          and min_credit_amount <= $2
          and (starts_at is null or starts_at <= now())
          and (expires_at is null or expires_at >= now())
          and (max_uses is null or used_count < max_uses)
        limit 1
        "#,
    )
    .bind(normalized)
    .bind(requested_credits)
    .fetch_optional(&state.db)
    .await?;

    Ok(promo)
}

async fn find_quote_by_idempotency_key(state: &AppState, key: &str) -> GatewayResult<Option<CreditQuote>> {
    let row = sqlx::query_as::<_, CreditQuoteRow>(
        r#"
        select
            id,
            quote_reference,
            funding_method,
            requested_credits::float8 as requested_credits,
            discount_percentage::float8 as discount_percentage,
            promo_code,
            final_credits::float8 as final_credits,
            usd_value::float8 as usd_value,
            pex_price_usd::float8 as pex_price_usd,
            pex_required::float8 as pex_required,
            fiat_required::float8 as fiat_required,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            status
        from credit_purchase_quotes
        where idempotency_key = $1
        limit 1
        "#,
    )
    .bind(key)
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(|row| {
        let method = parse_funding_method(&row.funding_method).unwrap_or(CreditFundingMethod::Card);
        row_to_quote(row, method, method.asset_code(), "stored_quote".to_string())
    }))
}

#[derive(Debug, sqlx::FromRow)]
struct CreditQuoteRow {
    id: Uuid,
    quote_reference: String,
    funding_method: String,
    requested_credits: f64,
    discount_percentage: f64,
    promo_code: Option<String>,
    final_credits: f64,
    usd_value: f64,
    pex_price_usd: Option<f64>,
    pex_required: f64,
    fiat_required: f64,
    burn_percentage: f64,
    burn_usd_value: f64,
    status: String,
}

fn row_to_quote(row: CreditQuoteRow, method: CreditFundingMethod, asset_code: &str, pex_price_source: String) -> CreditQuote {
    CreditQuote {
        quote_id: row.id,
        quote_reference: row.quote_reference,
        funding_method: method,
        asset_code: asset_code.to_string(),
        requested_credits: row.requested_credits,
        discount_percentage: row.discount_percentage,
        promo_code: row.promo_code,
        final_credits: row.final_credits,
        usd_value: row.usd_value,
        pex_price_usd: row.pex_price_usd,
        pex_required: row.pex_required,
        fiat_required: row.fiat_required,
        burn_percentage: row.burn_percentage,
        burn_usd_value: row.burn_usd_value,
        pex_price_source,
        status: row.status,
    }
}

fn method_discount_percentage(policy: &CreditPricingPolicyRecord, method: CreditFundingMethod) -> f64 {
    match method {
        CreditFundingMethod::Pex => policy.pex_discount_percentage,
        CreditFundingMethod::Card => policy.fiat_discount_percentage,
        CreditFundingMethod::Stablecoin => policy.stablecoin_discount_percentage,
        CreditFundingMethod::VirtualAccount => policy.virtual_account_discount_percentage,
    }
}

fn parse_funding_method(value: &str) -> Option<CreditFundingMethod> {
    match value {
        "pex" => Some(CreditFundingMethod::Pex),
        "card" => Some(CreditFundingMethod::Card),
        "stablecoin" => Some(CreditFundingMethod::Stablecoin),
        "virtual_account" => Some(CreditFundingMethod::VirtualAccount),
        _ => None,
    }
}

fn clamp_percentage(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn round_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn round_usd(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

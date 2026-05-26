use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domains::auth::middleware::AuthenticatedAccount,
    error::{GatewayError, GatewayResult},
    infra::cache,
    state::AppState,
};

const NUMBER_SETUP_COST: f64 = 5.00;

#[derive(Debug, Deserialize)]
pub struct NumberSearchQuery {
    pub country_code: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct NumberBuyRequest {
    pub phone_number: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberReserveRequest {
    pub country: String,
    pub phone_number: String,
    pub plan: String,
    pub credit_balance: f64,
    pub number_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberReserveResponse {
    pub order_id: String,
    pub phone_number: String,
    pub country: String,
    pub plan: String,
    pub status: String,
    pub credit_cost: f64,
    pub setup_fee_credits: f64,
    pub monthly_fee_credits: f64,
    pub next_renewal_at: chrono::DateTime<chrono::Utc>,
    pub remaining_credits: f64,
    pub message: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NumberPricingRecord {
    pub country: String,
    pub number_type: String,
    pub setup_fee_credits: f64,
    pub monthly_fee_credits: f64,
    pub annual_fee_credits: f64,
    pub currency: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberPricingResponse {
    pub pricing: Vec<NumberPricingDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberPricingDto {
    pub country: String,
    pub number_type: String,
    pub setup_fee_credits: f64,
    pub monthly_fee_credits: f64,
    pub annual_fee_credits: f64,
    pub currency: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MyNumberRecord {
    pub id: Uuid,
    pub phone_number: String,
    pub country: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub setup_fee_credits: Option<f64>,
    pub monthly_fee_credits: Option<f64>,
    pub next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
    pub billing_status: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyNumberDto {
    pub id: Uuid,
    pub phone_number: String,
    pub country: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub setup_fee_credits: Option<f64>,
    pub monthly_fee_credits: Option<f64>,
    pub next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
    pub billing_status: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyNumbersResponse {
    pub numbers: Vec<MyNumberDto>,
}

#[derive(Debug, Serialize)]
pub struct ProvisioningResponse {
    pub order_id: String,
    pub phone_number: String,
    pub status: String,
    pub credits_deducted: f64,
    pub regulatory_note: Option<String>,
}

pub async fn search_global_numbers(
    State(state): State<AppState>,
    _account: AuthenticatedAccount,
    Query(query): Query<NumberSearchQuery>,
) -> GatewayResult<Json<Value>> {
    let country_code = normalize_country_code(&query.country_code)?;
    let limit = query.limit.unwrap_or(5).clamp(1, 25);

    let response = state
        .http
        .get(format!(
            "{}/v2/available_phone_numbers",
            state.config.telnyx_base_url
        ))
        .bearer_auth(&state.config.jwt_secret)
        .query(&[
            ("filter[country_code]", country_code.as_str()),
            ("filter[limit]", &limit.to_string()),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        return Err(GatewayError::Upstream(format!(
            "Telnyx number inventory search failed: {err_text}"
        )));
    }

    Ok(Json(response.json().await?))
}

pub async fn list_number_pricing(
    State(state): State<AppState>,
) -> GatewayResult<Json<NumberPricingResponse>> {
    let records = sqlx::query_as::<_, NumberPricingRecord>(
        r#"
        select country,
               number_type,
               setup_fee_credits::double precision as setup_fee_credits,
               monthly_fee_credits::double precision as monthly_fee_credits,
               annual_fee_credits::double precision as annual_fee_credits,
               currency
        from number_pricing_settings
        where is_active = true
        order by country asc, number_type asc
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(NumberPricingResponse {
        pricing: records.into_iter().map(NumberPricingDto::from).collect(),
    }))
}

pub async fn reserve_number_with_credits(
    State(state): State<AppState>,
    Json(payload): Json<NumberReserveRequest>,
) -> GatewayResult<Json<NumberReserveResponse>> {
    let phone_number = normalize_phone_number(&payload.phone_number)?;
    let number_type = payload
        .number_type
        .clone()
        .unwrap_or_else(|| "local".to_string());
    let pricing = get_number_pricing(&state, &payload.country, &number_type).await?;
    let setup_fee = pricing.setup_fee_credits;
    let monthly_fee = pricing.monthly_fee_credits;
    let annual_fee = pricing.annual_fee_credits;
    let plan = payload.plan.trim().to_string();
    let recurring_months = if plan.eq_ignore_ascii_case("annual") { 12 } else { 1 };
    let subscription_cost = if plan.eq_ignore_ascii_case("annual") {
        annual_fee
    } else {
        monthly_fee
    };
    let credit_cost = setup_fee + subscription_cost;
    let remaining_credits = payload.credit_balance - credit_cost;
    let confirmed = credit_cost > 0.0 && remaining_credits >= 0.0;
    let order_id = format!("num_order_{}", chrono::Utc::now().timestamp_millis());
    let next_renewal_at = chrono::Utc::now() + chrono::Duration::days(30 * recurring_months);

    if confirmed {
        save_reserved_number(
            &state,
            &phone_number,
            &order_id,
            &payload.country,
            &plan,
            "reserved",
            setup_fee,
            monthly_fee,
            next_renewal_at,
        )
        .await?;
    }

    Ok(Json(NumberReserveResponse {
        order_id,
        phone_number,
        country: payload.country,
        plan,
        status: if confirmed {
            "reserved".to_string()
        } else {
            "rejected".to_string()
        },
        credit_cost: if confirmed { credit_cost } else { 0.0 },
        setup_fee_credits: setup_fee,
        monthly_fee_credits: monthly_fee,
        next_renewal_at,
        remaining_credits,
        message: if confirmed {
            "Global number reservation accepted. The number is a recurring subscription and will renew monthly unless cancelled.".to_string()
        } else {
            "Global number reservation rejected. Insufficient Credits for setup and subscription.".to_string()
        },
    }))
}

pub async fn list_my_numbers(State(state): State<AppState>) -> GatewayResult<Json<MyNumbersResponse>> {
    let records = sqlx::query_as::<_, MyNumberRecord>(
        r#"
        select id,
               phone_number,
               country,
               plan,
               status,
               setup_fee_credits::double precision as setup_fee_credits,
               monthly_fee_credits::double precision as monthly_fee_credits,
               next_renewal_at,
               billing_status,
               created_at
        from provisioned_numbers
        order by created_at desc
        limit 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(MyNumbersResponse {
        numbers: records.into_iter().map(MyNumberDto::from).collect(),
    }))
}

pub async fn purchase_number(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<NumberBuyRequest>,
) -> GatewayResult<Json<ProvisioningResponse>> {
    let phone_number = normalize_phone_number(&payload.phone_number)?;
    let user_cache_key = format!("client:balance:{}", account.account_id);

    let current_credits = cache::get_credits(&state.cache, &user_cache_key).await?;
    match current_credits {
        Some(balance) if balance >= NUMBER_SETUP_COST => {
            cache::increment_credits(&state.cache, &user_cache_key, -NUMBER_SETUP_COST).await?;
        }
        _ => return Err(GatewayError::InsufficientCredits),
    }

    let order_payload = json!({
        "phone_numbers": [{ "phone_number": phone_number }]
    });

    let response = state
        .http
        .post(format!("{}/v2/number_orders", state.config.telnyx_base_url))
        .bearer_auth(&state.config.jwt_secret)
        .json(&order_payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        cache::increment_credits(&state.cache, &user_cache_key, NUMBER_SETUP_COST).await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx number provisioning failed: {err_text}"
        )));
    }

    let resp_json: Value = response.json().await?;
    let order_id = resp_json["data"]["id"]
        .as_str()
        .unwrap_or("unknown_order")
        .to_string();
    let status = resp_json["data"]["status"]
        .as_str()
        .unwrap_or("pending")
        .to_string();

    save_provisioned_number(
        &state,
        account.account_id,
        &phone_number,
        &order_id,
        &status,
    )
    .await?;

    Ok(Json(ProvisioningResponse {
        order_id,
        phone_number,
        regulatory_note: regulatory_note(&status),
        status,
        credits_deducted: NUMBER_SETUP_COST,
    }))
}

async fn get_number_pricing(
    state: &AppState,
    country: &str,
    number_type: &str,
) -> GatewayResult<NumberPricingRecord> {
    sqlx::query_as::<_, NumberPricingRecord>(
        r#"
        select country,
               number_type,
               setup_fee_credits::double precision as setup_fee_credits,
               monthly_fee_credits::double precision as monthly_fee_credits,
               annual_fee_credits::double precision as annual_fee_credits,
               currency
        from number_pricing_settings
        where country = $1 and number_type = $2 and is_active = true
        limit 1
        "#,
    )
    .bind(country)
    .bind(number_type)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        GatewayError::Upstream(format!(
            "No active number pricing configured for {country} / {number_type}"
        ))
    })
}

async fn save_reserved_number(
    state: &AppState,
    phone_number: &str,
    order_id: &str,
    country: &str,
    plan: &str,
    status: &str,
    setup_fee_credits: f64,
    monthly_fee_credits: f64,
    next_renewal_at: chrono::DateTime<chrono::Utc>,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        insert into provisioned_numbers (
            id,
            phone_number,
            telnyx_order_id,
            country,
            plan,
            status,
            setup_fee_credits,
            monthly_fee_credits,
            next_renewal_at,
            billing_status
        )
        values (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, 'active')
        on conflict (phone_number) do update
        set telnyx_order_id = excluded.telnyx_order_id,
            country = excluded.country,
            plan = excluded.plan,
            status = excluded.status,
            setup_fee_credits = excluded.setup_fee_credits,
            monthly_fee_credits = excluded.monthly_fee_credits,
            next_renewal_at = excluded.next_renewal_at,
            billing_status = excluded.billing_status,
            updated_at = now()
        "#,
    )
    .bind(phone_number)
    .bind(order_id)
    .bind(country)
    .bind(plan)
    .bind(status)
    .bind(setup_fee_credits)
    .bind(monthly_fee_credits)
    .bind(next_renewal_at)
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn save_provisioned_number(
    state: &AppState,
    account_id: Uuid,
    phone_number: &str,
    order_id: &str,
    status: &str,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        insert into provisioned_numbers (id, account_id, phone_number, telnyx_order_id, status)
        values (gen_random_uuid(), $1, $2, $3, $4)
        on conflict (phone_number) do update
        set account_id = excluded.account_id,
            telnyx_order_id = excluded.telnyx_order_id,
            status = excluded.status,
            updated_at = now()
        "#,
    )
    .bind(account_id)
    .bind(phone_number)
    .bind(order_id)
    .bind(status)
    .execute(&state.db)
    .await?;

    Ok(())
}

impl From<NumberPricingRecord> for NumberPricingDto {
    fn from(value: NumberPricingRecord) -> Self {
        Self {
            country: value.country,
            number_type: value.number_type,
            setup_fee_credits: value.setup_fee_credits,
            monthly_fee_credits: value.monthly_fee_credits,
            annual_fee_credits: value.annual_fee_credits,
            currency: value.currency,
        }
    }
}

impl From<MyNumberRecord> for MyNumberDto {
    fn from(value: MyNumberRecord) -> Self {
        Self {
            id: value.id,
            phone_number: value.phone_number,
            country: value.country,
            plan: value.plan,
            status: value.status,
            setup_fee_credits: value.setup_fee_credits,
            monthly_fee_credits: value.monthly_fee_credits,
            next_renewal_at: value.next_renewal_at,
            billing_status: value.billing_status,
            created_at: value.created_at,
        }
    }
}

fn normalize_country_code(country_code: &str) -> GatewayResult<String> {
    let country_code = country_code.trim().to_ascii_uppercase();
    if country_code.len() == 2 && country_code.chars().all(|ch| ch.is_ascii_alphabetic()) {
        Ok(country_code)
    } else {
        Err(GatewayError::Upstream(
            "country_code must be an ISO 3166-1 alpha-2 code like US, GB, or NG".to_string(),
        ))
    }
}

fn normalize_phone_number(phone_number: &str) -> GatewayResult<String> {
    let phone_number = phone_number.trim();
    let valid = phone_number.starts_with('+')
        && phone_number.len() <= 32
        && phone_number.chars().skip(1).all(|ch| ch.is_ascii_digit());

    if valid {
        Ok(phone_number.to_string())
    } else {
        Err(GatewayError::Upstream(
            "phone_number must be in E.164 format, for example +13125551234".to_string(),
        ))
    }
}

fn regulatory_note(status: &str) -> Option<String> {
    if status.eq_ignore_ascii_case("pending") {
        Some("This number may require local regulatory documents before activation.".to_string())
    } else {
        None
    }
}

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
    pub credit_amount: f64,
    pub credit_balance: f64,
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
    pub remaining_credits: f64,
    pub message: String,
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

pub async fn reserve_number_with_credits(
    Json(payload): Json<NumberReserveRequest>,
) -> GatewayResult<Json<NumberReserveResponse>> {
    let phone_number = normalize_phone_number(&payload.phone_number)?;
    let credit_cost = payload.credit_amount.max(0.0);
    let remaining_credits = payload.credit_balance - credit_cost;
    let confirmed = credit_cost > 0.0 && remaining_credits >= 0.0;

    Ok(Json(NumberReserveResponse {
        order_id: format!("num_order_{}", chrono::Utc::now().timestamp_millis()),
        phone_number,
        country: payload.country,
        plan: payload.plan,
        status: if confirmed {
            "reserved".to_string()
        } else {
            "rejected".to_string()
        },
        credit_cost: if confirmed { credit_cost } else { 0.0 },
        remaining_credits,
        message: if confirmed {
            "Global number reservation accepted. Credits can be deducted and provisioning can continue.".to_string()
        } else {
            "Global number reservation rejected. Insufficient Credits or invalid cost.".to_string()
        },
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
        set telnyx_order_id = excluded.telnyx_order_id,
            status = excluded.status
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

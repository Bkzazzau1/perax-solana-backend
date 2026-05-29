use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        telecom::billing::{
            credit_balance, debit_credits, log_provider_transaction, round_credits,
        },
    },
    error::{GatewayError, GatewayResult},
    providers::TelnyxClient,
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

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct NumberRenewalCandidate {
    id: Uuid,
    account_id: Option<Uuid>,
    phone_number: String,
    monthly_fee_credits: Option<f64>,
    next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberRenewalItem {
    pub id: Uuid,
    pub phone_number: String,
    pub status: String,
    pub monthly_fee_credits: f64,
    pub next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberRenewalResponse {
    pub processed: usize,
    pub renewed: usize,
    pub past_due: usize,
    pub items: Vec<NumberRenewalItem>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct NumberSubscriptionRecord {
    id: Uuid,
    account_id: Option<Uuid>,
    phone_number: String,
    monthly_fee_credits: Option<f64>,
    next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
    billing_status: Option<String>,
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberSubscriptionActionResponse {
    pub id: Uuid,
    pub phone_number: String,
    pub status: String,
    pub billing_status: String,
    pub monthly_fee_credits: Option<f64>,
    pub next_renewal_at: Option<chrono::DateTime<chrono::Utc>>,
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

    let response = TelnyxClient::new(&state)
        .search_available_numbers(&country_code, limit)
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
    account: AuthenticatedAccount,
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
    let recurring_months = if plan.eq_ignore_ascii_case("annual") {
        12
    } else {
        1
    };
    let subscription_cost = if plan.eq_ignore_ascii_case("annual") {
        annual_fee
    } else {
        monthly_fee
    };
    let credit_cost = round_credits(setup_fee + subscription_cost);
    let balance = credit_balance(&state, account.account_id).await?;
    let remaining_credits = round_credits(balance - credit_cost);
    let confirmed = credit_cost > 0.0 && remaining_credits >= 0.0;
    let order_id = format!("num_order_{}", chrono::Utc::now().timestamp_millis());
    let next_renewal_at = chrono::Utc::now() + chrono::Duration::days(30 * recurring_months);

    if confirmed {
        debit_credits(
            &state,
            account.account_id,
            credit_cost,
            "telnyx_number_reservation",
            &order_id,
            "Telnyx number reservation/subscription",
            serde_json::json!({
                "phoneNumber": phone_number,
                "country": payload.country,
                "plan": plan,
                "setupFeeCredits": setup_fee,
                "subscriptionCost": subscription_cost
            }),
        )
        .await?;
        save_reserved_number(
            &state,
            account.account_id,
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
            "Global number reservation rejected. Insufficient Credits for setup and subscription."
                .to_string()
        },
    }))
}

pub async fn list_my_numbers(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
) -> GatewayResult<Json<MyNumbersResponse>> {
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
        where account_id = $1
        order by created_at desc
        limit 100
        "#,
    )
    .bind(account.account_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(MyNumbersResponse {
        numbers: records.into_iter().map(MyNumberDto::from).collect(),
    }))
}

pub async fn cancel_number_subscription(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(number_id): Path<Uuid>,
) -> GatewayResult<Json<NumberSubscriptionActionResponse>> {
    let record = sqlx::query_as::<_, NumberSubscriptionRecord>(
        r#"
        update provisioned_numbers
        set billing_status = 'cancelled',
            status = 'cancelled',
            next_renewal_at = null,
            updated_at = now()
        where id = $1 and account_id = $2
        returning id,
                  account_id,
                  phone_number,
                  monthly_fee_credits::double precision as monthly_fee_credits,
                  next_renewal_at,
                  billing_status,
                  status
        "#,
    )
    .bind(number_id)
    .bind(account.account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Number subscription not found.".to_string()))?;

    Ok(Json(NumberSubscriptionActionResponse::from_record(
        record,
        "Number subscription cancelled. Renewal billing has stopped.".to_string(),
    )))
}

pub async fn reactivate_number_subscription(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(number_id): Path<Uuid>,
) -> GatewayResult<Json<NumberSubscriptionActionResponse>> {
    let record = get_user_number_subscription(&state, account.account_id, number_id).await?;
    let monthly_fee = record.monthly_fee_credits.unwrap_or(0.0);

    if monthly_fee <= 0.0 {
        return Err(GatewayError::Upstream(
            "Monthly subscription fee is not configured for this number.".to_string(),
        ));
    }

    debit_credits(
        &state,
        account.account_id,
        monthly_fee,
        "telnyx_number_reactivation",
        &format!("telnyx_number_reactivation:{number_id}"),
        "Telnyx number subscription reactivation",
        serde_json::json!({ "numberId": number_id, "phoneNumber": record.phone_number }),
    )
    .await?;

    let next_renewal_at = chrono::Utc::now() + chrono::Duration::days(30);
    let updated = sqlx::query_as::<_, NumberSubscriptionRecord>(
        r#"
        update provisioned_numbers
        set billing_status = 'active',
            status = 'reserved',
            next_renewal_at = $1,
            updated_at = now()
        where id = $2 and account_id = $3
        returning id,
                  account_id,
                  phone_number,
                  monthly_fee_credits::double precision as monthly_fee_credits,
                  next_renewal_at,
                  billing_status,
                  status
        "#,
    )
    .bind(next_renewal_at)
    .bind(number_id)
    .bind(account.account_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(NumberSubscriptionActionResponse::from_record(
        updated,
        format!(
            "Number subscription reactivated. {} Credits deducted for the next month.",
            monthly_fee
        ),
    )))
}

pub async fn process_due_number_renewals(
    State(state): State<AppState>,
) -> GatewayResult<Json<NumberRenewalResponse>> {
    Ok(Json(process_due_number_renewals_inner(&state).await?))
}

pub fn spawn_number_renewal_worker(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(err) = process_due_number_renewals_inner(&state).await {
                tracing::error!(error = %err, "Telnyx number renewal worker failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

async fn process_due_number_renewals_inner(
    state: &AppState,
) -> GatewayResult<NumberRenewalResponse> {
    let candidates = sqlx::query_as::<_, NumberRenewalCandidate>(
        r#"
        select id,
               account_id,
               phone_number,
               monthly_fee_credits::double precision as monthly_fee_credits,
               next_renewal_at
        from provisioned_numbers
        where billing_status = 'active'
          and next_renewal_at is not null
          and next_renewal_at <= now()
        order by next_renewal_at asc
        limit 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let mut items = Vec::with_capacity(candidates.len());
    let mut renewed = 0usize;
    let mut past_due = 0usize;

    for candidate in candidates {
        let monthly_fee = candidate.monthly_fee_credits.unwrap_or(0.0);

        if monthly_fee <= 0.0 {
            mark_number_past_due(state, candidate.id).await?;
            past_due += 1;
            items.push(NumberRenewalItem {
                id: candidate.id,
                phone_number: candidate.phone_number,
                status: "past_due".to_string(),
                monthly_fee_credits: monthly_fee,
                next_renewal_at: candidate.next_renewal_at,
                message: "No monthly fee is configured for this number.".to_string(),
            });
            continue;
        }

        let Some(account_id) = candidate.account_id else {
            mark_number_past_due(state, candidate.id).await?;
            past_due += 1;
            items.push(NumberRenewalItem {
                id: candidate.id,
                phone_number: candidate.phone_number,
                status: "past_due".to_string(),
                monthly_fee_credits: monthly_fee,
                next_renewal_at: candidate.next_renewal_at,
                message: "Number has no linked account for renewal billing.".to_string(),
            });
            continue;
        };

        match debit_credits(
            state,
            account_id,
            monthly_fee,
            "telnyx_number_renewal",
            &format!(
                "telnyx_number_renewal:{}:{}",
                candidate.id,
                chrono::Utc::now().date_naive()
            ),
            "Telnyx number monthly subscription renewal",
            serde_json::json!({ "numberId": candidate.id, "phoneNumber": candidate.phone_number }),
        )
        .await
        {
            Ok(_) => {
                let new_renewal_at = chrono::Utc::now() + chrono::Duration::days(30);
                sqlx::query(
                    r#"
                    update provisioned_numbers
                    set next_renewal_at = $1,
                        billing_status = 'active',
                        status = 'reserved',
                        updated_at = now()
                    where id = $2
                    "#,
                )
                .bind(new_renewal_at)
                .bind(candidate.id)
                .execute(&state.db)
                .await?;

                renewed += 1;
                items.push(NumberRenewalItem {
                    id: candidate.id,
                    phone_number: candidate.phone_number,
                    status: "renewed".to_string(),
                    monthly_fee_credits: monthly_fee,
                    next_renewal_at: Some(new_renewal_at),
                    message: "Monthly subscription renewed successfully.".to_string(),
                });
            }
            _ => {
                mark_number_past_due(state, candidate.id).await?;
                past_due += 1;
                items.push(NumberRenewalItem {
                    id: candidate.id,
                    phone_number: candidate.phone_number,
                    status: "past_due".to_string(),
                    monthly_fee_credits: monthly_fee,
                    next_renewal_at: candidate.next_renewal_at,
                    message: "Insufficient Credits for monthly renewal.".to_string(),
                });
            }
        }
    }

    Ok(NumberRenewalResponse {
        processed: items.len(),
        renewed,
        past_due,
        items,
    })
}

pub async fn purchase_number(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<NumberBuyRequest>,
) -> GatewayResult<Json<ProvisioningResponse>> {
    let phone_number = normalize_phone_number(&payload.phone_number)?;
    let source_reference = format!("telnyx_number_order:{}", Uuid::new_v4().simple());
    debit_credits(
        &state,
        account.account_id,
        NUMBER_SETUP_COST,
        "telnyx_number_order",
        &source_reference,
        "Telnyx number purchase",
        serde_json::json!({ "phoneNumber": phone_number }),
    )
    .await?;

    let response = TelnyxClient::new(&state)
        .order_number(&phone_number)
        .await?;
    let http_status = response.status();

    if !http_status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        log_provider_transaction(
            &state,
            "order_number",
            Some(account.account_id),
            "telnyx_number_order",
            &source_reference,
            None,
            None,
            Some(http_status.as_u16()),
            false,
            Some(&err_text),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx number provisioning failed: {err_text}"
        )));
    }

    let resp_json: Value = response.json().await?;
    log_provider_transaction(
        &state,
        "order_number",
        Some(account.account_id),
        "telnyx_number_order",
        &source_reference,
        None,
        Some(resp_json.clone()),
        Some(http_status.as_u16()),
        true,
        None,
    )
    .await?;
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
    account_id: Uuid,
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
            account_id,
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
        values (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, $9, 'active')
        on conflict (phone_number) do update
        set account_id = excluded.account_id,
            telnyx_order_id = excluded.telnyx_order_id,
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
    .bind(account_id)
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

async fn get_user_number_subscription(
    state: &AppState,
    account_id: Uuid,
    number_id: Uuid,
) -> GatewayResult<NumberSubscriptionRecord> {
    sqlx::query_as::<_, NumberSubscriptionRecord>(
        r#"
        select id,
               account_id,
               phone_number,
               monthly_fee_credits::double precision as monthly_fee_credits,
               next_renewal_at,
               billing_status,
               status
        from provisioned_numbers
        where id = $1 and account_id = $2
        limit 1
        "#,
    )
    .bind(number_id)
    .bind(account_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("Number subscription not found.".to_string()))
}

async fn mark_number_past_due(state: &AppState, number_id: Uuid) -> GatewayResult<()> {
    sqlx::query(
        r#"
        update provisioned_numbers
        set billing_status = 'past_due',
            status = 'past_due',
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(number_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

impl NumberSubscriptionActionResponse {
    fn from_record(record: NumberSubscriptionRecord, message: String) -> Self {
        Self {
            id: record.id,
            phone_number: record.phone_number,
            status: record.status,
            billing_status: record
                .billing_status
                .unwrap_or_else(|| "unknown".to_string()),
            monthly_fee_credits: record.monthly_fee_credits,
            next_renewal_at: record.next_renewal_at,
            message,
        }
    }
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

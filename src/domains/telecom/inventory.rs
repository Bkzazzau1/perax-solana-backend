use axum::{
    Json,
    extract::{Path, Query, State},
};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        telecom::billing::{
            TelnyxEconomics, credit_balance, credit_credits, debit_credits,
            estimate_telnyx_economics, log_provider_transaction,
            log_provider_transaction_with_economics, round_credits,
            telnyx_number_estimated_usd_cost,
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
    pub telnyx_phone_number_id: Option<String>,
    pub provider_status: Option<String>,
    pub messaging_profile_id: Option<String>,
    pub messaging_product: Option<String>,
    pub setup_fee_credits: Option<f64>,
    pub monthly_fee_credits: Option<f64>,
    pub estimated_usd_cost: Option<f64>,
    pub provider_cost_currency: Option<String>,
    pub provider_cost_source: Option<String>,
    pub margin_credits: Option<f64>,
    pub margin_usd: Option<f64>,
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
    pub telnyx_phone_number_id: Option<String>,
    pub provider_status: Option<String>,
    pub messaging_profile_id: Option<String>,
    pub messaging_product: Option<String>,
    pub setup_fee_credits: Option<f64>,
    pub monthly_fee_credits: Option<f64>,
    pub estimated_usd_cost: Option<f64>,
    pub provider_cost_currency: Option<String>,
    pub provider_cost_source: Option<String>,
    pub margin_credits: Option<f64>,
    pub margin_usd: Option<f64>,
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
    telnyx_phone_number_id: Option<String>,
    provider_status: Option<String>,
    messaging_profile_id: Option<String>,
    messaging_product: Option<String>,
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
    pub provider_status: Option<String>,
    pub messaging_profile_id: Option<String>,
    pub messaging_product: Option<String>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberProviderSyncResponse {
    pub id: Uuid,
    pub phone_number: String,
    pub telnyx_phone_number_id: Option<String>,
    pub provider_status: Option<String>,
    pub messaging_profile_id: Option<String>,
    pub messaging_product: Option<String>,
    pub provider_payload: Value,
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
    let economics = estimate_telnyx_economics(credit_cost, telnyx_number_estimated_usd_cost());
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
            &economics,
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
               telnyx_phone_number_id,
               provider_status,
               messaging_profile_id,
               messaging_product,
               setup_fee_credits::double precision as setup_fee_credits,
               monthly_fee_credits::double precision as monthly_fee_credits,
               estimated_usd_cost::double precision as estimated_usd_cost,
               provider_cost_currency,
               provider_cost_source,
               margin_credits::double precision as margin_credits,
               margin_usd::double precision as margin_usd,
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

pub async fn sync_number_provider_status(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(number_id): Path<Uuid>,
) -> GatewayResult<Json<NumberProviderSyncResponse>> {
    let record = get_user_number_subscription(&state, account.account_id, number_id).await?;
    let provider_identifier = telnyx_number_identifier(&record);
    let response = TelnyxClient::new(&state)
        .retrieve_number_messaging(&provider_identifier)
        .await?;
    let http_status = response.status();
    let provider_payload = response_json_or_status(response).await?;
    log_provider_transaction(
        &state,
        "retrieve_number_messaging",
        Some(account.account_id),
        "telnyx_number_sync",
        &format!("telnyx_number_sync:{number_id}"),
        Some(serde_json::json!({ "identifier": provider_identifier })),
        Some(provider_payload.clone()),
        Some(http_status.as_u16()),
        http_status.is_success(),
        if http_status.is_success() {
            None
        } else {
            Some("Telnyx number messaging status sync failed")
        },
    )
    .await?;

    if !http_status.is_success() {
        return Err(GatewayError::Upstream(format!(
            "Telnyx number status sync failed: {provider_payload}"
        )));
    }

    let telnyx_phone_number_id = provider_payload["data"]["id"].as_str().map(str::to_string);
    let provider_status = provider_payload["data"]["record_type"]
        .as_str()
        .map(str::to_string)
        .or_else(|| Some("synced".to_string()));
    let messaging_profile_id = provider_payload["data"]["messaging_profile_id"]
        .as_str()
        .map(str::to_string);
    let messaging_product = provider_payload["data"]["messaging_product"]
        .as_str()
        .map(str::to_string);

    sqlx::query(
        r#"
        update provisioned_numbers
        set telnyx_phone_number_id = coalesce($1, telnyx_phone_number_id),
            provider_status = $2,
            provider_payload = $3,
            messaging_profile_id = $4,
            messaging_product = $5,
            last_provider_sync_at = now(),
            updated_at = now()
        where id = $6 and account_id = $7
        "#,
    )
    .bind(&telnyx_phone_number_id)
    .bind(&provider_status)
    .bind(&provider_payload)
    .bind(&messaging_profile_id)
    .bind(&messaging_product)
    .bind(number_id)
    .bind(account.account_id)
    .execute(&state.db)
    .await?;

    Ok(Json(NumberProviderSyncResponse {
        id: number_id,
        phone_number: record.phone_number,
        telnyx_phone_number_id,
        provider_status,
        messaging_profile_id,
        messaging_product,
        provider_payload,
    }))
}

pub async fn cancel_number_subscription(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Path(number_id): Path<Uuid>,
) -> GatewayResult<Json<NumberSubscriptionActionResponse>> {
    let current = get_user_number_subscription(&state, account.account_id, number_id).await?;
    let provider_identifier = telnyx_number_identifier(&current);
    let client = TelnyxClient::new(&state);

    let unassign_response = client
        .update_number_messaging_profile(&provider_identifier, "")
        .await?;
    let unassign_status = unassign_response.status();
    let unassign_payload = response_json_or_status(unassign_response).await?;
    log_provider_transaction(
        &state,
        "unassign_number_messaging_profile",
        Some(account.account_id),
        "telnyx_number_cancel",
        &format!("telnyx_number_cancel_unassign:{number_id}"),
        Some(serde_json::json!({
            "identifier": provider_identifier,
            "messagingProfileId": ""
        })),
        Some(unassign_payload.clone()),
        Some(unassign_status.as_u16()),
        unassign_status.is_success(),
        if unassign_status.is_success() {
            None
        } else {
            Some("Telnyx messaging profile unassign failed")
        },
    )
    .await?;

    let release_response = client.release_phone_number(&provider_identifier).await?;
    let release_status = release_response.status();
    let release_payload = response_json_or_status(release_response).await?;
    log_provider_transaction(
        &state,
        "release_phone_number",
        Some(account.account_id),
        "telnyx_number_cancel",
        &format!("telnyx_number_cancel_release:{number_id}"),
        Some(serde_json::json!({ "identifier": provider_identifier })),
        Some(release_payload.clone()),
        Some(release_status.as_u16()),
        release_status.is_success(),
        if release_status.is_success() {
            None
        } else {
            Some("Telnyx number release failed")
        },
    )
    .await?;

    if !release_status.is_success() {
        return Err(GatewayError::Upstream(format!(
            "Telnyx number release failed: {release_payload}"
        )));
    }

    let record = sqlx::query_as::<_, NumberSubscriptionRecord>(
        r#"
        update provisioned_numbers
        set billing_status = 'cancelled',
            status = 'cancelled',
            provider_status = 'released',
            provider_payload = $3,
            messaging_profile_id = null,
            messaging_product = null,
            last_provider_sync_at = now(),
            cancelled_at = now(),
            next_renewal_at = null,
            updated_at = now()
        where id = $1 and account_id = $2
        returning id,
                  account_id,
                  phone_number,
                  telnyx_phone_number_id,
                  provider_status,
                  messaging_profile_id,
                  messaging_product,
                  monthly_fee_credits::double precision as monthly_fee_credits,
                  next_renewal_at,
                  billing_status,
                  status
        "#,
    )
    .bind(number_id)
    .bind(account.account_id)
    .bind(release_payload)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(NumberSubscriptionActionResponse::from_record(
        record,
        "Number subscription cancelled at Telnyx. Renewal billing has stopped.".to_string(),
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

    let source_reference = format!("telnyx_number_reactivation:{number_id}");
    debit_credits(
        &state,
        account.account_id,
        monthly_fee,
        "telnyx_number_reactivation",
        &source_reference,
        "Telnyx number subscription reactivation",
        serde_json::json!({ "numberId": number_id, "phoneNumber": record.phone_number }),
    )
    .await?;

    let client = TelnyxClient::new(&state);
    let order_response = client.order_number(&record.phone_number).await?;
    let order_status = order_response.status();
    let order_payload = response_json_or_status(order_response).await?;
    log_provider_transaction(
        &state,
        "reactivate_order_number",
        Some(account.account_id),
        "telnyx_number_reactivation",
        &source_reference,
        Some(serde_json::json!({ "phoneNumber": record.phone_number })),
        Some(order_payload.clone()),
        Some(order_status.as_u16()),
        order_status.is_success(),
        if order_status.is_success() {
            None
        } else {
            Some("Telnyx number reactivation order failed")
        },
    )
    .await?;

    if !order_status.is_success() {
        credit_credits(
            &state,
            account.account_id,
            monthly_fee,
            "telnyx_number_reactivation_reversal",
            &format!("{source_reference}:provider_rejected"),
            "Reversal for rejected Telnyx number reactivation",
            serde_json::json!({
                "originalSourceReference": source_reference,
                "phoneNumber": record.phone_number,
                "providerStatus": order_status.as_u16(),
                "providerError": order_payload
            }),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx number reactivation failed: {order_payload}"
        )));
    }

    let telnyx_order_id = order_payload["data"]["id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("reactivation_{number_id}"));
    let provider_status = order_payload["data"]["status"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| "pending".to_string());
    let telnyx_phone_number_id = extract_telnyx_phone_number_id(&order_payload);
    let mut provider_payload = order_payload.clone();
    let mut messaging_profile_id = None;
    let mut messaging_product = None;

    if !state.config.telnyx_messaging_profile_id.trim().is_empty() {
        let identifier = telnyx_phone_number_id
            .as_deref()
            .unwrap_or(record.phone_number.as_str());
        let assign_response = client
            .update_number_messaging_profile(identifier, &state.config.telnyx_messaging_profile_id)
            .await?;
        let assign_status = assign_response.status();
        let assign_payload = response_json_or_status(assign_response).await?;
        log_provider_transaction(
            &state,
            "assign_number_messaging_profile",
            Some(account.account_id),
            "telnyx_number_reactivation",
            &format!("{source_reference}:messaging_profile"),
            Some(serde_json::json!({
                "identifier": identifier,
                "messagingProfileId": state.config.telnyx_messaging_profile_id
            })),
            Some(assign_payload.clone()),
            Some(assign_status.as_u16()),
            assign_status.is_success(),
            if assign_status.is_success() {
                None
            } else {
                Some("Telnyx messaging profile assign failed")
            },
        )
        .await?;

        if assign_status.is_success() {
            messaging_profile_id = assign_payload["data"]["messaging_profile_id"]
                .as_str()
                .map(str::to_string);
            messaging_product = assign_payload["data"]["messaging_product"]
                .as_str()
                .map(str::to_string);
            provider_payload = assign_payload;
        }
    }

    let next_renewal_at = chrono::Utc::now() + chrono::Duration::days(30);
    let updated = sqlx::query_as::<_, NumberSubscriptionRecord>(
        r#"
        update provisioned_numbers
        set billing_status = 'active',
            status = 'reserved',
            telnyx_order_id = $1,
            telnyx_phone_number_id = coalesce($2, telnyx_phone_number_id),
            provider_status = $3,
            provider_payload = $4,
            messaging_profile_id = $5,
            messaging_product = $6,
            last_provider_sync_at = now(),
            cancelled_at = null,
            next_renewal_at = $7,
            updated_at = now()
        where id = $8 and account_id = $9
        returning id,
                  account_id,
                  phone_number,
                  telnyx_phone_number_id,
                  provider_status,
                  messaging_profile_id,
                  messaging_product,
                  monthly_fee_credits::double precision as monthly_fee_credits,
                  next_renewal_at,
                  billing_status,
                  status
        "#,
    )
    .bind(telnyx_order_id)
    .bind(telnyx_phone_number_id)
    .bind(provider_status)
    .bind(provider_payload)
    .bind(messaging_profile_id)
    .bind(messaging_product)
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
    let economics =
        estimate_telnyx_economics(NUMBER_SETUP_COST, telnyx_number_estimated_usd_cost());
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

    let client = TelnyxClient::new(&state);
    let response = client.order_number(&phone_number).await?;
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
        credit_credits(
            &state,
            account.account_id,
            NUMBER_SETUP_COST,
            "telnyx_number_order_reversal",
            &format!("{source_reference}:provider_rejected"),
            "Reversal for rejected Telnyx number order",
            serde_json::json!({
                "originalSourceReference": source_reference,
                "phoneNumber": phone_number,
                "providerStatus": http_status.as_u16(),
                "providerError": err_text.clone()
            }),
        )
        .await?;
        return Err(GatewayError::Upstream(format!(
            "Telnyx number provisioning failed: {err_text}"
        )));
    }

    let resp_json: Value = response.json().await?;
    log_provider_transaction_with_economics(
        &state,
        "telnyx",
        "order_number",
        Some(account.account_id),
        "telnyx_number_order",
        &source_reference,
        None,
        Some(resp_json.clone()),
        Some(http_status.as_u16()),
        true,
        None,
        Some(&economics),
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
    let telnyx_phone_number_id = extract_telnyx_phone_number_id(&resp_json);
    let mut provider_payload = resp_json.clone();
    let mut messaging_profile_id = None;
    let mut messaging_product = None;

    if !state.config.telnyx_messaging_profile_id.trim().is_empty() {
        let identifier = telnyx_phone_number_id
            .as_deref()
            .unwrap_or(phone_number.as_str());
        let assign_response = client
            .update_number_messaging_profile(identifier, &state.config.telnyx_messaging_profile_id)
            .await?;
        let assign_status = assign_response.status();
        let assign_payload = response_json_or_status(assign_response).await?;
        log_provider_transaction(
            &state,
            "assign_number_messaging_profile",
            Some(account.account_id),
            "telnyx_number_order",
            &format!("{source_reference}:messaging_profile"),
            Some(serde_json::json!({
                "identifier": identifier,
                "messagingProfileId": state.config.telnyx_messaging_profile_id
            })),
            Some(assign_payload.clone()),
            Some(assign_status.as_u16()),
            assign_status.is_success(),
            if assign_status.is_success() {
                None
            } else {
                Some("Telnyx messaging profile assign failed")
            },
        )
        .await?;

        if assign_status.is_success() {
            messaging_profile_id = assign_payload["data"]["messaging_profile_id"]
                .as_str()
                .map(str::to_string);
            messaging_product = assign_payload["data"]["messaging_product"]
                .as_str()
                .map(str::to_string);
            provider_payload = assign_payload;
        }
    }

    save_provisioned_number(
        &state,
        account.account_id,
        &phone_number,
        &order_id,
        &status,
        &economics,
        telnyx_phone_number_id.as_deref(),
        Some(&status),
        &provider_payload,
        messaging_profile_id.as_deref(),
        messaging_product.as_deref(),
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
    economics: &TelnyxEconomics,
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
            billing_status,
            estimated_usd_cost,
            provider_cost_currency,
            provider_cost_source,
            margin_credits,
            margin_usd
        )
        values (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, $9, 'active', $10, $11, $12, $13, $14)
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
            estimated_usd_cost = excluded.estimated_usd_cost,
            provider_cost_currency = excluded.provider_cost_currency,
            provider_cost_source = excluded.provider_cost_source,
            margin_credits = excluded.margin_credits,
            margin_usd = excluded.margin_usd,
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
    .bind(economics.estimated_usd_cost)
    .bind(&economics.provider_cost_currency)
    .bind(&economics.provider_cost_source)
    .bind(economics.margin_credits)
    .bind(economics.margin_usd)
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
    economics: &TelnyxEconomics,
    telnyx_phone_number_id: Option<&str>,
    provider_status: Option<&str>,
    provider_payload: &Value,
    messaging_profile_id: Option<&str>,
    messaging_product: Option<&str>,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        insert into provisioned_numbers (
            id,
            account_id,
            phone_number,
            telnyx_order_id,
            status,
            telnyx_phone_number_id,
            provider_status,
            provider_payload,
            messaging_profile_id,
            messaging_product,
            last_provider_sync_at,
            estimated_usd_cost,
            provider_cost_currency,
            provider_cost_source,
            margin_credits,
            margin_usd
        )
        values (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, $9, now(), $10, $11, $12, $13, $14)
        on conflict (phone_number) do update
        set account_id = excluded.account_id,
            telnyx_order_id = excluded.telnyx_order_id,
            status = excluded.status,
            telnyx_phone_number_id = coalesce(excluded.telnyx_phone_number_id, provisioned_numbers.telnyx_phone_number_id),
            provider_status = excluded.provider_status,
            provider_payload = excluded.provider_payload,
            messaging_profile_id = excluded.messaging_profile_id,
            messaging_product = excluded.messaging_product,
            last_provider_sync_at = excluded.last_provider_sync_at,
            estimated_usd_cost = excluded.estimated_usd_cost,
            provider_cost_currency = excluded.provider_cost_currency,
            provider_cost_source = excluded.provider_cost_source,
            margin_credits = excluded.margin_credits,
            margin_usd = excluded.margin_usd,
            updated_at = now()
        "#,
    )
    .bind(account_id)
    .bind(phone_number)
    .bind(order_id)
    .bind(status)
    .bind(telnyx_phone_number_id)
    .bind(provider_status)
    .bind(provider_payload)
    .bind(messaging_profile_id)
    .bind(messaging_product)
    .bind(economics.estimated_usd_cost)
    .bind(&economics.provider_cost_currency)
    .bind(&economics.provider_cost_source)
    .bind(economics.margin_credits)
    .bind(economics.margin_usd)
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
               telnyx_phone_number_id,
               provider_status,
               messaging_profile_id,
               messaging_product,
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

fn telnyx_number_identifier(record: &NumberSubscriptionRecord) -> String {
    record
        .telnyx_phone_number_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(record.phone_number.as_str())
        .to_string()
}

fn extract_telnyx_phone_number_id(payload: &Value) -> Option<String> {
    payload["data"]["phone_numbers"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .or_else(|| payload["data"]["phone_number_id"].as_str())
        .or_else(|| {
            let record_type = payload["data"]["record_type"].as_str();
            if record_type == Some("messaging_settings") {
                payload["data"]["id"].as_str()
            } else {
                None
            }
        })
        .map(str::to_string)
}

async fn response_json_or_status(response: Response) -> GatewayResult<Value> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(serde_json::json!({ "status": status.as_u16() }));
    }

    Ok(serde_json::from_str(&text).unwrap_or_else(|_| {
        serde_json::json!({
            "status": status.as_u16(),
            "body": text
        })
    }))
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
            provider_status: record.provider_status,
            messaging_profile_id: record.messaging_profile_id,
            messaging_product: record.messaging_product,
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
            telnyx_phone_number_id: value.telnyx_phone_number_id,
            provider_status: value.provider_status,
            messaging_profile_id: value.messaging_profile_id,
            messaging_product: value.messaging_product,
            setup_fee_credits: value.setup_fee_credits,
            monthly_fee_credits: value.monthly_fee_credits,
            estimated_usd_cost: value.estimated_usd_cost,
            provider_cost_currency: value.provider_cost_currency,
            provider_cost_source: value.provider_cost_source,
            margin_credits: value.margin_credits,
            margin_usd: value.margin_usd,
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

use axum::{
    Json, Router,
    extract::State,
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domains::{
        auth::middleware::AuthenticatedAccount,
        checkout::ledger::reserve_checkout_credits,
        pricing,
        telecom::billing::credit_balance,
    },
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutConfirmRequest {
    pub product_id: String,
    pub quantity: Option<u64>,
    pub beneficiary_wallet: Option<String>,
    pub idempotency_key: Option<String>,
    // Legacy client fields remain accepted for compatibility but are never trusted.
    pub product_name: Option<String>,
    pub credit_cost: Option<f64>,
    pub credit_balance: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutConfirmResponse {
    pub order_id: String,
    pub order_reference: String,
    pub status: String,
    pub product_id: String,
    pub product_name: String,
    pub quantity: u64,
    pub unit_credit_cost: f64,
    pub credit_cost: f64,
    pub remaining_credits: f64,
    pub settlement_id_hex: String,
    pub settlement_product_id_hex: String,
    pub settlement_status: String,
    pub message: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CheckoutOrderRow {
    id: Uuid,
    order_reference: String,
    service_code: String,
    service_name: String,
    quantity: i64,
    unit_credit_cost: f64,
    total_credit_cost: f64,
    settlement_id_hex: String,
    settlement_product_id_hex: String,
    order_status: String,
    settlement_status: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/checkout/confirm", post(confirm_checkout))
}

async fn confirm_checkout(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
    Json(payload): Json<CheckoutConfirmRequest>,
) -> GatewayResult<Json<CheckoutConfirmResponse>> {
    let service_code = payload.product_id.trim();
    if service_code.is_empty() {
        return Err(GatewayError::BadRequest(
            "productId is required".to_string(),
        ));
    }

    // Older clients may still send these fields. They are deliberately ignored.
    let _ = (
        payload.product_name.as_ref(),
        payload.credit_cost,
        payload.credit_balance,
    );

    let quantity = payload.quantity.unwrap_or(1);
    if quantity == 0 || quantity > 10_000 {
        return Err(GatewayError::BadRequest(
            "quantity must be between 1 and 10,000".to_string(),
        ));
    }

    let configured_price = pricing::get_utility_price(&state, service_code).await?;
    if !configured_price.credit_cost.is_finite() || configured_price.credit_cost <= 0.0 {
        return Err(GatewayError::Upstream(
            "active product price is invalid".to_string(),
        ));
    }

    let credits_per_usd = sqlx::query_scalar::<_, f64>(
        r#"
        select credits_per_usd::float8
        from credit_pricing_policy
        where policy_key = 'default' and is_active = true
        limit 1
        "#,
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("active credit pricing policy is missing".to_string()))?;
    if !credits_per_usd.is_finite() || credits_per_usd <= 0.0 {
        return Err(GatewayError::Upstream(
            "active credits-per-USD policy is invalid".to_string(),
        ));
    }

    let total_credit_cost = round_amount(configured_price.credit_cost * quantity as f64);
    let quote_value_usd = round_amount(total_credit_cost / credits_per_usd);
    if total_credit_cost <= 0.0 || quote_value_usd <= 0.0 {
        return Err(GatewayError::Upstream(
            "calculated checkout amount is invalid".to_string(),
        ));
    }

    let stored_wallet = sqlx::query_scalar::<_, Option<String>>(
        "select pex_wallet_address from accounts where id = $1 limit 1",
    )
    .bind(account.account_id)
    .fetch_optional(&state.db)
    .await?
    .flatten();
    let requested_wallet = payload
        .beneficiary_wallet
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let beneficiary_wallet = match (stored_wallet.as_deref(), requested_wallet) {
        (Some(stored), Some(requested)) if stored != requested => {
            return Err(GatewayError::BadRequest(
                "beneficiaryWallet must match the authenticated account wallet".to_string(),
            ));
        }
        (Some(stored), _) => Some(stored.to_string()),
        (None, Some(requested)) => Some(requested.to_string()),
        (None, None) => None,
    };

    let idempotency_key = payload
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let Some(key) = idempotency_key.as_deref() {
        if let Some(existing) = find_order_by_idempotency(&state, account.account_id, key).await? {
            let balance = credit_balance(&state, account.account_id).await?;
            return Ok(Json(order_response(existing, balance, true)));
        }
    }

    let order_reference = format!("checkout_{}", Uuid::new_v4().simple());
    let settlement_id_hex = random_32_byte_hex();
    let settlement_product_id_hex = sha256_hex(service_code.as_bytes());
    let inserted = sqlx::query_as::<_, CheckoutOrderRow>(
        r#"
        insert into checkout_settlement_orders (
            order_reference,
            idempotency_key,
            account_id,
            service_code,
            service_name,
            service_category,
            quantity,
            unit_credit_cost,
            total_credit_cost,
            credits_per_usd,
            quote_value_usd,
            settlement_id_hex,
            settlement_product_id_hex,
            beneficiary_wallet,
            order_status,
            settlement_status
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'created', 'pending')
        on conflict (account_id, idempotency_key) where idempotency_key is not null
        do nothing
        returning
            id,
            order_reference,
            service_code,
            service_name,
            quantity,
            unit_credit_cost::float8 as unit_credit_cost,
            total_credit_cost::float8 as total_credit_cost,
            settlement_id_hex,
            settlement_product_id_hex,
            order_status,
            settlement_status
        "#,
    )
    .bind(&order_reference)
    .bind(idempotency_key.as_deref())
    .bind(account.account_id)
    .bind(service_code)
    .bind(&configured_price.service_name)
    .bind(&configured_price.category)
    .bind(quantity as i64)
    .bind(configured_price.credit_cost)
    .bind(total_credit_cost)
    .bind(credits_per_usd)
    .bind(quote_value_usd)
    .bind(&settlement_id_hex)
    .bind(&settlement_product_id_hex)
    .bind(beneficiary_wallet.as_deref())
    .fetch_optional(&state.db)
    .await?;

    let order = match inserted {
        Some(order) => order,
        None => {
            let key = idempotency_key.as_deref().ok_or_else(|| {
                GatewayError::Upstream("checkout order could not be created".to_string())
            })?;
            let existing = find_order_by_idempotency(&state, account.account_id, key)
                .await?
                .ok_or_else(|| {
                    GatewayError::Upstream("idempotent checkout order is missing".to_string())
                })?;
            let balance = credit_balance(&state, account.account_id).await?;
            return Ok(Json(order_response(existing, balance, true)));
        }
    };

    let reservation = reserve_checkout_credits(
        &state,
        account.account_id,
        order.id,
        total_credit_cost,
        json!({
            "orderReference": order.order_reference,
            "serviceCode": order.service_code,
            "quantity": order.quantity,
            "unitCreditCost": order.unit_credit_cost,
            "totalCreditCost": order.total_credit_cost,
            "settlementIdHex": order.settlement_id_hex,
            "settlementProductIdHex": order.settlement_product_id_hex,
            "quoteValueUsd": quote_value_usd
        }),
    )
    .await?;
    let _ledger_id = reservation.ledger_id;
    let completed = find_order_by_id(&state, account.account_id, order.id)
        .await?
        .ok_or_else(|| GatewayError::Upstream("reserved checkout order is missing".to_string()))?;

    Ok(Json(order_response(
        completed,
        reservation.balance_after,
        reservation.already_reserved,
    )))
}

async fn find_order_by_idempotency(
    state: &AppState,
    account_id: Uuid,
    idempotency_key: &str,
) -> GatewayResult<Option<CheckoutOrderRow>> {
    sqlx::query_as::<_, CheckoutOrderRow>(
        r#"
        select
            id,
            order_reference,
            service_code,
            service_name,
            quantity,
            unit_credit_cost::float8 as unit_credit_cost,
            total_credit_cost::float8 as total_credit_cost,
            settlement_id_hex,
            settlement_product_id_hex,
            order_status,
            settlement_status
        from checkout_settlement_orders
        where account_id = $1 and idempotency_key = $2
        limit 1
        "#,
    )
    .bind(account_id)
    .bind(idempotency_key)
    .fetch_optional(&state.db)
    .await
    .map_err(Into::into)
}

async fn find_order_by_id(
    state: &AppState,
    account_id: Uuid,
    order_id: Uuid,
) -> GatewayResult<Option<CheckoutOrderRow>> {
    sqlx::query_as::<_, CheckoutOrderRow>(
        r#"
        select
            id,
            order_reference,
            service_code,
            service_name,
            quantity,
            unit_credit_cost::float8 as unit_credit_cost,
            total_credit_cost::float8 as total_credit_cost,
            settlement_id_hex,
            settlement_product_id_hex,
            order_status,
            settlement_status
        from checkout_settlement_orders
        where account_id = $1 and id = $2
        limit 1
        "#,
    )
    .bind(account_id)
    .bind(order_id)
    .fetch_optional(&state.db)
    .await
    .map_err(Into::into)
}

fn order_response(
    order: CheckoutOrderRow,
    remaining_credits: f64,
    idempotent_replay: bool,
) -> CheckoutConfirmResponse {
    CheckoutConfirmResponse {
        order_id: order.id.to_string(),
        order_reference: order.order_reference,
        status: order.order_status,
        product_id: order.service_code,
        product_name: order.service_name,
        quantity: order.quantity.max(0) as u64,
        unit_credit_cost: order.unit_credit_cost,
        credit_cost: order.total_credit_cost,
        remaining_credits,
        settlement_id_hex: order.settlement_id_hex,
        settlement_product_id_hex: order.settlement_product_id_hex,
        settlement_status: order.settlement_status,
        message: if idempotent_replay {
            "Existing checkout order returned without a second credit debit.".to_string()
        } else {
            "Checkout debited from the authenticated Credits ledger. Contract settlement is queued under the returned settlement ID.".to_string()
        },
    }
}

fn random_32_byte_hex() -> String {
    format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn round_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

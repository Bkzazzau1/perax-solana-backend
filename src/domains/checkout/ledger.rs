use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct CheckoutCreditReservation {
    pub ledger_id: Uuid,
    pub balance_after: f64,
    pub already_reserved: bool,
}

pub async fn reserve_checkout_credits(
    state: &AppState,
    account_id: Uuid,
    order_id: Uuid,
    amount: f64,
    metadata: Value,
) -> GatewayResult<CheckoutCreditReservation> {
    if !amount.is_finite() || amount <= 0.0 {
        return Err(GatewayError::BadRequest(
            "checkout credit amount must be greater than zero".to_string(),
        ));
    }
    let amount = round_amount(amount);
    let mut tx = state.db.begin().await?;

    // Serialize every balance-changing checkout for this account. The lock is released
    // automatically when the transaction commits or rolls back.
    sqlx::query_scalar::<_, i64>(
        "select pg_advisory_xact_lock(hashtextextended($1, 0))::text::bigint",
    )
    .bind(account_id.to_string())
    .fetch_optional(&mut *tx)
    .await?;

    let order = sqlx::query_as::<_, CheckoutOrderLock>(
        r#"
        select order_reference, order_status, credit_ledger_id
        from checkout_settlement_orders
        where id = $1 and account_id = $2
        for update
        "#,
    )
    .bind(order_id)
    .bind(account_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| GatewayError::BadRequest("checkout order was not found".to_string()))?;

    let current_balance = sqlx::query_scalar::<_, f64>(
        r#"
        select coalesce(sum(credit_delta), 0)::float8
        from credit_ledger
        where account_id = $1 and ledger_status = 'posted'
        "#,
    )
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;

    if order.order_status == "credits_reserved" {
        let ledger_id = order.credit_ledger_id.ok_or_else(|| {
            GatewayError::Upstream(
                "reserved checkout order is missing its credit ledger entry".to_string(),
            )
        })?;
        tx.commit().await?;
        return Ok(CheckoutCreditReservation {
            ledger_id,
            balance_after: round_amount(current_balance),
            already_reserved: true,
        });
    }
    if order.order_status == "cancelled" {
        return Err(GatewayError::BadRequest(
            "cancelled checkout order cannot be charged".to_string(),
        ));
    }

    if current_balance + 0.000001 < amount {
        sqlx::query(
            r#"
            update checkout_settlement_orders
            set order_status = 'failed',
                settlement_error = 'insufficient Credits',
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(order_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Err(GatewayError::InsufficientCredits);
    }

    let balance_after = round_amount(current_balance - amount);
    let ledger_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into credit_ledger (
            payment_intent_id,
            quote_reference,
            user_id,
            credits_granted,
            ledger_status,
            account_id,
            ledger_direction,
            credit_delta,
            balance_after,
            source,
            source_reference,
            description,
            metadata
        ) values (
            null,
            $1,
            $2,
            $3,
            'posted',
            $4,
            'debit',
            $5,
            $6,
            'checkout',
            $7,
            'Authoritative product checkout',
            $8
        )
        on conflict (source, source_reference)
            where source is not null and source_reference is not null
        do update set source_reference = excluded.source_reference
        returning id
        "#,
    )
    .bind(&order.order_reference)
    .bind(account_id.to_string())
    .bind(-amount)
    .bind(account_id)
    .bind(-amount)
    .bind(balance_after)
    .bind(&order.order_reference)
    .bind(metadata)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        update checkout_settlement_orders
        set credit_ledger_id = $2,
            order_status = 'credits_reserved',
            settlement_status = 'pending',
            settlement_error = null,
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(order_id)
    .bind(ledger_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(CheckoutCreditReservation {
        ledger_id,
        balance_after,
        already_reserved: false,
    })
}

#[derive(Debug, sqlx::FromRow)]
struct CheckoutOrderLock {
    order_reference: String,
    order_status: String,
    credit_ledger_id: Option<Uuid>,
}

fn round_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

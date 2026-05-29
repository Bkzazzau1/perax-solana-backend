use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct CreditDebit {
    pub ledger_id: Uuid,
    pub balance_after: f64,
}

pub async fn credit_balance(state: &AppState, account_id: Uuid) -> GatewayResult<f64> {
    let balance = sqlx::query_scalar::<_, f64>(
        r#"
        select coalesce(sum(credit_delta), 0)::float8
        from credit_ledger
        where account_id = $1
          and ledger_status = 'posted'
        "#,
    )
    .bind(account_id)
    .fetch_one(&state.db)
    .await?;

    Ok(round_credits(balance))
}

pub async fn debit_credits(
    state: &AppState,
    account_id: Uuid,
    amount: f64,
    source: &str,
    source_reference: &str,
    description: &str,
    metadata: Value,
) -> GatewayResult<CreditDebit> {
    if !amount.is_finite() || amount <= 0.0 {
        return Err(GatewayError::Upstream(
            "credit debit amount must be greater than zero".to_string(),
        ));
    }

    let amount = round_credits(amount);
    let mut tx = state.db.begin().await?;
    let current_balance = sqlx::query_scalar::<_, f64>(
        r#"
        select coalesce(sum(credit_delta), 0)::float8
        from credit_ledger
        where account_id = $1
          and ledger_status = 'posted'
        "#,
    )
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;

    if current_balance + 0.000001 < amount {
        return Err(GatewayError::InsufficientCredits);
    }

    let balance_after = round_credits(current_balance - amount);
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
            $7,
            $8,
            $9,
            $10
        )
        on conflict (source, source_reference) where source is not null and source_reference is not null
        do update set source_reference = excluded.source_reference
        returning id
        "#,
    )
    .bind(source_reference)
    .bind(account_id.to_string())
    .bind(-amount)
    .bind(account_id)
    .bind(-amount)
    .bind(balance_after)
    .bind(source)
    .bind(source_reference)
    .bind(description)
    .bind(metadata)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(CreditDebit {
        ledger_id,
        balance_after,
    })
}

pub async fn log_provider_transaction(
    state: &AppState,
    provider_action: &str,
    account_id: Option<Uuid>,
    source: &str,
    source_reference: &str,
    request_payload: Option<Value>,
    response_payload: Option<Value>,
    http_status: Option<u16>,
    success: bool,
    error_message: Option<&str>,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        insert into provider_transactions (
            provider,
            provider_action,
            account_id,
            source,
            source_reference,
            request_payload,
            response_payload,
            http_status,
            success,
            error_message
        ) values ('telnyx', $1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(provider_action)
    .bind(account_id)
    .bind(source)
    .bind(source_reference)
    .bind(request_payload)
    .bind(response_payload)
    .bind(http_status.map(i32::from))
    .bind(success)
    .bind(error_message)
    .execute(&state.db)
    .await?;

    Ok(())
}

pub fn round_credits(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

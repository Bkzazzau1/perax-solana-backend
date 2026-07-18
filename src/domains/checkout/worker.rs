use std::{env, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

const DEFAULT_INTERVAL_SECONDS: u64 = 30;
const CLAIM_TIMEOUT_SECONDS: i64 = 300;

pub fn spawn_checkout_settlement_worker(state: AppState) {
    let Some(executor_url) = settlement_executor_url() else {
        info!(
            "PERAX_SETTLEMENT_EXECUTOR_URL is not configured; checkout settlement worker is disabled"
        );
        return;
    };

    tokio::spawn(async move {
        let interval = settlement_worker_interval();
        info!(
            executor_url = %executor_url,
            interval_seconds = interval.as_secs(),
            "Starting checkout settlement worker"
        );

        loop {
            if let Err(err) = process_available_jobs(&state, &executor_url).await {
                error!(error = %err, "Checkout settlement worker iteration failed");
            }
            tokio::time::sleep(interval).await;
        }
    });
}

async fn process_available_jobs(state: &AppState, executor_url: &str) -> GatewayResult<()> {
    for _ in 0..10 {
        let Some(job) = claim_next_job(state).await? else {
            break;
        };

        match submit_job(state, executor_url, &job).await {
            Ok(result) => {
                if let Err(err) = reconcile_executor_result(state, &job, result).await {
                    warn!(
                        order_reference = %job.order_reference,
                        settlement_id_hex = %job.settlement_id_hex,
                        error = %err,
                        "Settlement result could not be reconciled; order remains retryable"
                    );
                    release_claim_for_retry(state, job.id, &err.to_string()).await?;
                }
            }
            Err(err) => {
                warn!(
                    order_reference = %job.order_reference,
                    settlement_id_hex = %job.settlement_id_hex,
                    error = %err,
                    "Settlement executor request failed; order remains retryable"
                );
                release_claim_for_retry(state, job.id, &err.to_string()).await?;
            }
        }
    }
    Ok(())
}

async fn claim_next_job(state: &AppState) -> GatewayResult<Option<CheckoutSettlementJob>> {
    let job = sqlx::query_as::<_, CheckoutSettlementJob>(
        r#"
        with candidate as (
            select id
            from checkout_settlement_orders
            where order_status = 'credits_reserved'
              and settlement_status in ('pending', 'planned', 'funding', 'ready')
              and (
                  settlement_claimed_at is null
                  or settlement_claimed_at < now() - make_interval(secs => $1)
              )
            order by created_at asc
            for update skip locked
            limit 1
        )
        update checkout_settlement_orders as orders
        set settlement_claimed_at = now(),
            settlement_last_attempt_at = now(),
            settlement_attempt_count = settlement_attempt_count + 1,
            updated_at = now()
        from candidate
        where orders.id = candidate.id
        returning
            orders.id,
            orders.order_reference,
            orders.account_id,
            orders.quantity,
            orders.total_credit_cost::float8 as total_credit_cost,
            orders.settlement_id_hex,
            orders.settlement_product_id_hex,
            orders.beneficiary_wallet,
            orders.settlement_funding_method,
            orders.settlement_status,
            orders.settlement_attempt_count
        "#,
    )
    .bind(CLAIM_TIMEOUT_SECONDS)
    .fetch_optional(&state.db)
    .await?;

    Ok(job)
}

async fn submit_job(
    state: &AppState,
    executor_url: &str,
    job: &CheckoutSettlementJob,
) -> GatewayResult<SettlementExecutorResponse> {
    let payload = SettlementExecutorRequest {
        solana_rpc_url: state.config.solana_rpc_url.clone(),
        program_id: state.config.perax_program_id.clone(),
        state_pda: state.config.perax_state_pda.clone(),
        pex_mint_address: state.config.pex_mint_address.clone(),
        order_reference: job.order_reference.clone(),
        settlement_id_hex: job.settlement_id_hex.clone(),
        product_id_hex: job.settlement_product_id_hex.clone(),
        funding_method: normalize_funding_method(&job.settlement_funding_method)?,
        quantity: u64::try_from(job.quantity).map_err(|_| {
            GatewayError::Config("checkout settlement quantity is invalid".to_string())
        })?,
        beneficiary_wallet: job.beneficiary_wallet.clone(),
        previous_status: job.settlement_status.clone(),
        attempt: job.settlement_attempt_count.max(0) as u32,
    };

    let mut request = state.http.post(executor_url).json(&payload);
    if let Ok(token) = env::var("PERAX_SETTLEMENT_EXECUTOR_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
    }

    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    let parsed = serde_json::from_str::<SettlementExecutorResponse>(&body).map_err(|err| {
        GatewayError::Upstream(format!(
            "settlement executor returned invalid JSON ({status}): {err}; body={body}"
        ))
    })?;

    if !status.is_success() && !parsed.terminal_failure {
        return Err(GatewayError::Upstream(format!(
            "settlement executor returned retryable HTTP {status}: {}",
            parsed.error.as_deref().unwrap_or("no error supplied")
        )));
    }
    Ok(parsed)
}

async fn reconcile_executor_result(
    state: &AppState,
    job: &CheckoutSettlementJob,
    result: SettlementExecutorResponse,
) -> GatewayResult<()> {
    let normalized_status = normalize_executor_status(&result.status)?;
    if result.terminal_failure && normalized_status != "failed" {
        return Err(GatewayError::Upstream(
            "terminalFailure may only accompany failed settlement status".to_string(),
        ));
    }

    if normalized_status == "failed" {
        if result.terminal_failure {
            refund_terminal_failure(
                state,
                job,
                result
                    .error
                    .as_deref()
                    .unwrap_or("terminal on-chain settlement failure"),
            )
            .await?;
            return Ok(());
        }

        release_claim_for_retry(
            state,
            job.id,
            result
                .error
                .as_deref()
                .unwrap_or("retryable settlement failure"),
        )
        .await?;
        return Ok(());
    }

    if normalized_status == "finalized" {
        let settlement_record_address = result
            .settlement_record_address
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                GatewayError::Upstream(
                    "finalized settlement response is missing record address".to_string(),
                )
            })?;
        let transaction_signature = result
            .transaction_signature
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                GatewayError::Upstream(
                    "finalized settlement response is missing transaction signature".to_string(),
                )
            })?;
        verify_finalized_on_chain(
            state,
            transaction_signature,
            settlement_record_address,
        )
        .await?;
    }

    sqlx::query(
        r#"
        update checkout_settlement_orders
        set settlement_status = $2,
            settlement_record_address = coalesce($3, settlement_record_address),
            settlement_transaction_signature = coalesce($4, settlement_transaction_signature),
            settlement_error = $5,
            settlement_claimed_at = null,
            settled_at = case when $2 = 'finalized' then now() else settled_at end,
            updated_at = now()
        where id = $1 and order_status = 'credits_reserved'
        "#,
    )
    .bind(job.id)
    .bind(normalized_status)
    .bind(result.settlement_record_address.as_deref())
    .bind(result.transaction_signature.as_deref())
    .bind(result.error.as_deref())
    .execute(&state.db)
    .await?;

    info!(
        order_reference = %job.order_reference,
        settlement_id_hex = %job.settlement_id_hex,
        settlement_status = normalized_status,
        "Checkout settlement executor result recorded"
    );
    Ok(())
}

async fn verify_finalized_on_chain(
    state: &AppState,
    transaction_signature: &str,
    settlement_record_address: &str,
) -> GatewayResult<()> {
    let signature_response = solana_rpc(
        state,
        "getSignatureStatuses",
        json!([[transaction_signature], { "searchTransactionHistory": true }]),
    )
    .await?;
    let signature_status = signature_response
        .pointer("/result/value/0")
        .filter(|value| !value.is_null())
        .ok_or_else(|| {
            GatewayError::Upstream(
                "settlement transaction signature is not visible on Solana".to_string(),
            )
        })?;
    if !signature_status.get("err").unwrap_or(&Value::Null).is_null() {
        return Err(GatewayError::Upstream(
            "settlement transaction failed on Solana".to_string(),
        ));
    }
    let confirmation = signature_status
        .get("confirmationStatus")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(confirmation, "confirmed" | "finalized") {
        return Err(GatewayError::Upstream(
            "settlement transaction is not yet confirmed".to_string(),
        ));
    }

    let account_response = solana_rpc(
        state,
        "getAccountInfo",
        json!([settlement_record_address, { "encoding": "base64", "commitment": "confirmed" }]),
    )
    .await?;
    let account = account_response
        .pointer("/result/value")
        .filter(|value| !value.is_null())
        .ok_or_else(|| {
            GatewayError::Upstream(
                "settlement record account is not visible on Solana".to_string(),
            )
        })?;
    let owner = account
        .get("owner")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if owner != state.config.perax_program_id {
        return Err(GatewayError::Upstream(
            "settlement record is not owned by the configured Pera-X program".to_string(),
        ));
    }
    Ok(())
}

async fn solana_rpc(state: &AppState, method: &str, params: Value) -> GatewayResult<Value> {
    let response = state
        .http
        .post(&state.config.solana_rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .send()
        .await?;
    let status = response.status();
    let body = response.json::<Value>().await?;
    if !status.is_success() || body.get("error").is_some() {
        return Err(GatewayError::Upstream(format!(
            "Solana RPC {method} verification failed: HTTP {status}; response={body}"
        )));
    }
    Ok(body)
}

async fn release_claim_for_retry(
    state: &AppState,
    order_id: Uuid,
    reason: &str,
) -> GatewayResult<()> {
    sqlx::query(
        r#"
        update checkout_settlement_orders
        set settlement_claimed_at = null,
            settlement_error = $2,
            updated_at = now()
        where id = $1 and settlement_status <> 'finalized'
        "#,
    )
    .bind(order_id)
    .bind(reason)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn refund_terminal_failure(
    state: &AppState,
    job: &CheckoutSettlementJob,
    reason: &str,
) -> GatewayResult<()> {
    let mut tx = state.db.begin().await?;
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(job.account_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;

    let locked = sqlx::query_as::<_, RefundOrderLock>(
        r#"
        select order_status, settlement_status, refund_ledger_id
        from checkout_settlement_orders
        where id = $1 and account_id = $2
        for update
        "#,
    )
    .bind(job.id)
    .bind(job.account_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| GatewayError::Upstream("checkout refund order is missing".to_string()))?;

    if locked.refund_ledger_id.is_some() {
        tx.commit().await?;
        return Ok(());
    }
    if locked.settlement_status == "finalized" {
        return Err(GatewayError::Upstream(
            "finalized settlement cannot be refunded automatically".to_string(),
        ));
    }
    if locked.order_status != "credits_reserved" {
        return Err(GatewayError::Upstream(
            "only a reserved checkout order can be refunded".to_string(),
        ));
    }

    let current_balance = sqlx::query_scalar::<_, f64>(
        r#"
        select coalesce(sum(credit_delta), 0)::float8
        from credit_ledger
        where account_id = $1 and ledger_status = 'posted'
        "#,
    )
    .bind(job.account_id)
    .fetch_one(&mut *tx)
    .await?;
    let balance_after = round_amount(current_balance + job.total_credit_cost);

    let refund_ledger_id = sqlx::query_scalar::<_, Uuid>(
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
            'credit',
            $5,
            $6,
            'checkout_refund',
            $7,
            'Refund for terminal checkout settlement failure',
            $8
        )
        on conflict (source, source_reference)
            where source is not null and source_reference is not null
        do update set source_reference = excluded.source_reference
        returning id
        "#,
    )
    .bind(&job.order_reference)
    .bind(job.account_id.to_string())
    .bind(job.total_credit_cost)
    .bind(job.account_id)
    .bind(job.total_credit_cost)
    .bind(balance_after)
    .bind(&job.order_reference)
    .bind(json!({
        "settlementIdHex": job.settlement_id_hex,
        "productIdHex": job.settlement_product_id_hex,
        "terminalFailure": true,
        "reason": reason
    }))
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        update checkout_settlement_orders
        set order_status = 'failed',
            settlement_status = 'failed',
            settlement_error = $2,
            settlement_claimed_at = null,
            refund_ledger_id = $3,
            refunded_at = now(),
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job.id)
    .bind(reason)
    .bind(refund_ledger_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    warn!(
        order_reference = %job.order_reference,
        settlement_id_hex = %job.settlement_id_hex,
        refund_ledger_id = %refund_ledger_id,
        "Terminal checkout settlement failed; Credits refunded idempotently"
    );
    Ok(())
}

fn settlement_executor_url() -> Option<String> {
    env::var("PERAX_SETTLEMENT_EXECUTOR_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.ends_with("/execute/settlement") {
                value
            } else {
                format!("{value}/execute/settlement")
            }
        })
}

fn settlement_worker_interval() -> Duration {
    env::var("PERAX_SETTLEMENT_INTERVAL_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_INTERVAL_SECONDS))
}

fn normalize_funding_method(value: &str) -> GatewayResult<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pex" => Ok("pex".to_string()),
        "stablecoin" => Ok("stablecoin".to_string()),
        "fiat" => Ok("fiat".to_string()),
        "virtual_account" | "virtualaccount" => Ok("virtualAccount".to_string()),
        _ => Err(GatewayError::Config(
            "checkout settlement funding method is invalid".to_string(),
        )),
    }
}

fn normalize_executor_status(value: &str) -> GatewayResult<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => Ok("pending"),
        "planned" => Ok("planned"),
        "funding" => Ok("funding"),
        "ready" => Ok("ready"),
        "finalized" => Ok("finalized"),
        "failed" => Ok("failed"),
        _ => Err(GatewayError::Upstream(
            "settlement executor returned an unsupported status".to_string(),
        )),
    }
}

fn round_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[derive(Debug, sqlx::FromRow)]
struct CheckoutSettlementJob {
    id: Uuid,
    order_reference: String,
    account_id: Uuid,
    quantity: i64,
    total_credit_cost: f64,
    settlement_id_hex: String,
    settlement_product_id_hex: String,
    beneficiary_wallet: Option<String>,
    settlement_funding_method: String,
    settlement_status: String,
    settlement_attempt_count: i32,
}

#[derive(Debug, sqlx::FromRow)]
struct RefundOrderLock {
    order_status: String,
    settlement_status: String,
    refund_ledger_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettlementExecutorRequest {
    solana_rpc_url: String,
    program_id: String,
    state_pda: String,
    pex_mint_address: String,
    order_reference: String,
    settlement_id_hex: String,
    product_id_hex: String,
    funding_method: String,
    quantity: u64,
    beneficiary_wallet: Option<String>,
    previous_status: String,
    attempt: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettlementExecutorResponse {
    status: String,
    #[serde(default)]
    terminal_failure: bool,
    settlement_record_address: Option<String>,
    transaction_signature: Option<String>,
    error: Option<String>,
}

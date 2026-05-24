use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityPaymentEvent {
    pub reference_hex: String,
    pub payer_wallet: Option<String>,
    pub token_mint: Option<String>,
    pub trading_company_token_account: String,
    pub amount: f64,
    pub source: String,
    pub service_code: Option<String>,
    pub tx_signature: Option<String>,
    pub raw_event: Option<Value>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct UtilityPaymentRecord {
    pub id: Uuid,
    pub reference_hex: String,
    pub payer_wallet: Option<String>,
    pub token_mint: Option<String>,
    pub trading_company_token_account: String,
    pub amount: f64,
    pub source: String,
    pub service_code: Option<String>,
    pub status: String,
    pub tx_signature: Option<String>,
}

pub async fn ingest_utility_payment_event(
    state: &AppState,
    event: UtilityPaymentEvent,
) -> GatewayResult<UtilityPaymentRecord> {
    let reference_bytes = decode_reference_hex(&event.reference_hex)?;

    let record = sqlx::query_as::<_, UtilityPaymentRecord>(
        r#"
        insert into utility_payments (
            reference,
            reference_hex,
            payer_wallet,
            token_mint,
            trading_company_token_account,
            amount,
            source,
            service_code,
            status,
            tx_signature,
            raw_event,
            confirmed_at
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, 'confirmed', $9, $10, now())
        on conflict (reference_hex) do update
        set
            payer_wallet = coalesce(excluded.payer_wallet, utility_payments.payer_wallet),
            token_mint = coalesce(excluded.token_mint, utility_payments.token_mint),
            trading_company_token_account = excluded.trading_company_token_account,
            amount = excluded.amount,
            source = excluded.source,
            service_code = coalesce(excluded.service_code, utility_payments.service_code),
            status = case
                when utility_payments.status = 'granted' then utility_payments.status
                else 'confirmed'
            end,
            tx_signature = coalesce(excluded.tx_signature, utility_payments.tx_signature),
            raw_event = coalesce(excluded.raw_event, utility_payments.raw_event),
            confirmed_at = coalesce(utility_payments.confirmed_at, now()),
            updated_at = now()
        returning
            id,
            reference_hex,
            payer_wallet,
            token_mint,
            trading_company_token_account,
            amount::float8 as amount,
            source,
            service_code,
            status,
            tx_signature
        "#,
    )
    .bind(reference_bytes)
    .bind(normalize_reference_hex(&event.reference_hex))
    .bind(event.payer_wallet)
    .bind(event.token_mint)
    .bind(event.trading_company_token_account)
    .bind(event.amount)
    .bind(event.source)
    .bind(event.service_code)
    .bind(event.tx_signature)
    .bind(event.raw_event)
    .fetch_one(&state.db)
    .await?;

    info!(
        payment_id = %record.id,
        reference = %record.reference_hex,
        amount = %record.amount,
        status = %record.status,
        "Utility payment event ingested and confirmed"
    );

    Ok(record)
}

pub async fn mark_utility_payment_granted(
    state: &AppState,
    reference_hex: &str,
) -> GatewayResult<Option<UtilityPaymentRecord>> {
    let record = sqlx::query_as::<_, UtilityPaymentRecord>(
        r#"
        update utility_payments
        set status = 'granted', granted_at = now(), updated_at = now()
        where reference_hex = $1 and status in ('detected', 'confirmed')
        returning
            id,
            reference_hex,
            payer_wallet,
            token_mint,
            trading_company_token_account,
            amount::float8 as amount,
            source,
            service_code,
            status,
            tx_signature
        "#,
    )
    .bind(normalize_reference_hex(reference_hex))
    .fetch_optional(&state.db)
    .await?;

    if record.is_none() {
        warn!(reference = %reference_hex, "No grantable utility payment found");
    }

    Ok(record)
}

fn normalize_reference_hex(reference_hex: &str) -> String {
    reference_hex.trim().trim_start_matches("0x").to_lowercase()
}

fn decode_reference_hex(reference_hex: &str) -> GatewayResult<Vec<u8>> {
    let normalized = normalize_reference_hex(reference_hex);

    if normalized.len() != 64 {
        return Err(GatewayError::Upstream(
            "payment reference must be 32 bytes / 64 hex characters".to_string(),
        ));
    }

    let mut bytes = Vec::with_capacity(32);
    for chunk in normalized.as_bytes().chunks(2) {
        let hex_pair = std::str::from_utf8(chunk).map_err(|_| {
            GatewayError::Upstream("payment reference is not valid hex".to_string())
        })?;
        let byte = u8::from_str_radix(hex_pair, 16).map_err(|_| {
            GatewayError::Upstream("payment reference is not valid hex".to_string())
        })?;
        bytes.push(byte);
    }

    Ok(bytes)
}

use std::env;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    domains::{
        credits::pricing_engine::{
            CreditFundingMethod, CreditQuote, get_credit_quote_by_reference,
        },
        solana::revenue_ledger::{RecordPexRevenueInput, record_pex_revenue_event},
    },
    error::{GatewayError, GatewayResult},
    state::AppState,
};

type HmacSha256 = Hmac<Sha256>;

const PAYMENT_RECORD_SPACE: u64 = 8 + 32 + 32 + 8 + 32 + 32 + 32 + 8 + 1;
const PAYMENT_RECORD_REFERENCE_OFFSET: usize = 8;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/payments/intents", post(create_payment_intent))
        .route(
            "/payments/intents/{intent_reference}",
            get(get_payment_intent),
        )
        .route("/payments/verify/pex", post(verify_pex_payment))
        .route("/payments/verify/provider", post(verify_provider_payment))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentIntentRequest {
    pub quote_reference: String,
    pub user_id: Option<String>,
    pub payer_wallet: Option<String>,
    pub provider: Option<String>,
    pub provider_reference: Option<String>,
    pub reference_hex: Option<String>,
    pub idempotency_key: Option<String>,
}

pub async fn create_intent_for_quote(
    state: &AppState,
    quote: &CreditQuote,
    request: CreatePaymentIntentRequest,
) -> GatewayResult<PaymentIntentRecord> {
    upsert_payment_intent(state, quote, request).await
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntentRecord {
    pub id: Uuid,
    pub intent_reference: String,
    pub quote_reference: String,
    pub funding_method: String,
    pub user_id: Option<String>,
    pub payer_wallet: Option<String>,
    pub provider: Option<String>,
    pub provider_reference: Option<String>,
    pub expected_asset_code: String,
    pub expected_amount: f64,
    pub expected_usd_value: f64,
    pub expected_credits: f64,
    pub pex_price_usd: Option<f64>,
    pub burn_percentage: f64,
    pub burn_usd_value: f64,
    pub reference_hex: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentIntentResponse {
    pub intent: PaymentIntentRecord,
    pub quote: Option<CreditQuote>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPexPaymentRequest {
    pub intent_reference: String,
    pub reference_hex: String,
    pub payer_wallet: Option<String>,
    pub tx_signature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProviderPaymentRequest {
    pub intent_reference: String,
    pub provider: String,
    pub provider_reference: String,
    pub amount_paid: f64,
    pub currency: String,
    pub status: String,
    pub signature: Option<String>,
    pub raw_confirmation: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResponse {
    pub accepted: bool,
    pub intent: PaymentIntentRecord,
    pub confirmation_id: Option<Uuid>,
    pub credit_ledger_id: Option<Uuid>,
    pub revenue_ledger_id: Option<Uuid>,
    pub burn_liability_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone)]
struct PaymentRecordAccount {
    reference_hex: String,
    payer_wallet: String,
    amount_base_units: u64,
    token_mint: String,
    trading_company_token_account: String,
    trading_company_revenue_token_account: String,
    created_at: i64,
}

async fn create_payment_intent(
    State(state): State<AppState>,
    Json(request): Json<CreatePaymentIntentRequest>,
) -> GatewayResult<Json<PaymentIntentResponse>> {
    let quote = get_credit_quote_by_reference(&state, &request.quote_reference).await?;
    let intent = upsert_payment_intent(&state, &quote, request).await?;

    Ok(Json(PaymentIntentResponse {
        intent,
        quote: Some(quote),
    }))
}

async fn get_payment_intent(
    State(state): State<AppState>,
    Path(intent_reference): Path<String>,
) -> GatewayResult<Json<PaymentIntentResponse>> {
    let intent = find_payment_intent(&state, &intent_reference).await?;
    Ok(Json(PaymentIntentResponse {
        intent,
        quote: None,
    }))
}

async fn verify_pex_payment(
    State(state): State<AppState>,
    Json(request): Json<VerifyPexPaymentRequest>,
) -> GatewayResult<Json<VerificationResponse>> {
    let reference_hex = normalize_hex_32(&request.reference_hex)?;
    let mut intent = find_payment_intent(&state, &request.intent_reference).await?;
    ensure_intent_pending(&intent)?;
    ensure_method(&intent, CreditFundingMethod::Pex)?;

    let payment_record = fetch_payment_record(&state, &reference_hex).await?;
    let expected_amount = pex_to_base_units(intent.expected_amount)?;

    if payment_record.reference_hex != reference_hex {
        return fail_verification("PaymentRecord reference does not match request");
    }
    if payment_record.amount_base_units != expected_amount {
        return fail_verification("PaymentRecord amount does not match quote");
    }
    if payment_record.token_mint != state.config.pex_mint_address {
        return fail_verification("PaymentRecord token mint does not match configured PEX mint");
    }
    if payment_record.trading_company_token_account != state.config.trading_co_treasury {
        return fail_verification(
            "PaymentRecord locked Trading Company account does not match config",
        );
    }
    if payment_record.trading_company_revenue_token_account
        != state.config.trading_company_second_wallet
    {
        return fail_verification("PaymentRecord revenue account does not match config");
    }
    if let Some(expected_payer) = request
        .payer_wallet
        .as_deref()
        .or(intent.payer_wallet.as_deref())
    {
        if !expected_payer.trim().is_empty() && payment_record.payer_wallet != expected_payer.trim()
        {
            return fail_verification("PaymentRecord payer wallet does not match expected payer");
        }
    }
    if payment_record.created_at <= 0 {
        return fail_verification("PaymentRecord created_at is invalid");
    }

    let quote = get_credit_quote_by_reference(&state, &intent.quote_reference).await?;
    let expected_amount = intent.expected_amount;
    let posted = post_verified_payment(
        &state,
        &mut intent,
        VerifiedPaymentInput {
            method: CreditFundingMethod::Pex,
            verification_source: "solana_payment_record".to_string(),
            provider: None,
            provider_reference: None,
            reference_hex: Some(reference_hex),
            tx_signature: request.tx_signature,
            payer_wallet: Some(payment_record.payer_wallet),
            token_mint: Some(payment_record.token_mint),
            trading_company_token_account: Some(payment_record.trading_company_token_account),
            trading_company_revenue_token_account: Some(
                payment_record.trading_company_revenue_token_account,
            ),
            amount_paid: expected_amount,
            currency: "PEX".to_string(),
            raw_confirmation: Some(json!({
                "paymentRecordCreatedAt": payment_record.created_at,
                "amountBaseUnits": payment_record.amount_base_units
            })),
        },
        &quote,
    )
    .await?;

    Ok(Json(posted))
}

async fn verify_provider_payment(
    State(state): State<AppState>,
    Json(request): Json<VerifyProviderPaymentRequest>,
) -> GatewayResult<Json<VerificationResponse>> {
    let mut intent = find_payment_intent(&state, &request.intent_reference).await?;
    ensure_intent_pending(&intent)?;
    if matches!(intent.funding_method.as_str(), "pex") {
        return Err(GatewayError::Upstream(
            "PEX payments must use /payments/verify/pex".to_string(),
        ));
    }

    let provider = clean_required(&request.provider, "provider")?;
    let provider_reference = clean_required(&request.provider_reference, "providerReference")?;
    let currency = clean_required(&request.currency, "currency")?.to_uppercase();
    let status = request.status.trim().to_lowercase();

    if !matches!(
        status.as_str(),
        "successful" | "succeeded" | "paid" | "confirmed"
    ) {
        return fail_verification("provider payment status is not successful");
    }
    if !amount_matches(request.amount_paid, intent.expected_amount) {
        return fail_verification("provider amount does not match quote");
    }
    if currency != intent.expected_asset_code {
        return fail_verification("provider currency does not match quote asset");
    }
    verify_provider_signature(
        &state,
        &provider,
        &provider_reference,
        request.amount_paid,
        &currency,
        &status,
        request.signature.as_deref(),
    )?;

    let payer_wallet = intent.payer_wallet.clone();
    let quote = get_credit_quote_by_reference(&state, &intent.quote_reference).await?;
    let posted = post_verified_payment(
        &state,
        &mut intent,
        VerifiedPaymentInput {
            method: quote.funding_method,
            verification_source: "provider_attestation".to_string(),
            provider: Some(provider),
            provider_reference: Some(provider_reference),
            reference_hex: None,
            tx_signature: None,
            payer_wallet,
            token_mint: None,
            trading_company_token_account: None,
            trading_company_revenue_token_account: None,
            amount_paid: request.amount_paid,
            currency,
            raw_confirmation: request.raw_confirmation,
        },
        &quote,
    )
    .await?;

    Ok(Json(posted))
}

async fn upsert_payment_intent(
    state: &AppState,
    quote: &CreditQuote,
    request: CreatePaymentIntentRequest,
) -> GatewayResult<PaymentIntentRecord> {
    if let Some(key) = request.idempotency_key.as_deref() {
        if let Some(existing) = find_intent_by_idempotency_key(state, key).await? {
            return Ok(existing);
        }
    }

    let expected_amount = if matches!(quote.funding_method, CreditFundingMethod::Pex) {
        quote.pex_required
    } else {
        quote.fiat_required
    };

    let reference_hex = request
        .reference_hex
        .as_deref()
        .map(normalize_hex_32)
        .transpose()?;

    let intent_reference = format!("pi_{}", Uuid::new_v4().simple());
    let intent = sqlx::query_as::<_, PaymentIntentRecord>(
        r#"
        insert into payment_intents (
            intent_reference,
            quote_reference,
            funding_method,
            user_id,
            payer_wallet,
            provider,
            provider_reference,
            expected_asset_code,
            expected_amount,
            expected_usd_value,
            expected_credits,
            pex_price_usd,
            burn_percentage,
            burn_usd_value,
            reference_hex,
            idempotency_key
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
        returning
            id,
            intent_reference,
            quote_reference,
            funding_method,
            user_id,
            payer_wallet,
            provider,
            provider_reference,
            expected_asset_code,
            expected_amount::float8 as expected_amount,
            expected_usd_value::float8 as expected_usd_value,
            expected_credits::float8 as expected_credits,
            pex_price_usd::float8 as pex_price_usd,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            reference_hex,
            status
        "#,
    )
    .bind(intent_reference)
    .bind(&quote.quote_reference)
    .bind(quote.funding_method.as_str())
    .bind(clean_optional_text(request.user_id))
    .bind(clean_optional_text(request.payer_wallet))
    .bind(clean_optional_text(request.provider))
    .bind(clean_optional_text(request.provider_reference))
    .bind(&quote.asset_code)
    .bind(expected_amount)
    .bind(quote.usd_value)
    .bind(quote.final_credits)
    .bind(quote.pex_price_usd)
    .bind(quote.burn_percentage)
    .bind(quote.burn_usd_value)
    .bind(reference_hex)
    .bind(clean_optional_text(request.idempotency_key))
    .fetch_one(&state.db)
    .await?;

    Ok(intent)
}

async fn find_payment_intent(
    state: &AppState,
    intent_reference: &str,
) -> GatewayResult<PaymentIntentRecord> {
    sqlx::query_as::<_, PaymentIntentRecord>(
        intent_select_sql("where intent_reference = $1").as_str(),
    )
    .bind(intent_reference.trim())
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| GatewayError::Upstream("payment intent not found".to_string()))
}

async fn find_intent_by_idempotency_key(
    state: &AppState,
    key: &str,
) -> GatewayResult<Option<PaymentIntentRecord>> {
    sqlx::query_as::<_, PaymentIntentRecord>(
        intent_select_sql("where idempotency_key = $1").as_str(),
    )
    .bind(key.trim())
    .fetch_optional(&state.db)
    .await
    .map_err(Into::into)
}

fn intent_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
            id,
            intent_reference,
            quote_reference,
            funding_method,
            user_id,
            payer_wallet,
            provider,
            provider_reference,
            expected_asset_code,
            expected_amount::float8 as expected_amount,
            expected_usd_value::float8 as expected_usd_value,
            expected_credits::float8 as expected_credits,
            pex_price_usd::float8 as pex_price_usd,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            reference_hex,
            status
        from payment_intents
        {where_clause}
        limit 1
        "#
    )
}

struct VerifiedPaymentInput {
    method: CreditFundingMethod,
    verification_source: String,
    provider: Option<String>,
    provider_reference: Option<String>,
    reference_hex: Option<String>,
    tx_signature: Option<String>,
    payer_wallet: Option<String>,
    token_mint: Option<String>,
    trading_company_token_account: Option<String>,
    trading_company_revenue_token_account: Option<String>,
    amount_paid: f64,
    currency: String,
    raw_confirmation: Option<Value>,
}

async fn post_verified_payment(
    state: &AppState,
    intent: &mut PaymentIntentRecord,
    input: VerifiedPaymentInput,
    quote: &CreditQuote,
) -> GatewayResult<VerificationResponse> {
    let mut tx = state.db.begin().await?;

    let updated_intent = sqlx::query_as::<_, PaymentIntentRecord>(
        r#"
        update payment_intents
        set
            status = 'credited',
            payer_wallet = coalesce($2, payer_wallet),
            provider = coalesce($3, provider),
            provider_reference = coalesce($4, provider_reference),
            reference_hex = coalesce($5, reference_hex),
            verified_at = coalesce(verified_at, now()),
            credited_at = coalesce(credited_at, now()),
            updated_at = now()
        where id = $1 and status = 'pending_verification'
        returning
            id,
            intent_reference,
            quote_reference,
            funding_method,
            user_id,
            payer_wallet,
            provider,
            provider_reference,
            expected_asset_code,
            expected_amount::float8 as expected_amount,
            expected_usd_value::float8 as expected_usd_value,
            expected_credits::float8 as expected_credits,
            pex_price_usd::float8 as pex_price_usd,
            burn_percentage::float8 as burn_percentage,
            burn_usd_value::float8 as burn_usd_value,
            reference_hex,
            status
        "#,
    )
    .bind(intent.id)
    .bind(input.payer_wallet.clone())
    .bind(input.provider.clone())
    .bind(input.provider_reference.clone())
    .bind(input.reference_hex.clone())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        GatewayError::Upstream("payment intent was already verified or credited".to_string())
    })?;

    let confirmation_id: Uuid = sqlx::query_scalar(
        r#"
        insert into payment_confirmations (
            payment_intent_id,
            method,
            verification_source,
            status,
            provider,
            provider_reference,
            reference_hex,
            tx_signature,
            payer_wallet,
            token_mint,
            trading_company_token_account,
            trading_company_revenue_token_account,
            amount_paid,
            currency,
            raw_confirmation
        ) values ($1, $2, $3, 'verified', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        returning id
        "#,
    )
    .bind(intent.id)
    .bind(input.method.as_str())
    .bind(input.verification_source)
    .bind(input.provider.clone())
    .bind(input.provider_reference.clone())
    .bind(input.reference_hex.clone())
    .bind(input.tx_signature.clone())
    .bind(input.payer_wallet.clone())
    .bind(input.token_mint.clone())
    .bind(input.trading_company_token_account.clone())
    .bind(input.trading_company_revenue_token_account.clone())
    .bind(input.amount_paid)
    .bind(input.currency.clone())
    .bind(input.raw_confirmation.clone())
    .fetch_one(&mut *tx)
    .await?;

    let credit_ledger_id: Uuid = sqlx::query_scalar(
        r#"
        insert into credit_ledger (
            payment_intent_id,
            quote_reference,
            user_id,
            credits_granted,
            account_id,
            ledger_direction,
            credit_delta,
            balance_after,
            source,
            source_reference,
            description
        ) values (
            $1,
            $2,
            $3,
            $4,
            case
                when $3 ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
                then $3::uuid
                else null
            end,
            'credit',
            $4,
            null,
            'payment',
            $2,
            'Verified payment credit grant'
        )
        on conflict (source, source_reference) where source is not null and source_reference is not null do update
        set credits_granted = credit_ledger.credits_granted
        returning id
        "#,
    )
    .bind(intent.id)
    .bind(&intent.quote_reference)
    .bind(updated_intent.user_id.clone())
    .bind(intent.expected_credits)
    .fetch_one(&mut *tx)
    .await?;

    let revenue_ledger_id: Uuid = sqlx::query_scalar(
        r#"
        insert into revenue_ledger (
            payment_intent_id,
            quote_reference,
            funding_method,
            asset_code,
            asset_amount,
            usd_value,
            pex_price_usd,
            revenue_status
        ) values ($1, $2, $3, $4, $5, $6, $7, 'realized')
        on conflict (payment_intent_id) do update
        set asset_amount = revenue_ledger.asset_amount
        returning id
        "#,
    )
    .bind(intent.id)
    .bind(&intent.quote_reference)
    .bind(input.method.as_str())
    .bind(&input.currency)
    .bind(input.amount_paid)
    .bind(intent.expected_usd_value)
    .bind(intent.pex_price_usd)
    .fetch_one(&mut *tx)
    .await?;

    let pex_price_usd = quote
        .pex_price_usd
        .unwrap_or_else(|| intent.pex_price_usd.unwrap_or(0.000012));
    let (fiat_revenue_usd, pex_burn_required, liability_status) =
        if matches!(input.method, CreditFundingMethod::Pex) {
            (0.0, 0.0, "not_required")
        } else {
            let required = round_amount(intent.burn_usd_value / pex_price_usd);
            (intent.expected_usd_value, required, "pending_pex_funding")
        };

    let burn_liability_id: Uuid = sqlx::query_scalar(
        r#"
        insert into burn_liabilities (
            payment_intent_id,
            quote_reference,
            funding_method,
            fiat_revenue_usd,
            burn_percentage,
            burn_usd_value,
            pex_price_usd,
            pex_burn_required,
            status
        ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        on conflict (payment_intent_id) do update
        set pex_burn_required = burn_liabilities.pex_burn_required
        returning id
        "#,
    )
    .bind(intent.id)
    .bind(&intent.quote_reference)
    .bind(input.method.as_str())
    .bind(fiat_revenue_usd)
    .bind(intent.burn_percentage)
    .bind(intent.burn_usd_value)
    .bind(pex_price_usd)
    .bind(pex_burn_required)
    .bind(liability_status)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        update credit_purchase_quotes
        set status = 'credited', updated_at = now()
        where quote_reference = $1 and status in ('quoted', 'accepted')
        "#,
    )
    .bind(&intent.quote_reference)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    if matches!(input.method, CreditFundingMethod::Pex) {
        record_pex_revenue_event(
            state,
            RecordPexRevenueInput {
                reference_hex: input.reference_hex.unwrap_or_default(),
                payer_wallet: input.payer_wallet,
                token_mint: input.token_mint,
                pex_received: input.amount_paid,
                credits_granted: intent.expected_credits,
                service_code: Some("credits_buy".to_string()),
                raw_event: Some(json!({
                    "paymentIntent": intent.intent_reference,
                    "quoteReference": intent.quote_reference,
                    "txSignature": input.tx_signature,
                    "verificationSource": "solana_payment_record"
                })),
            },
        )
        .await?;
    }

    *intent = updated_intent;

    Ok(VerificationResponse {
        accepted: true,
        intent: intent.clone(),
        confirmation_id: Some(confirmation_id),
        credit_ledger_id: Some(credit_ledger_id),
        revenue_ledger_id: Some(revenue_ledger_id),
        burn_liability_id: Some(burn_liability_id),
        message: "payment verified and Credits posted exactly once".to_string(),
    })
}

async fn fetch_payment_record(
    state: &AppState,
    reference_hex: &str,
) -> GatewayResult<PaymentRecordAccount> {
    let reference_bytes = hex_to_bytes(reference_hex)?;
    let reference_base58 = bs58::encode(reference_bytes).into_string();

    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccounts",
        "params": [
            state.config.perax_program_id,
            {
                "encoding": "base64",
                "commitment": "confirmed",
                "filters": [
                    { "dataSize": PAYMENT_RECORD_SPACE },
                    { "memcmp": { "offset": PAYMENT_RECORD_REFERENCE_OFFSET, "bytes": reference_base58 } }
                ]
            }
        ]
    });

    let response = state
        .http
        .post(&state.config.solana_rpc_url)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(GatewayError::Upstream(
            "failed to query Solana PaymentRecord".to_string(),
        ));
    }

    let body: Value = response.json().await?;
    let accounts = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            GatewayError::Upstream("Solana RPC PaymentRecord response is invalid".to_string())
        })?;

    let account = accounts.first().ok_or_else(|| {
        GatewayError::Upstream("PaymentRecord PDA/account not found for reference".to_string())
    })?;

    let data_b64 = account
        .get("account")
        .and_then(|value| value.get("data"))
        .and_then(Value::as_array)
        .and_then(|data| data.first())
        .and_then(Value::as_str)
        .ok_or_else(|| {
            GatewayError::Upstream("PaymentRecord account data is missing".to_string())
        })?;

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|_| {
            GatewayError::Upstream("PaymentRecord account data is not valid base64".to_string())
        })?;

    decode_payment_record(&data)
}

fn decode_payment_record(data: &[u8]) -> GatewayResult<PaymentRecordAccount> {
    if data.len() < PAYMENT_RECORD_SPACE as usize {
        return Err(GatewayError::Upstream(
            "PaymentRecord account data is too short".to_string(),
        ));
    }

    let mut offset = 8;
    let reference = take(data, &mut offset, 32)?;
    let payer = take(data, &mut offset, 32)?;
    let amount = u64::from_le_bytes(take_array(data, &mut offset)?);
    let token_mint = take(data, &mut offset, 32)?;
    let trading_company_token_account = take(data, &mut offset, 32)?;
    let trading_company_revenue_token_account = take(data, &mut offset, 32)?;
    let created_at = i64::from_le_bytes(take_array(data, &mut offset)?);

    Ok(PaymentRecordAccount {
        reference_hex: bytes_to_hex(reference),
        payer_wallet: bs58::encode(payer).into_string(),
        amount_base_units: amount,
        token_mint: bs58::encode(token_mint).into_string(),
        trading_company_token_account: bs58::encode(trading_company_token_account).into_string(),
        trading_company_revenue_token_account: bs58::encode(trading_company_revenue_token_account)
            .into_string(),
        created_at,
    })
}

fn take<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> GatewayResult<&'a [u8]> {
    let end = *offset + len;
    let slice = data.get(*offset..end).ok_or_else(|| {
        GatewayError::Upstream("PaymentRecord account data is truncated".to_string())
    })?;
    *offset = end;
    Ok(slice)
}

fn take_array<const N: usize>(data: &[u8], offset: &mut usize) -> GatewayResult<[u8; N]> {
    let slice = take(data, offset, N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    Ok(out)
}

fn ensure_intent_pending(intent: &PaymentIntentRecord) -> GatewayResult<()> {
    if intent.status != "pending_verification" {
        return Err(GatewayError::Upstream(
            "payment intent is not pending verification".to_string(),
        ));
    }
    Ok(())
}

fn ensure_method(intent: &PaymentIntentRecord, method: CreditFundingMethod) -> GatewayResult<()> {
    if intent.funding_method != method.as_str() {
        return Err(GatewayError::Upstream(
            "payment method does not match payment intent".to_string(),
        ));
    }
    Ok(())
}

fn fail_verification<T>(message: &str) -> GatewayResult<T> {
    Err(GatewayError::Upstream(message.to_string()))
}

fn verify_provider_signature(
    state: &AppState,
    provider: &str,
    provider_reference: &str,
    amount_paid: f64,
    currency: &str,
    status: &str,
    signature: Option<&str>,
) -> GatewayResult<()> {
    let secret = provider_webhook_secret(state, provider);
    let secret = secret.trim();
    if secret.is_empty() {
        return Ok(());
    }

    let signature = signature
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GatewayError::Upstream("provider signature is required".to_string()))?;
    let payload = format!("{provider}|{provider_reference}|{amount_paid:.6}|{currency}|{status}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| GatewayError::Upstream("invalid provider webhook secret".to_string()))?;
    mac.update(payload.as_bytes());
    let expected = bytes_to_hex(&mac.finalize().into_bytes());

    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(GatewayError::Upstream(
            "provider signature is invalid".to_string(),
        ));
    }

    Ok(())
}

fn provider_webhook_secret(state: &AppState, provider: &str) -> String {
    let provider = provider.trim().to_lowercase();
    let config = &state.config;

    let configured = match provider.as_str() {
        "stripe" | "card" => config.stripe_webhook_secret.as_str(),
        "payscribe" | "mobile_money" | "utility" | "electricity" | "data" => {
            config.payscribe_webhook_secret.as_str()
        }
        "bank" | "bank_transfer" | "virtual_account" | "va" => {
            config.bank_rails_webhook_secret.as_str()
        }
        "telnyx" => config.telnyx_webhook_signing_secret.as_str(),
        _ => "",
    };

    if !configured.trim().is_empty() {
        return configured.to_string();
    }

    env::var("PAYMENT_PROVIDER_WEBHOOK_SECRET").unwrap_or_default()
}

fn amount_matches(actual: f64, expected: f64) -> bool {
    (round_amount(actual) - round_amount(expected)).abs() < 0.000001
}

fn pex_to_base_units(amount: f64) -> GatewayResult<u64> {
    if !amount.is_finite() || amount <= 0.0 {
        return Err(GatewayError::Upstream(
            "PEX amount must be positive".to_string(),
        ));
    }
    Ok((amount * 1_000_000.0).round() as u64)
}

fn normalize_hex_32(value: &str) -> GatewayResult<String> {
    let normalized = value.trim().trim_start_matches("0x").to_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(GatewayError::Upstream(
            "referenceHex must be 32 bytes / 64 hex characters".to_string(),
        ));
    }
    Ok(normalized)
}

fn hex_to_bytes(value: &str) -> GatewayResult<Vec<u8>> {
    let normalized = normalize_hex_32(value)?;
    let mut out = Vec::with_capacity(32);
    for chunk in normalized.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk)
            .map_err(|_| GatewayError::Upstream("invalid hex bytes".to_string()))?;
        out.push(
            u8::from_str_radix(pair, 16)
                .map_err(|_| GatewayError::Upstream("invalid hex bytes".to_string()))?,
        );
    }
    Ok(out)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn clean_required(value: &str, field: &str) -> GatewayResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(GatewayError::Upstream(format!("{field} is required")));
    }
    Ok(trimmed.to_string())
}

fn round_amount(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

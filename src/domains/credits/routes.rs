use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domains::{
        credits::pricing_engine::{
            BuildCreditQuoteInput, CreditFundingMethod, CreditQuote, build_credit_quote,
            get_credit_quote_by_reference, mark_credit_quote_accepted,
        },
        payments::verification::{
            CreatePaymentIntentRequest, PaymentIntentRecord, create_intent_for_quote,
        },
    },
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteCreditsRequest {
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub promo_code: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteCreditsResponse {
    pub quote: CreditQuote,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsRequest {
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub quote_reference: Option<String>,
    pub pex_balance: Option<f64>,
    pub reference_hex: Option<String>,
    pub payer_wallet: Option<String>,
    pub promo_code: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PexRevenueLedgerSummary {
    pub event_id: Uuid,
    pub reference_hex: String,
    pub pex_received: f64,
    pub credits_granted: f64,
    pub immediate_burn_percentage: f64,
    pub pex_burn_amount: f64,
    pub pex_remaining_amount: f64,
    pub burn_status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsResponse {
    pub accepted: bool,
    pub method: CreditFundingMethod,
    pub requested_credits: f64,
    pub final_credits: f64,
    pub discount_percentage: f64,
    pub promo_code: Option<String>,
    pub asset_code: String,
    pub asset_required: f64,
    pub pex_required: f64,
    pub fiat_required: f64,
    pub usd_value: f64,
    pub burn_percentage: f64,
    pub burn_usd_value: f64,
    pub remaining_pex: Option<f64>,
    pub quote: CreditQuote,
    pub status: String,
    pub message: String,
    pub payment_intent: Option<PaymentIntentRecord>,
    pub pex_revenue_ledger: Option<PexRevenueLedgerSummary>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/credits/quote", post(quote_credits))
        .route("/credits/buy", post(buy_credits))
}

async fn quote_credits(
    State(state): State<AppState>,
    Json(payload): Json<QuoteCreditsRequest>,
) -> GatewayResult<Json<QuoteCreditsResponse>> {
    let quote = build_credit_quote(
        &state,
        BuildCreditQuoteInput {
            funding_method: payload.method,
            requested_credits: payload.credit_amount,
            promo_code: payload.promo_code,
            idempotency_key: payload.idempotency_key,
        },
    )
    .await?;

    Ok(Json(QuoteCreditsResponse {
        message: format_quote_message(&quote),
        quote,
    }))
}

async fn buy_credits(
    State(state): State<AppState>,
    Json(payload): Json<BuyCreditsRequest>,
) -> GatewayResult<Json<BuyCreditsResponse>> {
    let quote = match payload.quote_reference.as_deref() {
        Some(reference) if !reference.trim().is_empty() => {
            get_credit_quote_by_reference(&state, reference).await?
        }
        _ => {
            build_credit_quote(
                &state,
                BuildCreditQuoteInput {
                    funding_method: payload.method,
                    requested_credits: payload.credit_amount,
                    promo_code: payload.promo_code.clone(),
                    idempotency_key: payload.idempotency_key.clone(),
                },
            )
            .await?
        }
    };

    if quote.funding_method != payload.method {
        return Err(GatewayError::Upstream(
            "quote funding method does not match purchase funding method".to_string(),
        ));
    }

    if quote.status != "quoted" {
        return Err(GatewayError::Upstream(
            "quote has already been accepted, credited, cancelled, or expired".to_string(),
        ));
    }

    let remaining_pex = payload
        .pex_balance
        .map(|balance| balance - quote.pex_required);
    let accepted =
        quote.final_credits > 0.0 && remaining_pex.map(|value| value >= 0.0).unwrap_or(true);

    let mut status = if accepted {
        "pending_payment_verification".to_string()
    } else {
        "rejected".to_string()
    };
    let mut message = if accepted {
        format!(
            "Credit purchase accepted for quote {}. Create/complete payment verification before Credits are granted.",
            quote.quote_reference
        )
    } else {
        "Credit purchase rejected. Check amount or available PEX balance.".to_string()
    };
    let mut payment_intent = None;

    if accepted {
        mark_credit_quote_accepted(&state, &quote.quote_reference).await?;
        let intent = create_intent_for_quote(
            &state,
            &quote,
            CreatePaymentIntentRequest {
                quote_reference: quote.quote_reference.clone(),
                user_id: None,
                payer_wallet: payload.payer_wallet.clone(),
                provider: None,
                provider_reference: None,
                reference_hex: payload.reference_hex.clone(),
                idempotency_key: payload.idempotency_key.clone(),
            },
        )
        .await?;

        status = "payment_intent_created".to_string();
        message = format!(
            "Payment intent {} created for quote {} using {}. Credits will be granted only after payment verification succeeds.",
            intent.intent_reference, quote.quote_reference, quote.asset_code
        );
        payment_intent = Some(intent);
    }

    Ok(Json(BuyCreditsResponse {
        accepted,
        method: payload.method,
        requested_credits: quote.requested_credits,
        final_credits: quote.final_credits,
        discount_percentage: quote.discount_percentage,
        promo_code: quote.promo_code.clone(),
        asset_code: quote.asset_code.clone(),
        asset_required: if matches!(payload.method, CreditFundingMethod::Pex) {
            quote.pex_required
        } else {
            quote.fiat_required
        },
        pex_required: quote.pex_required,
        fiat_required: quote.fiat_required,
        usd_value: quote.usd_value,
        burn_percentage: quote.burn_percentage,
        burn_usd_value: quote.burn_usd_value,
        remaining_pex,
        quote,
        status,
        message,
        payment_intent,
        pex_revenue_ledger: None,
    }))
}

fn format_quote_message(quote: &CreditQuote) -> String {
    match quote.funding_method {
        CreditFundingMethod::Pex => format!(
            "Admin policy price: {} Credits costs ${:.2}. User pays {:.6} PEX at PEX/USD price {:?}. Discount: {}%.",
            quote.final_credits,
            quote.usd_value,
            quote.pex_required,
            quote.pex_price_usd,
            quote.discount_percentage
        ),
        CreditFundingMethod::Card | CreditFundingMethod::VirtualAccount => format!(
            "Admin policy price: {} Credits costs ${:.2}. User pays ${:.2}. Discount: {}%.",
            quote.final_credits, quote.usd_value, quote.fiat_required, quote.discount_percentage
        ),
        CreditFundingMethod::Stablecoin => format!(
            "Admin policy price: {} Credits costs ${:.2}. User pays {:.2} USDC/stablecoin. Discount: {}%.",
            quote.final_credits, quote.usd_value, quote.fiat_required, quote.discount_percentage
        ),
    }
}

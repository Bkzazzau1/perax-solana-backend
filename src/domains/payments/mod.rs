use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domains::solana::payment_listener::{
        UtilityPaymentEvent, UtilityPaymentRecord, ingest_utility_payment_event,
        mark_utility_payment_granted,
    },
    error::GatewayResult,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/api/trading-company-status",
            get(trading_company_status),
        )
        .route("/admin/api/utility-payments", get(list_utility_payments))
        .route(
            "/admin/api/utility-payments/ingest",
            post(ingest_utility_payment),
        )
        .route(
            "/admin/api/utility-payments/grant",
            post(grant_utility_payment),
        )
}

#[derive(Debug, Serialize)]
struct TradingCompanyStatusResponse {
    configured: bool,
    spl_token_account: String,
    warning: Option<&'static str>,
}

async fn trading_company_status(
    State(state): State<AppState>,
) -> Json<TradingCompanyStatusResponse> {
    let value = state.config.trading_co_treasury.trim();
    let configured = !value.is_empty()
        && !value.eq_ignore_ascii_case("replace-with-trading-company-token-account")
        && !value.eq_ignore_ascii_case("replace-with-trading-company-spl-token-account")
        && value.len() >= 32;

    Json(TradingCompanyStatusResponse {
        configured,
        spl_token_account: state.config.trading_co_treasury.clone(),
        warning: if configured {
            None
        } else {
            Some(
                "TRADING_CO_TREASURY is not configured with a real Trading Company SPL token account",
            )
        },
    })
}

#[derive(Debug, Deserialize)]
struct UtilityPaymentListQuery {
    limit: Option<i64>,
    status: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct UtilityPaymentListRecord {
    id: Uuid,
    reference_hex: String,
    payer_wallet: Option<String>,
    token_mint: Option<String>,
    trading_company_token_account: String,
    amount: f64,
    source: String,
    service_code: Option<String>,
    status: String,
    tx_signature: Option<String>,
}

#[derive(Debug, Serialize)]
struct UtilityPaymentListResponse {
    count: usize,
    payments: Vec<UtilityPaymentListRecord>,
}

async fn list_utility_payments(
    State(state): State<AppState>,
    Query(query): Query<UtilityPaymentListQuery>,
) -> GatewayResult<Json<UtilityPaymentListResponse>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let status = query.status.unwrap_or_else(|| "%".to_string());

    let payments = sqlx::query_as::<_, UtilityPaymentListRecord>(
        r#"
        select
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
        from utility_payments
        where status like $1
        order by detected_at desc
        limit $2
        "#,
    )
    .bind(status)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(UtilityPaymentListResponse {
        count: payments.len(),
        payments,
    }))
}

#[derive(Debug, Deserialize)]
struct IngestUtilityPaymentRequest {
    reference_hex: String,
    payer_wallet: Option<String>,
    token_mint: Option<String>,
    trading_company_token_account: Option<String>,
    amount: f64,
    source: Option<String>,
    service_code: Option<String>,
    tx_signature: Option<String>,
}

#[derive(Debug, Serialize)]
struct IngestUtilityPaymentResponse {
    payment: UtilityPaymentRecord,
    message: &'static str,
}

async fn ingest_utility_payment(
    State(state): State<AppState>,
    Json(request): Json<IngestUtilityPaymentRequest>,
) -> GatewayResult<Json<IngestUtilityPaymentResponse>> {
    let event = UtilityPaymentEvent {
        reference_hex: request.reference_hex,
        payer_wallet: request.payer_wallet,
        token_mint: request.token_mint,
        trading_company_token_account: request
            .trading_company_token_account
            .unwrap_or_else(|| state.config.trading_co_treasury.clone()),
        amount: request.amount,
        source: request.source.unwrap_or_else(|| "admin_test".to_string()),
        service_code: request.service_code,
        tx_signature: request.tx_signature,
        raw_event: Some(json!({ "source": "admin_test_endpoint" })),
    };

    let payment = ingest_utility_payment_event(&state, event).await?;

    Ok(Json(IngestUtilityPaymentResponse {
        payment,
        message: "utility payment ingested and confirmed",
    }))
}

#[derive(Debug, Deserialize)]
struct GrantUtilityPaymentRequest {
    reference_hex: String,
}

#[derive(Debug, Serialize)]
struct GrantUtilityPaymentResponse {
    payment: Option<UtilityPaymentRecord>,
    granted: bool,
}

async fn grant_utility_payment(
    State(state): State<AppState>,
    Json(request): Json<GrantUtilityPaymentRequest>,
) -> GatewayResult<Json<GrantUtilityPaymentResponse>> {
    let payment = mark_utility_payment_granted(&state, &request.reference_hex).await?;
    let granted = payment.is_some();

    Ok(Json(GrantUtilityPaymentResponse { payment, granted }))
}

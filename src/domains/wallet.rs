use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domains::{auth::middleware::AuthenticatedAccount, telecom::billing::credit_balance},
    error::GatewayResult,
    state::AppState,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSummaryResponse {
    pub account_id: Uuid,
    pub email: Option<String>,
    pub pex_wallet_address: Option<String>,
    pub sol: f64,
    pub usdc: f64,
    #[serde(rename = "PEX")]
    pub pex: f64,
    pub credits: f64,
    pub pex_usd_rate: f64,
    pub pex_usd_value: f64,
    pub burned_pex: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct AccountWalletRow {
    email: Option<String>,
    pex_wallet_address: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/wallet/summary", get(wallet_summary))
}

async fn wallet_summary(
    State(state): State<AppState>,
    account: AuthenticatedAccount,
) -> GatewayResult<Json<WalletSummaryResponse>> {
    let credits = credit_balance(&state, account.account_id).await?;
    let account_row = sqlx::query_as::<_, AccountWalletRow>(
        r#"
        select email, pex_wallet_address
        from accounts
        where id = $1
        "#,
    )
    .bind(account.account_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(WalletSummaryResponse {
        account_id: account.account_id,
        email: account_row.as_ref().and_then(|row| row.email.clone()),
        pex_wallet_address: account_row.and_then(|row| row.pex_wallet_address),
        sol: 0.0,
        usdc: 0.0,
        pex: 0.0,
        credits,
        pex_usd_rate: 0.10,
        pex_usd_value: 0.0,
        burned_pex: 0.0,
    }))
}

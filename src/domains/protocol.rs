use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::{
    config::Config,
    domains::solana::tokenomics::{self, PEX_TOKENOMICS, UNLOCK_POLICY},
    state::AppState,
};

const UTILITY_APP_URL: &str = "https://app.pera-x.xyz";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStatusResponse {
    pub protocol_name: &'static str,
    pub token_symbol: &'static str,
    pub token_name: &'static str,
    pub network: String,
    pub program_id: String,
    pub total_supply: u64,
    pub decimals: u8,
    pub fixed_supply: bool,
    pub initial_price_usd: f64,
    pub initial_valuation_usd: f64,
    pub liquidity_venue: &'static str,
    pub liquidity_pair: &'static str,
    pub initial_liquidity_percentage: f64,
    pub initial_liquidity_pex_amount: u64,
    pub initial_liquidity_quote_usd: f64,
    pub utility_app_url: &'static str,
    pub trading_company_locked_account: String,
    pub trading_company_revenue_account: String,
    pub trading_company_locked_account_configured: bool,
    pub trading_company_revenue_account_configured: bool,
    pub burn_execution_mode: String,
    pub immediate_burn_percentage: f64,
    pub monthly_sell_cap_percentage: f64,
    pub max_daily_unlock_percentage_of_total_supply: f64,
    pub max_daily_unlock_amount: u64,
    pub max_monthly_unlock_percentage_of_total_supply: f64,
    pub max_monthly_unlock_amount: u64,
    pub release_authority: &'static str,
    pub safety_authority: &'static str,
    pub emergency_pause_enabled: bool,
    pub release_preview_total_amount: u64,
    pub trading_company_release_amount: u64,
    pub other_release_wallets_amount: u64,
    pub trading_company_release_share_percentage: f64,
    pub other_release_wallets_share_percentage: f64,
    pub status: &'static str,
    pub note: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/protocol/status", get(protocol_status))
}

async fn protocol_status(State(state): State<AppState>) -> Json<ProtocolStatusResponse> {
    let config = &state.config;
    let trading_wallet = &state.trading_co_wallet;
    let release_preview =
        tokenomics::release_allocation_preview(UNLOCK_POLICY.max_daily_unlock_amount);

    Json(ProtocolStatusResponse {
        protocol_name: "Pera-X",
        token_symbol: PEX_TOKENOMICS.token_symbol,
        token_name: PEX_TOKENOMICS.token_name,
        network: network_label(config),
        program_id: config.perax_program_id.clone(),
        total_supply: PEX_TOKENOMICS.total_supply,
        decimals: PEX_TOKENOMICS.decimals,
        fixed_supply: PEX_TOKENOMICS.fixed_supply,
        initial_price_usd: PEX_TOKENOMICS.initial_price_usd,
        initial_valuation_usd: PEX_TOKENOMICS.initial_valuation_usd,
        liquidity_venue: PEX_TOKENOMICS.liquidity_venue,
        liquidity_pair: PEX_TOKENOMICS.liquidity_pair,
        initial_liquidity_percentage: PEX_TOKENOMICS.initial_liquidity_percentage,
        initial_liquidity_pex_amount: PEX_TOKENOMICS.initial_liquidity_pex_amount,
        initial_liquidity_quote_usd: PEX_TOKENOMICS.initial_liquidity_quote_usd,
        utility_app_url: UTILITY_APP_URL,
        trading_company_locked_account: trading_wallet.locked_account.clone(),
        trading_company_revenue_account: trading_wallet.revenue_account.clone(),
        trading_company_locked_account_configured: is_real_account(&trading_wallet.locked_account),
        trading_company_revenue_account_configured: is_real_account(
            &trading_wallet.revenue_account,
        ),
        burn_execution_mode: config.burn_execution_mode.as_str().to_string(),
        immediate_burn_percentage: config.pex_immediate_burn_percentage,
        monthly_sell_cap_percentage: config.pex_monthly_sell_cap_percentage,
        max_daily_unlock_percentage_of_total_supply: UNLOCK_POLICY
            .max_daily_unlock_percentage_of_total_supply,
        max_daily_unlock_amount: UNLOCK_POLICY.max_daily_unlock_amount,
        max_monthly_unlock_percentage_of_total_supply: UNLOCK_POLICY
            .max_monthly_unlock_percentage_of_total_supply,
        max_monthly_unlock_amount: UNLOCK_POLICY.max_monthly_unlock_amount,
        release_authority: UNLOCK_POLICY.release_authority,
        safety_authority: UNLOCK_POLICY.safety_authority,
        emergency_pause_enabled: UNLOCK_POLICY.emergency_pause_enabled,
        release_preview_total_amount: release_preview.total_release_amount,
        trading_company_release_amount: release_preview.trading_company_amount,
        other_release_wallets_amount: release_preview.other_release_wallets_amount,
        trading_company_release_share_percentage: release_preview.trading_company_share_percentage,
        other_release_wallets_share_percentage: release_preview
            .other_release_wallets_share_percentage,
        status: "configured",
        note: "Public status endpoint for the Pera-X utility app and dApp. Treat Devnet data as testing data until mainnet deployment is complete.",
    })
}

fn network_label(config: &Config) -> String {
    let rpc = config.solana_rpc_url.to_lowercase();
    if rpc.contains("devnet") {
        "solana-devnet".to_string()
    } else if rpc.contains("testnet") {
        "solana-testnet".to_string()
    } else if rpc.contains("mainnet") {
        "solana-mainnet".to_string()
    } else {
        "custom-solana-rpc".to_string()
    }
}

fn is_real_account(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && !trimmed.starts_with("replace-with-") && trimmed.len() >= 32
}

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::{config::Config, state::AppState};

const PEX_TOTAL_SUPPLY: u64 = 1_000_000_000;
const PEX_DECIMALS: u8 = 6;
const UTILITY_APP_URL: &str = "https://app.pera-x.xyz";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStatusResponse {
    pub protocol_name: &'static str,
    pub token_symbol: &'static str,
    pub network: String,
    pub program_id: String,
    pub total_supply: u64,
    pub decimals: u8,
    pub utility_app_url: &'static str,
    pub trading_company_locked_account_configured: bool,
    pub trading_company_revenue_account_configured: bool,
    pub burn_execution_mode: String,
    pub immediate_burn_percentage: f64,
    pub monthly_sell_cap_percentage: f64,
    pub status: &'static str,
    pub note: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/protocol/status", get(protocol_status))
}

async fn protocol_status(State(state): State<AppState>) -> Json<ProtocolStatusResponse> {
    let config = &state.config;

    Json(ProtocolStatusResponse {
        protocol_name: "Pera-X",
        token_symbol: "PEX",
        network: network_label(config),
        program_id: config.perax_program_id.clone(),
        total_supply: PEX_TOTAL_SUPPLY,
        decimals: PEX_DECIMALS,
        utility_app_url: UTILITY_APP_URL,
        trading_company_locked_account_configured: is_real_account(&config.trading_co_treasury),
        trading_company_revenue_account_configured: is_real_account(
            &config.trading_company_second_wallet,
        ),
        burn_execution_mode: config.burn_execution_mode.as_str().to_string(),
        immediate_burn_percentage: config.pex_immediate_burn_percentage,
        monthly_sell_cap_percentage: config.pex_monthly_sell_cap_percentage,
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

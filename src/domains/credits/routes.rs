use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};

use crate::{error::GatewayResult, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsRequest {
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub pex_balance: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum CreditFundingMethod {
    Pex,
    Card,
    Stablecoin,
    VirtualAccount,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyCreditsResponse {
    pub accepted: bool,
    pub method: CreditFundingMethod,
    pub credit_amount: f64,
    pub pex_required: f64,
    pub remaining_pex: Option<f64>,
    pub status: String,
    pub message: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/credits/buy", post(buy_credits))
}

async fn buy_credits(
    Json(payload): Json<BuyCreditsRequest>,
) -> GatewayResult<Json<BuyCreditsResponse>> {
    let credit_amount = payload.credit_amount.max(0.0);
    let pex_required = match payload.method {
        CreditFundingMethod::Pex => credit_amount,
        CreditFundingMethod::Card
        | CreditFundingMethod::Stablecoin
        | CreditFundingMethod::VirtualAccount => 0.0,
    };

    let remaining_pex = payload.pex_balance.map(|balance| balance - pex_required);
    let accepted = credit_amount > 0.0 && remaining_pex.map(|value| value >= 0.0).unwrap_or(true);

    Ok(Json(BuyCreditsResponse {
        accepted,
        method: payload.method,
        credit_amount,
        pex_required,
        remaining_pex,
        status: if accepted {
            "pending_settlement".to_string()
        } else {
            "rejected".to_string()
        },
        message: if accepted {
            "Credit purchase request accepted. Settlement and crediting will be finalized by backend policy.".to_string()
        } else {
            "Credit purchase rejected. Check amount or available PEX balance.".to_string()
        },
    }))
}

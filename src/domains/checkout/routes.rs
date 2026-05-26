use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};

use crate::{error::GatewayResult, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutConfirmRequest {
    pub product_id: String,
    pub product_name: String,
    pub credit_cost: f64,
    pub credit_balance: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutConfirmResponse {
    pub order_id: String,
    pub status: String,
    pub product_id: String,
    pub product_name: String,
    pub credit_cost: f64,
    pub remaining_credits: f64,
    pub message: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/checkout/confirm", post(confirm_checkout))
}

async fn confirm_checkout(
    Json(payload): Json<CheckoutConfirmRequest>,
) -> GatewayResult<Json<CheckoutConfirmResponse>> {
    let credit_cost = payload.credit_cost.max(0.0);
    let remaining_credits = payload.credit_balance - credit_cost;
    let confirmed = credit_cost > 0.0 && remaining_credits >= 0.0;

    Ok(Json(CheckoutConfirmResponse {
        order_id: format!("order_{}", chrono::Utc::now().timestamp_millis()),
        status: if confirmed {
            "confirmed".to_string()
        } else {
            "rejected".to_string()
        },
        product_id: payload.product_id,
        product_name: payload.product_name,
        credit_cost,
        remaining_credits,
        message: if confirmed {
            "Checkout confirmed. Credits can be deducted and service activation can continue."
                .to_string()
        } else {
            "Checkout rejected. Insufficient Credits or invalid service cost.".to_string()
        },
    }))
}

// src/domains/telecom/routes.rs
use axum::{
    Router,
    routing::{get, post},
};

use crate::{
    domains::telecom::{inventory, sms, voice},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/telecom/call/{id}", get(voice::get_call))
        .route("/telecom/webrtc/offer", post(voice::create_offer))
        .route("/telecom/sms", post(sms::send_sms))
        .route(
            "/telecom/numbers/search",
            get(inventory::search_global_numbers),
        )
        .route("/telecom/numbers/buy", post(inventory::purchase_number))
}

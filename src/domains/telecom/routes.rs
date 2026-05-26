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
        .route("/telecom/calls/start", post(voice::start_call_session))
        .route("/telecom/calls/end", post(voice::end_call_session))
        .route("/telecom/webrtc/offer", post(voice::create_offer))
        .route("/telecom/sms", post(sms::send_sms))
        .route("/telecom/sms/inbound", post(sms::receive_inbound_sms))
        .route("/telecom/sms/inbox", get(sms::get_sms_inbox))
        .route(
            "/telecom/numbers/search",
            get(inventory::search_global_numbers),
        )
        .route(
            "/telecom/numbers/pricing",
            get(inventory::list_number_pricing),
        )
        .route("/telecom/numbers/mine", get(inventory::list_my_numbers))
        .route(
            "/telecom/numbers/{id}/cancel",
            post(inventory::cancel_number_subscription),
        )
        .route(
            "/telecom/numbers/{id}/reactivate",
            post(inventory::reactivate_number_subscription),
        )
        .route(
            "/telecom/numbers/renewals/process",
            post(inventory::process_due_number_renewals),
        )
        .route(
            "/telecom/numbers/reserve",
            post(inventory::reserve_number_with_credits),
        )
        .route("/telecom/numbers/buy", post(inventory::purchase_number))
}

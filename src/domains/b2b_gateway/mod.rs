pub mod handlers;
pub mod services;

use axum::{Router, routing::post};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/proxy/claude/messages",
            post(handlers::proxy_claude_messages),
        )
        .route(
            "/v1/proxy/copyleaks/scan",
            post(handlers::scan_edtech_payload),
        )
}

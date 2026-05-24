pub mod handlers;
pub mod services;

use axum::{Router, routing::post};

use crate::state::AppState;

/// Mounts and exposes the Codex B2B Utility API Proxy routes to the main application server.
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

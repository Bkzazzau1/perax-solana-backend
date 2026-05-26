use axum::{Router, routing::post};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/api/auth/login", post(|| async { "ok" }))
}

mod common;
mod config;
mod domains;
mod error;
mod infra;
mod state;

use axum::{Router, response::Redirect, routing::get};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Config, error::GatewayResult, state::AppState};

#[tokio::main]
async fn main() -> GatewayResult<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = Config::from_env()?;
    let bind_addr = config.bind_addr();
    let state = AppState::build(config).await?;

    domains::solana::client::spawn_treasury_listener(state.clone());
    domains::solana::burner::spawn_daily_burner(state.clone());
    domains::solana::market_monitor::spawn_market_unlock_monitor(state.clone());

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .merge(domains::admin::router())
        .merge(domains::admin_auth::router())
        .merge(domains::admin_pricing::router())
        .merge(domains::payments::router())
        .merge(domains::pricing::router())
        .merge(domains::ai::routes::router())
        .merge(domains::credits::routes::router())
        .merge(domains::checkout::routes::router())
        .merge(domains::b2b_gateway::router())
        .merge(domains::telecom::routes::router())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(
        "perax utility gateway listening on {}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;

    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn root() -> Redirect {
    Redirect::temporary("/admin")
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

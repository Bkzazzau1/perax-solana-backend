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

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .merge(domains::admin::router())
        .merge(domains::payments::router())
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
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn root() -> Redirect {
    Redirect::temporary("/admin")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

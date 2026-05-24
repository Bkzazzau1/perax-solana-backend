use crate::error::GatewayResult;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

/// Configures and establishes a thread-safe connection pool to the PostgreSQL cluster.
/// Programmatically executes structural schema updates on boot up.
pub async fn connect(database_url: &str) -> GatewayResult<PgPool> {
    tracing::info!("Initializing structural PostgreSQL connection pool...");

    let pool = PgPoolOptions::new()
        .max_connections(20) // Kept at your clean baseline target
        .min_connections(2) // Maintains a warm connection floor to absorb instant latency spikes
        .acquire_timeout(Duration::from_secs(5)) // Blocks database pool starvation from freezing the worker threads
        .idle_timeout(Duration::from_secs(600)) // Reclaims system resources by closing unused idle backends
        .connect(database_url)
        .await?;

    // Programmatically trigger SQLx migrations located inside the /migrations directory.
    // This removes the need to manually run `sqlx migrate run` on your production nodes.
    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!("PostgreSQL connection pool initialized and migrated successfully.");
    Ok(pool)
}

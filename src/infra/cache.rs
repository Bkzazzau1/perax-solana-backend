use fred::{
    interfaces::{ClientLike, HashesInterface},
    prelude::{Builder, Client as RedisClient, Config},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{GatewayError, GatewayResult};

#[derive(Clone)]
pub enum CacheStore {
    Redis(RedisClient),
    Memory(Arc<RwLock<HashMap<String, f64>>>),
}

/// Establishes a multiplexed, thread-safe connection to the Redis server cache.
pub async fn connect(redis_url: &str) -> GatewayResult<CacheStore> {
    if redis_url == "memory://local" {
        tracing::warn!("Using in-memory cache store. This is intended for local development only.");
        return Ok(CacheStore::Memory(Arc::new(RwLock::new(HashMap::new()))));
    }

    tracing::info!("Initializing high-performance multiplexed Redis connection client...");

    let config = Config::from_url(redis_url)?;
    let client = Builder::from_config(config).build()?;
    client.init().await?;

    tracing::info!("Redis client successfully connected and running.");
    Ok(CacheStore::Redis(client))
}

/// Automates an atomic look-up against the account balance map stored in Redis memory.
/// This acts as our API firewall, blocking depleted balances before they execute downstream APIs.
pub async fn account_has_credits(cache: &CacheStore, account_id: Uuid) -> GatewayResult<bool> {
    // Structure our redis keys cleanly using standard workspace scoping
    let balance_key = format!("client:balance:{}", account_id);

    let credits = get_credits(cache, &balance_key).await?;

    match credits {
        Some(balance) => {
            if balance <= 0.0 {
                tracing::warn!(account_id = %account_id, balance = %balance, "API request blocked: Account balance exhausted");
                Ok(false)
            } else {
                Ok(true)
            }
        }
        None => {
            // If the account doesn't exist in Redis yet, treat it as uncredited
            tracing::error!(account_id = %account_id, "API request blocked: Account record missing from memory cache");
            Ok(false)
        }
    }
}

pub async fn get_credits(cache: &CacheStore, balance_key: &str) -> GatewayResult<Option<f64>> {
    match cache {
        CacheStore::Redis(client) => client
            .hget(balance_key, "credits")
            .await
            .map_err(GatewayError::Redis),
        CacheStore::Memory(values) => {
            let mut values = values.write().await;
            let balance = values.entry(balance_key.to_string()).or_insert(1_000_000.0);
            Ok(Some(*balance))
        }
    }
}

pub async fn increment_credits(
    cache: &CacheStore,
    balance_key: &str,
    amount: f64,
) -> GatewayResult<f64> {
    match cache {
        CacheStore::Redis(client) => client
            .hincrbyfloat(balance_key, "credits", amount)
            .await
            .map_err(GatewayError::Redis),
        CacheStore::Memory(values) => {
            let mut values = values.write().await;
            let balance = values.entry(balance_key.to_string()).or_insert(0.0);
            *balance += amount;
            Ok(*balance)
        }
    }
}

pub async fn set_credits(cache: &CacheStore, balance_key: &str, amount: f64) -> GatewayResult<f64> {
    match cache {
        CacheStore::Redis(_) => {
            let current = get_credits(cache, balance_key).await?.unwrap_or(0.0);
            increment_credits(cache, balance_key, amount - current).await
        }
        CacheStore::Memory(values) => {
            let mut values = values.write().await;
            values.insert(balance_key.to_string(), amount);
            Ok(amount)
        }
    }
}

use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    config::Config,
    domains::solana::client::{TradingWallet, TreasuryRpc},
    error::GatewayResult,
    infra::{
        cache::{self, CacheStore},
        db,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cache: CacheStore,
    pub solana_rpc: Arc<TreasuryRpc>,
    pub trading_co_wallet: Arc<TradingWallet>,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
}

impl AppState {
    pub async fn build(config: Config) -> GatewayResult<Self> {
        let db = db::connect(&config.database_url).await?;
        let cache = cache::connect(&config.redis_url).await?;
        let solana_rpc = Arc::new(TreasuryRpc::new(config.solana_rpc_url.clone()));
        let trading_co_wallet = Arc::new(TradingWallet::new(
    config.trading_co_treasury.clone(),
    config.trading_company_second_wallet.clone(),
));
        let http = reqwest::Client::new();

        Ok(Self {
            db,
            cache,
            solana_rpc,
            trading_co_wallet,
            http,
            config: Arc::new(config),
        })
    }
}

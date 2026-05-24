use std::{env, net::SocketAddr};

use crate::error::{GatewayError, GatewayResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BurnExecutionMode {
    Manual,
    Approved,
}

impl BurnExecutionMode {
    pub fn from_env_value(value: &str) -> GatewayResult<Self> {
        match value.trim().to_lowercase().as_str() {
            "manual" => Ok(Self::Manual),
            "approved" => Ok(Self::Approved),
            other => Err(GatewayError::Config(format!(
                "BURN_EXECUTION_MODE must be either 'manual' or 'approved', got '{other}'"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Approved => "approved",
        }
    }

    pub fn allows_approved_execution(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub redis_url: String,
    pub solana_rpc_url: String,
    pub solana_ws_url: String,
    pub perax_anchor_workspace: String,
    pub perax_program_id: String,
    pub trading_co_treasury: String,
    pub burn_execution_mode: BurnExecutionMode,
    pub jwt_secret: String,
    pub claude_base_url: String,
    pub copyleaks_base_url: String,
    pub telnyx_base_url: String,
}

impl Config {
    pub fn from_env() -> GatewayResult<Self> {
        let burn_execution_mode =
            BurnExecutionMode::from_env_value(&env_or("BURN_EXECUTION_MODE", "manual"))?;

        let config = Self {
            host: env_or("HOST", "0.0.0.0"),
            port: parse_port()?,
            database_url: required("DATABASE_URL")?,
            redis_url: env_or("REDIS_URL", "redis://127.0.0.1:6379"),
            solana_rpc_url: env_or("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com"),
            solana_ws_url: env_or("SOLANA_WS_URL", "wss://api.mainnet-beta.solana.com"),
            perax_anchor_workspace: env_or(
                "PERAX_ANCHOR_WORKSPACE",
                "C:\\PROJECTS\\smartcontract PEX\\perax-ecosystem\\perax-contracts",
            ),
            perax_program_id: env_or("PERAX_PROGRAM_ID", "11111111111111111111111111111111"),
            trading_co_treasury: required("TRADING_CO_TREASURY")?,
            burn_execution_mode,
            jwt_secret: required("JWT_SECRET")?,
            claude_base_url: env_or("CLAUDE_BASE_URL", "https://api.anthropic.com"),
            copyleaks_base_url: env_or("COPYLEAKS_BASE_URL", "https://api.copyleaks.com"),
            telnyx_base_url: env_or("TELNYX_BASE_URL", "https://api.telnyx.com"),
        };

        config.validate()?;
        Ok(config)
    }

    pub fn bind_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("validated bind address")
    }

    fn validate(&self) -> GatewayResult<()> {
        if self.jwt_secret.len() < 32 {
            return Err(GatewayError::Config(
                "JWT_SECRET must be at least 32 characters".to_string(),
            ));
        }

        self.bind_addr();
        Ok(())
    }
}

fn required(key: &'static str) -> GatewayResult<String> {
    env::var(key).map_err(|_| GatewayError::Config(format!("{key} is required")))
}

fn env_or(key: &'static str, fallback: &'static str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn parse_port() -> GatewayResult<u16> {
    env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .map_err(|_| GatewayError::Config("PORT must be a valid u16".to_string()))
}

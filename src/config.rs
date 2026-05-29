use std::{env, net::SocketAddr};

use crate::error::{GatewayError, GatewayResult};

pub const DEFAULT_PERAX_PROGRAM_ID: &str = "FqEiSx5vujh2vi3yk12NaZMXhjMSaKovGUuzcKiAgshn";
pub const DEFAULT_PEX_MINT_ADDRESS: &str = "DnkAW3B1ckzW6eimgSBNPK3XTt83wMiZRETy8iF3gdsn";
pub const DEFAULT_PERAX_STATE_PDA: &str = "8LNUe8ud9Lrtt1HmuS132YoGs5tBNEeWeviNJwWDkHWT";
pub const DEFAULT_PEX_TOKEN_SYMBOL: &str = "PEX";
pub const DEFAULT_PEX_DECIMALS: u8 = 6;
pub const DEFAULT_PEX_TOTAL_SUPPLY: u64 = 1_000_000_000;
pub const DEFAULT_PEX_INITIAL_PRICE_USD: f64 = 0.000012;
pub const DEFAULT_PEX_INITIAL_LIQUIDITY_AMOUNT: u64 = 380_000_000;
pub const DEFAULT_PEX_INITIAL_LIQUIDITY_QUOTE_USD: f64 = 4_560.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BurnExecutionMode {
    Disabled,
    Automatic,
}

impl BurnExecutionMode {
    pub fn from_env_value(value: &str) -> GatewayResult<Self> {
        match value.trim().to_lowercase().as_str() {
            "disabled" | "manual" | "prepare_only" | "prepare-only" => Ok(Self::Disabled),
            "automatic" | "auto" | "system" | "approved" => Ok(Self::Automatic),
            other => Err(GatewayError::Config(format!(
                "BURN_EXECUTION_MODE must be either 'disabled' or 'automatic', got '{other}'"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Automatic => "automatic",
        }
    }

    pub fn allows_automatic_execution(&self) -> bool {
        matches!(self, Self::Automatic)
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
    pub perax_state_pda: String,
    pub pex_mint_address: String,
    pub trading_co_treasury: String,
    pub trading_company_second_wallet: String,
    pub pex_immediate_burn_percentage: f64,
    pub pex_monthly_sell_cap_percentage: f64,
    pub burn_execution_mode: BurnExecutionMode,
    pub unlock_requires_manual_approval: bool,
    pub jwt_secret: String,
    pub claude_base_url: String,
    pub copyleaks_base_url: String,
    pub telnyx_base_url: String,
}

impl Config {
    pub fn from_env() -> GatewayResult<Self> {
        let burn_execution_mode =
            BurnExecutionMode::from_env_value(&env_or("BURN_EXECUTION_MODE", "disabled"))?;

        let config = Self {
            host: env_or("HOST", "0.0.0.0"),
            port: parse_port()?,
            database_url: required("DATABASE_URL")?,
            redis_url: env_or("REDIS_URL", "redis://127.0.0.1:6379"),
            solana_rpc_url: env_or("SOLANA_RPC_URL", "https://api.devnet.solana.com"),
            solana_ws_url: env_or("SOLANA_WS_URL", "wss://api.devnet.solana.com"),
            perax_anchor_workspace: env_or(
                "PERAX_ANCHOR_WORKSPACE",
                "C:\\PROJECTS\\Pera-X-ecosystem\\perax-contracts",
            ),
            perax_program_id: env_or("PERAX_PROGRAM_ID", DEFAULT_PERAX_PROGRAM_ID),
            perax_state_pda: env_or("PERAX_STATE_PDA", DEFAULT_PERAX_STATE_PDA),
            pex_mint_address: env_or("PEX_MINT_ADDRESS", DEFAULT_PEX_MINT_ADDRESS),
            trading_co_treasury: required_any(&[
                "TRADING_COMPANY_TOKEN_ACCOUNT",
                "TRADING_CO_TREASURY",
            ])?,
            trading_company_second_wallet: required_any(&[
                "TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT",
                "TRADING_COMPANY_SECOND_WALLET",
            ])?,
            pex_immediate_burn_percentage: parse_percentage_env("PEX_IMMEDIATE_BURN_PERCENTAGE", 10.0)?,
            pex_monthly_sell_cap_percentage: parse_percentage_env("PEX_MONTHLY_SELL_CAP_PERCENTAGE", 50.0)?,
            burn_execution_mode,
            unlock_requires_manual_approval: parse_bool_env("PEX_UNLOCK_REQUIRES_MANUAL_APPROVAL", false)?,
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

        validate_wallet_like("PERAX_PROGRAM_ID", &self.perax_program_id)?;
        validate_wallet_like("PERAX_STATE_PDA", &self.perax_state_pda)?;
        validate_wallet_like("PEX_MINT_ADDRESS", &self.pex_mint_address)?;
        validate_wallet_like("TRADING_COMPANY_TOKEN_ACCOUNT", &self.trading_co_treasury)?;
        validate_wallet_like(
            "TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT",
            &self.trading_company_second_wallet,
        )?;

        if self.trading_co_treasury == self.trading_company_second_wallet {
            return Err(GatewayError::Config(
                "Trading Company locked account and revenue account must be different".to_string(),
            ));
        }

        if self.perax_program_id == "11111111111111111111111111111111" {
            return Err(GatewayError::Config(
                "PERAX_PROGRAM_ID must not use the old placeholder program id".to_string(),
            ));
        }

        if self.unlock_requires_manual_approval {
            return Err(GatewayError::Config(
                "PEX_UNLOCK_REQUIRES_MANUAL_APPROVAL must remain false; Pera-X uses market-condition oracle-only release approval".to_string(),
            ));
        }

        if !(0.0..=100.0).contains(&self.pex_immediate_burn_percentage) {
            return Err(GatewayError::Config(
                "PEX_IMMEDIATE_BURN_PERCENTAGE must be between 0 and 100".to_string(),
            ));
        }

        if self.pex_monthly_sell_cap_percentage != 50.0 {
            return Err(GatewayError::Config(
                "PEX_MONTHLY_SELL_CAP_PERCENTAGE must remain 50 under approved PEX policy"
                    .to_string(),
            ));
        }

        self.bind_addr();
        Ok(())
    }
}

fn required(key: &'static str) -> GatewayResult<String> {
    env::var(key).map_err(|_| GatewayError::Config(format!("{key} is required")))
}

fn required_any(keys: &[&'static str]) -> GatewayResult<String> {
    for key in keys {
        if let Ok(value) = env::var(key) {
            if !value.trim().is_empty() {
                return Ok(value);
            }
        }
    }

    Err(GatewayError::Config(format!(
        "one of {} is required",
        keys.join(" or ")
    )))
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

fn parse_percentage_env(key: &'static str, fallback: f64) -> GatewayResult<f64> {
    env::var(key)
        .unwrap_or_else(|_| fallback.to_string())
        .parse::<f64>()
        .map_err(|_| GatewayError::Config(format!("{key} must be a valid percentage number")))
}

fn parse_bool_env(key: &'static str, fallback: bool) -> GatewayResult<bool> {
    match env::var(key) {
        Ok(value) => match value.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            other => Err(GatewayError::Config(format!(
                "{key} must be true or false, got '{other}'"
            ))),
        },
        Err(_) => Ok(fallback),
    }
}

fn validate_wallet_like(key: &'static str, value: &str) -> GatewayResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("replace-with-") || trimmed.len() < 32 {
        return Err(GatewayError::Config(format!(
            "{key} must be configured with a real Solana account address"
        )));
    }

    Ok(())
}

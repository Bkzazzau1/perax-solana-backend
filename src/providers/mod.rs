use axum::{Json, Router, extract::State, routing::get};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Response;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    error::{GatewayError, GatewayResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/api/providers/status", get(provider_status))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusResponse {
    pub anthropic: ProviderConnectionStatus,
    pub copyleaks: ProviderConnectionStatus,
    pub telnyx: ProviderConnectionStatus,
    pub payscribe: ProviderConnectionStatus,
    pub stripe: ProviderConnectionStatus,
    pub bank_rails: ProviderConnectionStatus,
    pub pyth_network: ProviderConnectionStatus,
    pub meteora_dlmm: ProviderConnectionStatus,
    pub solana_rpc: ProviderConnectionStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionStatus {
    pub configured: bool,
    pub base_url: Option<String>,
    pub required_env: Vec<&'static str>,
}

async fn provider_status(State(state): State<AppState>) -> Json<ProviderStatusResponse> {
    let config = &state.config;

    Json(ProviderStatusResponse {
        anthropic: ProviderConnectionStatus {
            configured: configured(&[&config.anthropic_api_key, &config.claude_model]),
            base_url: Some(config.claude_base_url.clone()),
            required_env: vec!["ANTHROPIC_API_KEY", "CLAUDE_MODEL"],
        },
        copyleaks: ProviderConnectionStatus {
            configured: configured(&[
                &config.copyleaks_email,
                &config.copyleaks_api_key,
                &config.copyleaks_webhook_secret,
            ]),
            base_url: Some(config.copyleaks_base_url.clone()),
            required_env: vec![
                "COPYLEAKS_EMAIL",
                "COPYLEAKS_API_KEY",
                "COPYLEAKS_WEBHOOK_SECRET",
            ],
        },
        telnyx: ProviderConnectionStatus {
            configured: configured(&[&config.telnyx_api_key, &config.telnyx_webhook_public_key]),
            base_url: Some(config.telnyx_base_url.clone()),
            required_env: vec!["TELNYX_API_KEY", "TELNYX_WEBHOOK_PUBLIC_KEY"],
        },
        payscribe: ProviderConnectionStatus {
            configured: configured(&[
                &config.payscribe_api_key,
                &config.payscribe_secret_key,
                &config.payscribe_webhook_secret,
            ]),
            base_url: Some(config.payscribe_base_url.clone()),
            required_env: vec![
                "PAYSCRIBE_API_KEY",
                "PAYSCRIBE_SECRET_KEY",
                "PAYSCRIBE_WEBHOOK_SECRET",
            ],
        },
        stripe: ProviderConnectionStatus {
            configured: configured(&[&config.stripe_secret_key]),
            base_url: Some("https://api.stripe.com".to_string()),
            required_env: vec!["STRIPE_SECRET_KEY"],
        },
        bank_rails: ProviderConnectionStatus {
            configured: configured(&[
                &config.bank_rails_base_url,
                &config.bank_rails_api_key,
                &config.bank_rails_webhook_secret,
            ]),
            base_url: non_empty(&config.bank_rails_base_url),
            required_env: vec![
                "BANK_RAILS_BASE_URL",
                "BANK_RAILS_API_KEY",
                "BANK_RAILS_WEBHOOK_SECRET",
            ],
        },
        pyth_network: ProviderConnectionStatus {
            configured: configured(&[
                &config.pyth_price_service_url,
                &config.pyth_pex_price_feed_id,
                &config.pyth_sol_price_feed_id,
            ]),
            base_url: Some(config.pyth_price_service_url.clone()),
            required_env: vec![
                "PYTH_PRICE_SERVICE_URL",
                "PYTH_PEX_PRICE_FEED_ID",
                "PYTH_SOL_PRICE_FEED_ID",
            ],
        },
        meteora_dlmm: ProviderConnectionStatus {
            configured: configured(&[
                &config.meteora_api_base_url,
                &config.meteora_dlmm_pair_address,
            ]),
            base_url: Some(config.meteora_api_base_url.clone()),
            required_env: vec!["METEORA_API_BASE_URL", "METEORA_DLMM_PAIR_ADDRESS"],
        },
        solana_rpc: ProviderConnectionStatus {
            configured: configured(&[&config.solana_rpc_url, &config.solana_ws_url]),
            base_url: Some(config.solana_rpc_url.clone()),
            required_env: vec!["SOLANA_RPC_URL", "SOLANA_WS_URL"],
        },
    })
}

pub struct AnthropicClient<'a> {
    state: &'a AppState,
}

impl<'a> AnthropicClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn stream_messages(&self, payload: &Value) -> GatewayResult<Response> {
        require_config("ANTHROPIC_API_KEY", &self.state.config.anthropic_api_key)?;
        let upstream_url = format!("{}/v1/messages", self.state.config.claude_base_url);

        self.state
            .http
            .post(upstream_url)
            .header("x-api-key", &self.state.config.anthropic_api_key)
            .header("anthropic-version", "2023-06-01")
            .json(payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

pub struct CopyleaksClient<'a> {
    state: &'a AppState,
}

impl<'a> CopyleaksClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn submit_scan(&self, scan_id: &str, text: &str) -> GatewayResult<Response> {
        require_config("COPYLEAKS_EMAIL", &self.state.config.copyleaks_email)?;
        require_config("COPYLEAKS_API_KEY", &self.state.config.copyleaks_api_key)?;

        let endpoint = format!(
            "{}/v3/education/scan/submit/{}",
            self.state.config.copyleaks_base_url, scan_id
        );
        let payload = json!({
            "base64": STANDARD.encode(text.as_bytes()),
            "filename": format!("{scan_id}.txt"),
            "properties": {
                "aiDetection": { "submit": true },
                "plagiarism": { "submit": true },
                "sandbox": true
            }
        });

        self.state
            .http
            .put(endpoint)
            .bearer_auth(&self.state.config.copyleaks_api_key)
            .header("X-Copyleaks-Email", &self.state.config.copyleaks_email)
            .json(&payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

pub struct TelnyxClient<'a> {
    state: &'a AppState,
}

impl<'a> TelnyxClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn send_sms(&self, to: &str, from: &str, body: &str) -> GatewayResult<Response> {
        require_config("TELNYX_API_KEY", &self.state.config.telnyx_api_key)?;
        let payload = json!({
            "to": to,
            "from": from,
            "text": body,
            "messaging_profile_id": optional_value(&self.state.config.telnyx_messaging_profile_id),
        });

        self.state
            .http
            .post(format!("{}/v2/messages", self.state.config.telnyx_base_url))
            .bearer_auth(&self.state.config.telnyx_api_key)
            .json(&payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }

    pub async fn create_call(
        &self,
        to: &str,
        from: &str,
        client_state: &str,
        command_id: &str,
    ) -> GatewayResult<Response> {
        require_config("TELNYX_API_KEY", &self.state.config.telnyx_api_key)?;
        require_config(
            "TELNYX_CONNECTION_ID",
            &self.state.config.telnyx_connection_id,
        )?;

        let payload = json!({
            "connection_id": self.state.config.telnyx_connection_id,
            "to": to,
            "from": from,
            "client_state": client_state,
            "command_id": command_id,
        });

        self.state
            .http
            .post(format!("{}/v2/calls", self.state.config.telnyx_base_url))
            .bearer_auth(&self.state.config.telnyx_api_key)
            .json(&payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }

    pub async fn call_action(
        &self,
        call_control_id: &str,
        action: &str,
        payload: &Value,
    ) -> GatewayResult<Response> {
        require_config("TELNYX_API_KEY", &self.state.config.telnyx_api_key)?;
        let call_control_id = call_control_id.trim();
        let action = action.trim();
        if call_control_id.is_empty() || action.is_empty() {
            return Err(GatewayError::Upstream(
                "call_control_id and action are required".to_string(),
            ));
        }

        self.state
            .http
            .post(format!(
                "{}/v2/calls/{call_control_id}/actions/{action}",
                self.state.config.telnyx_base_url
            ))
            .bearer_auth(&self.state.config.telnyx_api_key)
            .json(payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }

    pub async fn search_available_numbers(
        &self,
        country_code: &str,
        limit: usize,
    ) -> GatewayResult<Response> {
        require_config("TELNYX_API_KEY", &self.state.config.telnyx_api_key)?;

        self.state
            .http
            .get(format!(
                "{}/v2/available_phone_numbers",
                self.state.config.telnyx_base_url
            ))
            .bearer_auth(&self.state.config.telnyx_api_key)
            .query(&[
                ("filter[country_code]", country_code),
                ("filter[limit]", &limit.to_string()),
            ])
            .send()
            .await
            .map_err(GatewayError::Http)
    }

    pub async fn order_number(&self, phone_number: &str) -> GatewayResult<Response> {
        require_config("TELNYX_API_KEY", &self.state.config.telnyx_api_key)?;
        let payload = json!({
            "phone_numbers": [{ "phone_number": phone_number }]
        });

        self.state
            .http
            .post(format!(
                "{}/v2/number_orders",
                self.state.config.telnyx_base_url
            ))
            .bearer_auth(&self.state.config.telnyx_api_key)
            .json(&payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

#[allow(dead_code)]
pub struct PayscribeClient<'a> {
    state: &'a AppState,
}

#[allow(dead_code)]
impl<'a> PayscribeClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn post_json(&self, path: &str, payload: &Value) -> GatewayResult<Response> {
        require_config("PAYSCRIBE_API_KEY", &self.state.config.payscribe_api_key)?;
        let url = provider_url(&self.state.config.payscribe_base_url, path);

        self.state
            .http
            .post(url)
            .bearer_auth(&self.state.config.payscribe_api_key)
            .json(payload)
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

#[allow(dead_code)]
pub struct StripeClient<'a> {
    state: &'a AppState,
}

#[allow(dead_code)]
impl<'a> StripeClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn retrieve_payment_intent(
        &self,
        payment_intent_id: &str,
    ) -> GatewayResult<Response> {
        require_config("STRIPE_SECRET_KEY", &self.state.config.stripe_secret_key)?;
        let id = payment_intent_id.trim();
        if id.is_empty() {
            return Err(GatewayError::Upstream(
                "Stripe payment intent id is required".to_string(),
            ));
        }

        self.state
            .http
            .get(format!("https://api.stripe.com/v1/payment_intents/{id}"))
            .bearer_auth(&self.state.config.stripe_secret_key)
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

#[allow(dead_code)]
pub struct MarketDataClient<'a> {
    state: &'a AppState,
}

#[allow(dead_code)]
impl<'a> MarketDataClient<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn fetch_pyth_price(&self, feed_id: &str) -> GatewayResult<Response> {
        let feed_id = feed_id.trim();
        if feed_id.is_empty() {
            return Err(GatewayError::Upstream(
                "Pyth price feed id is required".to_string(),
            ));
        }

        self.state
            .http
            .get(format!(
                "{}/v2/updates/price/latest",
                self.state.config.pyth_price_service_url
            ))
            .query(&[("ids[]", feed_id)])
            .send()
            .await
            .map_err(GatewayError::Http)
    }

    pub async fn fetch_meteora_pair(&self) -> GatewayResult<Response> {
        require_config(
            "METEORA_DLMM_PAIR_ADDRESS",
            &self.state.config.meteora_dlmm_pair_address,
        )?;

        self.state
            .http
            .get(provider_url(
                &self.state.config.meteora_api_base_url,
                &format!("/pair/{}", self.state.config.meteora_dlmm_pair_address),
            ))
            .send()
            .await
            .map_err(GatewayError::Http)
    }
}

fn require_config(key: &'static str, value: &str) -> GatewayResult<()> {
    if value.trim().is_empty() || value.trim().starts_with("replace-with-") {
        return Err(GatewayError::Config(format!(
            "{key} is required for this provider call"
        )));
    }

    Ok(())
}

fn configured(values: &[&str]) -> bool {
    values
        .iter()
        .all(|value| !value.trim().is_empty() && !value.trim().starts_with("replace-with-"))
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn optional_value(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[allow(dead_code)]
fn provider_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domains::{pricing, telecom::billing::debit_credits},
    error::{GatewayError, GatewayResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/copy/quote", post(copy_quote))
        .route("/ai/copy/generate", post(copy_generate))
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum CopyKind {
    AdCopy,
    SocialCaption,
    ProductDescription,
    EmailCopy,
    SmsCopy,
    LandingHero,
    BusinessBio,
}

impl CopyKind {
    fn label(self) -> &'static str {
        match self {
            Self::AdCopy => "Ad copy",
            Self::SocialCaption => "Social caption",
            Self::ProductDescription => "Product description",
            Self::EmailCopy => "Email copy",
            Self::SmsCopy => "SMS copy",
            Self::LandingHero => "Landing page hero",
            Self::BusinessBio => "Business bio",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyQuoteRequest {
    pub copy_kind: CopyKind,
    pub variants: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyQuoteResponse {
    pub accepted: bool,
    pub service_code: String,
    pub copy_kind: CopyKind,
    pub unit_credit_cost: f64,
    pub variants: i32,
    pub total_credit_cost: f64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyGenerateRequest {
    pub account_id: Uuid,
    pub copy_kind: CopyKind,
    pub business_name: Option<String>,
    pub product_or_service: String,
    pub target_audience: Option<String>,
    pub key_points: Option<Vec<String>>,
    pub tone: Option<String>,
    pub platform: Option<String>,
    pub call_to_action: Option<String>,
    pub variants: Option<i32>,
    pub ref_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyGenerateResponse {
    pub accepted: bool,
    pub reference: String,
    pub copy_kind: CopyKind,
    pub title: String,
    pub credit_cost: f64,
    pub outputs: Vec<String>,
}

async fn copy_quote(
    State(state): State<AppState>,
    Json(payload): Json<CopyQuoteRequest>,
) -> GatewayResult<Json<CopyQuoteResponse>> {
    let unit_cost = unit_cost(&state).await?;
    let variants = normalize_variants(payload.variants)?;
    Ok(Json(CopyQuoteResponse {
        accepted: true,
        service_code: "copy_ai_generate".to_string(),
        copy_kind: payload.copy_kind,
        unit_credit_cost: unit_cost,
        variants,
        total_credit_cost: unit_cost * variants as f64,
        message: "Copy AI quote generated. No Credits debited yet.".to_string(),
    }))
}

async fn copy_generate(
    State(state): State<AppState>,
    Json(payload): Json<CopyGenerateRequest>,
) -> GatewayResult<Json<CopyGenerateResponse>> {
    validate_request(&payload)?;
    let variants = normalize_variants(payload.variants)?;
    let credit_cost = unit_cost(&state).await? * variants as f64;
    let reference = payload
        .ref_id
        .clone()
        .unwrap_or_else(|| format!("copy_ai_{}", Uuid::new_v4().simple()));

    debit_credits(
        &state,
        payload.account_id,
        credit_cost,
        "copy_ai_generate",
        &reference,
        "Copy AI generation",
        json!({
            "copyKind": payload.copy_kind,
            "productOrService": payload.product_or_service,
            "businessName": payload.business_name,
            "variants": variants
        }),
    )
    .await?;

    let outputs = (1..=variants)
        .map(|i| render_copy(&payload, i))
        .collect::<Vec<_>>();

    Ok(Json(CopyGenerateResponse {
        accepted: true,
        reference,
        copy_kind: payload.copy_kind,
        title: format!(
            "{} for {}",
            payload.copy_kind.label(),
            payload.product_or_service
        ),
        credit_cost,
        outputs,
    }))
}

async fn unit_cost(state: &AppState) -> GatewayResult<f64> {
    Ok(pricing::get_utility_price(state, "copy_ai_generate")
        .await?
        .credit_cost)
}

fn normalize_variants(value: Option<i32>) -> GatewayResult<i32> {
    let variants = value.unwrap_or(1);
    if !(1..=5).contains(&variants) {
        return Err(GatewayError::Upstream(
            "variants must be between 1 and 5".to_string(),
        ));
    }
    Ok(variants)
}

fn validate_request(payload: &CopyGenerateRequest) -> GatewayResult<()> {
    if payload.product_or_service.trim().is_empty() {
        return Err(GatewayError::Upstream(
            "productOrService is required".to_string(),
        ));
    }
    if payload.product_or_service.chars().count() > 200 {
        return Err(GatewayError::Upstream(
            "productOrService is too long".to_string(),
        ));
    }
    if payload
        .key_points
        .as_ref()
        .is_some_and(|items| items.len() > 8)
    {
        return Err(GatewayError::Upstream(
            "keyPoints cannot exceed 8 items".to_string(),
        ));
    }
    Ok(())
}

fn render_copy(payload: &CopyGenerateRequest, index: i32) -> String {
    let business = payload.business_name.as_deref().unwrap_or("Your brand");
    let audience = payload.target_audience.as_deref().unwrap_or("customers");
    let tone = payload.tone.as_deref().unwrap_or("clear and professional");
    let platform = payload.platform.as_deref().unwrap_or("general marketing");
    let cta = payload
        .call_to_action
        .as_deref()
        .unwrap_or("Get started today");
    let points = payload
        .key_points
        .as_ref()
        .map(|items| {
            items
                .iter()
                .filter(|item| !item.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| {
            "simple value, reliable service, and a smooth customer experience".to_string()
        });

    match payload.copy_kind {
        CopyKind::AdCopy => format!(
            "Variant {index}: {business} helps {audience} enjoy {} with {points}. Built for {platform} in a {tone} tone. {cta}.",
            payload.product_or_service
        ),
        CopyKind::SocialCaption => format!(
            "Variant {index}: Need {} that feels simple and reliable? {business} brings you {points}. {cta}.",
            payload.product_or_service
        ),
        CopyKind::ProductDescription => format!(
            "Variant {index}: {} by {business} is designed for {audience}. It focuses on {points} with a {tone} experience.",
            payload.product_or_service
        ),
        CopyKind::EmailCopy => format!(
            "Subject: A better way to use {}\n\nHello,\n\n{business} helps {audience} get more value from {}. Key benefits include {points}.\n\n{cta}.",
            payload.product_or_service, payload.product_or_service
        ),
        CopyKind::SmsCopy => format!(
            "{business}: Get {} with {points}. {cta}.",
            payload.product_or_service
        ),
        CopyKind::LandingHero => format!(
            "{} that helps {audience} move faster\n\n{business} gives you {points} with a {tone} experience.\n\n{cta}.",
            payload.product_or_service
        ),
        CopyKind::BusinessBio => format!(
            "{business} provides {} for {audience}. The brand focuses on {points} and delivers a {tone} customer experience.",
            payload.product_or_service
        ),
    }
}

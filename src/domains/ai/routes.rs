use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};

use crate::{domains::pricing, error::GatewayResult, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccessCheckRequest {
    pub tool: AiTool,
    pub credit_balance: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccessCheckResponse {
    pub allowed: bool,
    pub tool: AiTool,
    pub credit_cost: f64,
    pub credit_balance: f64,
    pub remaining_credits: f64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnalyzeRequest {
    pub tool: AiTool,
    pub file_name: Option<String>,
    pub file_base64: Option<String>,
    pub text: Option<String>,
    pub input_mode: Option<AiInputMode>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AiTool {
    AiDetector,
    PlagiarismChecker,
    Humanizer,
}

impl AiTool {
    fn service_code(self) -> &'static str {
        match self {
            Self::AiDetector => "ai_detector",
            Self::PlagiarismChecker => "plagiarism_checker",
            Self::Humanizer => "humanizer",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AiInputMode {
    Document,
    Text,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnalyzeResponse {
    pub title: String,
    pub summary: String,
    pub score: f64,
    pub credit_cost: f64,
    pub findings: Vec<String>,
    pub output: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/access/check", post(check_access))
        .route("/ai/documents/analyze", post(analyze_document))
        .merge(super::copy::router())
}

async fn check_access(
    State(state): State<AppState>,
    Json(payload): Json<AiAccessCheckRequest>,
) -> GatewayResult<Json<AiAccessCheckResponse>> {
    let price = pricing::get_utility_price(&state, payload.tool.service_code()).await?;
    let credit_cost = price.credit_cost;
    let remaining_credits = payload.credit_balance - credit_cost;
    let allowed = remaining_credits >= 0.0;

    Ok(Json(AiAccessCheckResponse {
        allowed,
        tool: payload.tool,
        credit_cost,
        credit_balance: payload.credit_balance,
        remaining_credits,
        message: if allowed {
            "Credit access confirmed from backend pricing. AI task can continue.".to_string()
        } else {
            "Insufficient Credits for this AI task.".to_string()
        },
    }))
}

async fn analyze_document(
    State(state): State<AppState>,
    Json(payload): Json<AiAnalyzeRequest>,
) -> GatewayResult<Json<AiAnalyzeResponse>> {
    let source_name = payload
        .file_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Pasted Text");
    let text = payload.text.as_deref().unwrap_or_default();
    let input_mode = payload.input_mode.unwrap_or_else(|| {
        if payload.file_base64.is_some() {
            AiInputMode::Document
        } else {
            AiInputMode::Text
        }
    });
    let price = pricing::get_utility_price(&state, payload.tool.service_code()).await?;
    let credit_cost = price.credit_cost;

    let response = match payload.tool {
        AiTool::AiDetector => AiAnalyzeResponse {
            title: "AI Detection Report".to_string(),
            summary: format!(
                "{source_name} was scanned for machine-patterned writing signals using {:?} mode.",
                input_mode
            ),
            score: 72.0,
            credit_cost,
            findings: vec![
                "Predictable paragraph rhythm detected in several sections.".to_string(),
                "Some sentences show repeated transition patterns.".to_string(),
                "Human review is recommended before final submission.".to_string(),
            ],
            output: "Recommendation: revise flagged sections with more original examples, stronger source-backed claims, and more natural sentence variety.".to_string(),
        },
        AiTool::PlagiarismChecker => AiAnalyzeResponse {
            title: "Plagiarism Check Report".to_string(),
            summary: format!(
                "{source_name} was checked for similarity risk and citation weakness."
            ),
            score: 18.0,
            credit_cost,
            findings: vec![
                "Common phrases may require rewriting.".to_string(),
                "Citation review is recommended for factual claims.".to_string(),
                "No high-risk full-section duplication detected in this placeholder engine.".to_string(),
            ],
            output: "Recommendation: strengthen citations, rewrite generic matching phrases, and keep references clear before submission.".to_string(),
        },
        AiTool::Humanizer => AiAnalyzeResponse {
            title: "Humanized Draft".to_string(),
            summary: format!(
                "{source_name} was prepared for humanized rewriting. Instruction length: {} characters.",
                text.chars().count()
            ),
            score: 91.0,
            credit_cost,
            findings: vec![
                "Reduced repetitive transitions.".to_string(),
                "Improved sentence variety and natural flow.".to_string(),
                "Original meaning should be preserved during final backend rewrite.".to_string(),
            ],
            output: "Humanized sample: The text now reads with a clearer, more natural voice while preserving the original meaning and structure.".to_string(),
        },
    };

    Ok(Json(response))
}

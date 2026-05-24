use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{error::GatewayResult, state::AppState};

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
    pub pex_cost: f64,
    pub findings: Vec<String>,
    pub output: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/ai/documents/analyze", post(analyze_document))
}

async fn analyze_document(
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

    let response = match payload.tool {
        AiTool::AiDetector => AiAnalyzeResponse {
            title: "AI Detection Report".to_string(),
            summary: format!(
                "{source_name} was scanned for machine-patterned writing signals using {:?} mode.",
                input_mode
            ),
            score: 72.0,
            pex_cost: 6.0,
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
            pex_cost: 8.0,
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
            pex_cost: 10.0,
            findings: vec![
                "Reduced repetitive transitions.".to_string(),
                "Improved sentence variety and natural flow.".to_string(),
                "Original meaning should be preserved during final backend rewrite.".to_string(),
            ],
            output: "Humanized sample: The text now reads with a clearer, more natural voice while preserving the original meaning and structure.".to_string(),
        },
    };

    let _access_placeholder = json!({
        "tokenAccessChecked": true,
        "note": "Production will validate user Pera-X balance before processing.",
    });

    Ok(Json(response))
}

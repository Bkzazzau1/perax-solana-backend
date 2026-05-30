use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domains::{pricing, telecom::billing::debit_credits},
    error::{GatewayError, GatewayResult},
    state::AppState,
};

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
    pub account_id: Uuid,
    pub tool: AiTool,
    pub file_name: Option<String>,
    pub file_base64: Option<String>,
    pub text: Option<String>,
    pub input_mode: Option<AiInputMode>,
    pub ref_id: Option<String>,
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

    fn title(self) -> &'static str {
        match self {
            Self::AiDetector => "AI Detection Report",
            Self::PlagiarismChecker => "Plagiarism Check Report",
            Self::Humanizer => "Humanized Draft",
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
    pub reference: String,
    pub engine: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/access/check", post(check_access))
        .route("/ai/documents/analyze", post(analyze_document))
        .merge(super::copy::router())
        .merge(super::copyleaks::router())
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
    let text = payload.text.as_deref().unwrap_or_default().trim().to_string();
    if text.is_empty() && payload.file_base64.is_none() {
        return Err(GatewayError::Upstream("text or fileBase64 is required".to_string()));
    }

    let input_mode = payload.input_mode.unwrap_or_else(|| {
        if payload.file_base64.is_some() {
            AiInputMode::Document
        } else {
            AiInputMode::Text
        }
    });
    let price = pricing::get_utility_price(&state, payload.tool.service_code()).await?;
    let credit_cost = price.credit_cost;
    let reference = payload
        .ref_id
        .clone()
        .unwrap_or_else(|| format!("{}_{}", payload.tool.service_code(), Uuid::new_v4().simple()));

    debit_credits(
        &state,
        payload.account_id,
        credit_cost,
        payload.tool.service_code(),
        &reference,
        payload.tool.title(),
        json!({
            "tool": payload.tool,
            "sourceName": source_name,
            "inputMode": input_mode,
            "textLength": text.chars().count()
        }),
    )
    .await?;

    let response = match payload.tool {
        AiTool::AiDetector => run_ai_detector(source_name, &text, input_mode, credit_cost, reference),
        AiTool::PlagiarismChecker => run_plagiarism_checker(source_name, &text, credit_cost, reference),
        AiTool::Humanizer => run_humanizer(source_name, &text, credit_cost, reference),
    };

    Ok(Json(response))
}

fn run_ai_detector(source_name: &str, text: &str, input_mode: AiInputMode, credit_cost: f64, reference: String) -> AiAnalyzeResponse {
    let score = ai_pattern_score(text);
    let mut findings = Vec::new();
    if score >= 70.0 {
        findings.push("High machine-pattern risk detected from repetitive sentence rhythm and generic transitions.".to_string());
        findings.push("Add more personal examples, concrete evidence, and varied sentence structure.".to_string());
    } else if score >= 40.0 {
        findings.push("Moderate AI-writing signals detected. Some parts may need more natural rewriting.".to_string());
        findings.push("Improve specificity and reduce repeated phrasing.".to_string());
    } else {
        findings.push("Low AI-writing signal based on the MVP detector heuristic.".to_string());
        findings.push("Final review is still recommended before submission.".to_string());
    }

    AiAnalyzeResponse {
        title: "AI Detection Report".to_string(),
        summary: format!("{source_name} was scanned for machine-patterned writing signals using {:?} mode.", input_mode),
        score,
        credit_cost,
        findings,
        output: "Recommendation: use this as an early screening report only. Final production accuracy will improve when the live detector provider is connected.".to_string(),
        reference,
        engine: "heuristic_mvp".to_string(),
    }
}

fn run_plagiarism_checker(source_name: &str, text: &str, credit_cost: f64, reference: String) -> AiAnalyzeResponse {
    let score = plagiarism_risk_score(text);
    let findings = vec![
        "Checked for repeated generic phrases, suspicious repetition, and citation weakness.".to_string(),
        "Use /ai/copyleaks/submit for premium plagiarism and historical-alignment scan.".to_string(),
        if score >= 50.0 { "Risk is elevated. Rewrite generic sections and add citations for factual claims.".to_string() } else { "No high-risk pattern found by the MVP checker, but source matching is not yet live.".to_string() },
    ];

    AiAnalyzeResponse {
        title: "Plagiarism Check Report".to_string(),
        summary: format!("{source_name} was checked for similarity risk and citation weakness."),
        score,
        credit_cost,
        findings,
        output: "Recommendation: use Copyleaks premium scan for production-grade plagiarism and historical-alignment checking.".to_string(),
        reference,
        engine: "heuristic_mvp".to_string(),
    }
}

fn run_humanizer(source_name: &str, text: &str, credit_cost: f64, reference: String) -> AiAnalyzeResponse {
    let humanized = humanize_text(text);
    AiAnalyzeResponse {
        title: "Humanized Draft".to_string(),
        summary: format!("{source_name} was rewritten with a clearer and more natural voice."),
        score: 91.0,
        credit_cost,
        findings: vec![
            "Reduced repetitive transitions where possible.".to_string(),
            "Improved flow using shorter and more direct wording.".to_string(),
            "Meaning should be reviewed by the user before final use.".to_string(),
        ],
        output: humanized,
        reference,
        engine: "rewrite_mvp".to_string(),
    }
}

fn ai_pattern_score(text: &str) -> f64 {
    let sentence_count = text.matches('.').count().max(1) as f64;
    let word_count = text.split_whitespace().count() as f64;
    let avg_sentence = word_count / sentence_count;
    let generic_markers = ["moreover", "furthermore", "in conclusion", "it is important", "overall", "delve", "landscape"];
    let marker_hits = generic_markers.iter().filter(|marker| text.to_lowercase().contains(**marker)).count() as f64;
    let score = 25.0 + marker_hits * 12.0 + if avg_sentence > 24.0 { 20.0 } else { 5.0 };
    score.clamp(5.0, 95.0)
}

fn plagiarism_risk_score(text: &str) -> f64 {
    let words = text.split_whitespace().map(|word| word.to_lowercase()).collect::<Vec<_>>();
    if words.is_empty() { return 0.0; }
    let unique = words.iter().collect::<std::collections::HashSet<_>>().len() as f64;
    let repetition_ratio = 1.0 - (unique / words.len() as f64);
    (repetition_ratio * 100.0).clamp(0.0, 90.0)
}

fn humanize_text(text: &str) -> String {
    if text.trim().is_empty() {
        return "No text was provided for humanizing.".to_string();
    }
    text.replace("Furthermore,", "Also,")
        .replace("Moreover,", "Also,")
        .replace("In conclusion,", "To conclude,")
        .replace("It is important to note that", "Importantly,")
}

use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityCatalogResponse {
    pub title: &'static str,
    pub description: &'static str,
    pub services: Vec<UtilityService>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityService {
    pub code: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub route: &'static str,
    pub credit_unit: &'static str,
    pub status: &'static str,
}

pub fn router() -> Router<crate::state::AppState> {
    Router::new().route("/utility/catalog", get(utility_catalog))
}

async fn utility_catalog() -> Json<UtilityCatalogResponse> {
    Json(UtilityCatalogResponse {
        title: "Pera-X Utility Services",
        description: "Service catalog for the Pera-X app. Users spend Credits on supported utilities while PEX remains the ecosystem asset.",
        services: vec![
            UtilityService {
                code: "AI_LAB",
                name: "AI Lab",
                category: "AI Tools",
                description: "AI detection, plagiarism checks, humanizer tools, document intelligence, and future AI services.",
                route: "/ai-lab",
                credit_unit: "AI Credits",
                status: "active",
            },
            UtilityService {
                code: "CALLS",
                name: "International Calls",
                category: "Communication",
                description: "App-to-phone calls where receivers do not need the Pera-X app or internet access.",
                route: "/pera-x/calls",
                credit_unit: "Call Credits",
                status: "active",
            },
            UtilityService {
                code: "SMS",
                name: "SMS Messaging",
                category: "Communication",
                description: "Personal SMS, OTP, bulk messaging, alerts, campaigns, and developer SMS APIs.",
                route: "/pera-x/sms-inbox",
                credit_unit: "SMS Units",
                status: "active",
            },
            UtilityService {
                code: "NUMBERS",
                name: "Foreign Numbers",
                category: "Communication",
                description: "Buy, manage, renew, cancel, and reactivate international phone numbers.",
                route: "/pera-x/buy-number",
                credit_unit: "Number Credits",
                status: "active",
            },
            UtilityService {
                code: "BILLS",
                name: "Bills Payment",
                category: "Utilities",
                description: "Electricity, TV, internet, water, waste, institutional bills, and other approved bill payments.",
                route: "/bills",
                credit_unit: "Bill Credits",
                status: "planned",
            },
            UtilityService {
                code: "WEB_TOOLS",
                name: "Website Tools",
                category: "Web Services",
                description: "AI-generated websites, landing pages, and build-credit tools for small businesses and creators.",
                route: "/market",
                credit_unit: "Build Credits",
                status: "planned",
            },
        ],
    })
}

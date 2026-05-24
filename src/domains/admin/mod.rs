use axum::{
    Json, Router,
    extract::{Query, State},
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    common::crypto::{generate_virtual_key, hash_api_key, key_prefix},
    domains::solana::policy::{DailyBurnDecision, MarketPolicyInput, calculate_daily_burn_decision},
    error::GatewayResult,
    infra::cache::{self, CacheStore},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin", get(admin_panel))
        .route("/admin/api/status", get(status))
        .route("/admin/api/burn-preview", get(burn_preview))
        .route("/admin/api/dev-key", post(create_dev_key))
}

async fn admin_panel() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

#[derive(Debug, Serialize)]
struct AdminStatus {
    service: &'static str,
    health: &'static str,
    cache_mode: &'static str,
    anchor_workspace: String,
    perax_program_id: String,
    accounts: i64,
    api_keys: i64,
    routes: Vec<&'static str>,
}

async fn status(State(state): State<AppState>) -> GatewayResult<Json<AdminStatus>> {
    let accounts = sqlx::query_scalar::<_, i64>("select count(*) from accounts")
        .fetch_one(&state.db)
        .await?;
    let api_keys =
        sqlx::query_scalar::<_, i64>("select count(*) from api_keys where revoked_at is null")
            .fetch_one(&state.db)
            .await?;

    Ok(Json(AdminStatus {
        service: "Pera-X Utility Gateway",
        health: "ok",
        cache_mode: match &state.cache {
            CacheStore::Redis(_) => "redis",
            CacheStore::Memory(_) => "memory",
        },
        anchor_workspace: state.config.perax_anchor_workspace.clone(),
        perax_program_id: state.config.perax_program_id.clone(),
        accounts,
        api_keys,
        routes: vec![
            "GET /healthz",
            "GET /admin",
            "GET /admin/api/status",
            "GET /admin/api/burn-preview",
            "POST /admin/api/dev-key",
            "POST /v1/proxy/claude/messages",
            "POST /v1/proxy/copyleaks/scan",
            "GET /telecom/call/{id}",
            "POST /telecom/webrtc/offer",
            "POST /telecom/sms",
            "GET /telecom/numbers/search",
            "POST /telecom/numbers/buy",
        ],
    }))
}

#[derive(Debug, Deserialize)]
struct BurnPreviewQuery {
    market_health_score: Option<f64>,
    liquidity_score: Option<f64>,
    utility_usage_score: Option<f64>,
    holder_pressure_score: Option<f64>,
    trading_company_wallet_score: Option<f64>,
}

#[derive(Debug, Serialize)]
struct BurnPreviewResponse {
    policy: &'static str,
    min_burn_percent: f64,
    max_burn_percent: f64,
    input: MarketPolicyInput,
    decision: DailyBurnDecision,
}

async fn burn_preview(Query(query): Query<BurnPreviewQuery>) -> Json<BurnPreviewResponse> {
    let defaults = MarketPolicyInput::default();
    let input = MarketPolicyInput {
        market_health_score: query
            .market_health_score
            .unwrap_or(defaults.market_health_score),
        liquidity_score: query.liquidity_score.unwrap_or(defaults.liquidity_score),
        utility_usage_score: query
            .utility_usage_score
            .unwrap_or(defaults.utility_usage_score),
        holder_pressure_score: query
            .holder_pressure_score
            .unwrap_or(defaults.holder_pressure_score),
        trading_company_wallet_score: query
            .trading_company_wallet_score
            .unwrap_or(defaults.trading_company_wallet_score),
    };

    let decision = calculate_daily_burn_decision(input.clone());

    Json(BurnPreviewResponse {
        policy: "Pera-X dynamic daily burn policy",
        min_burn_percent: 2.0,
        max_burn_percent: 30.0,
        input,
        decision,
    })
}

#[derive(Debug, Deserialize)]
struct CreateDevKeyRequest {
    name: Option<String>,
    credits: Option<f64>,
}

#[derive(Debug, Serialize)]
struct CreateDevKeyResponse {
    account_id: Uuid,
    api_key: String,
    key_prefix: String,
    credits: f64,
    sample_curl: serde_json::Value,
}

async fn create_dev_key(
    State(state): State<AppState>,
    Json(request): Json<CreateDevKeyRequest>,
) -> GatewayResult<Json<CreateDevKeyResponse>> {
    let name = request
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Local Dev Account".to_string());
    let credits = request.credits.unwrap_or(1_000.0);

    let account_id = sqlx::query_scalar::<_, Uuid>(
        "insert into accounts (name, credit_balance) values ($1, $2) returning id",
    )
    .bind(name)
    .bind(credits.round() as i64)
    .fetch_one(&state.db)
    .await?;

    let api_key = generate_virtual_key();
    let prefix = key_prefix(&api_key);
    let hash = hash_api_key(&api_key);

    sqlx::query("insert into api_keys (account_id, key_prefix, key_hash) values ($1, $2, $3)")
        .bind(account_id)
        .bind(&prefix)
        .bind(&hash)
        .execute(&state.db)
        .await?;

    let cache_key = format!("client:balance:{account_id}");
    cache::set_credits(&state.cache, &cache_key, credits).await?;

    Ok(Json(CreateDevKeyResponse {
        account_id,
        api_key: api_key.clone(),
        key_prefix: prefix,
        credits,
        sample_curl: json!({
            "health": "curl http://127.0.0.1:8080/healthz",
            "burn_preview": "curl http://127.0.0.1:8080/admin/api/burn-preview",
            "copyleaks": format!("curl -X POST http://127.0.0.1:8080/v1/proxy/copyleaks/scan -H \"Authorization: Bearer {api_key}\" -H \"Content-Type: application/json\" -d \"{{\\\"text\\\":\\\"hello from pera x local admin\\\"}}\"")
        }),
    }))
}

const ADMIN_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Pera-X Admin</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #101317;
      --panel: #181d24;
      --line: #2b333f;
      --text: #edf2f7;
      --muted: #9aa7b5;
      --accent: #38d39f;
      --warn: #f7b955;
      --bad: #ff6b6b;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 20px 28px;
      border-bottom: 1px solid var(--line);
      background: #12171d;
    }
    h1 { margin: 0; font-size: 20px; letter-spacing: 0; }
    main {
      display: grid;
      grid-template-columns: 280px 1fr;
      min-height: calc(100vh - 70px);
    }
    nav {
      border-right: 1px solid var(--line);
      padding: 20px;
      background: #11161c;
    }
    nav a {
      display: block;
      color: var(--muted);
      text-decoration: none;
      padding: 10px 12px;
      border-radius: 6px;
      margin-bottom: 4px;
    }
    nav a.active, nav a:hover { color: var(--text); background: #1b222b; }
    section { padding: 24px 28px; }
    .grid {
      display: grid;
      grid-template-columns: repeat(4, minmax(150px, 1fr));
      gap: 12px;
      margin-bottom: 20px;
    }
    .card {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 16px;
    }
    .label { color: var(--muted); font-size: 12px; text-transform: uppercase; }
    .value { font-size: 24px; margin-top: 6px; font-weight: 700; }
    .ok { color: var(--accent); }
    .warn { color: var(--warn); }
    button, input {
      height: 38px;
      border-radius: 6px;
      border: 1px solid var(--line);
      background: #202833;
      color: var(--text);
      padding: 0 12px;
    }
    button { cursor: pointer; background: #1f7a5b; border-color: #2ca878; font-weight: 700; }
    button:hover { background: #258a68; }
    .row { display: flex; gap: 10px; flex-wrap: wrap; align-items: center; }
    pre {
      overflow: auto;
      background: #0b0e12;
      border: 1px solid var(--line);
      border-radius: 8px;
      padding: 14px;
      color: #d9e6f2;
      min-height: 80px;
    }
    table { width: 100%; border-collapse: collapse; }
    td { border-bottom: 1px solid var(--line); padding: 10px 8px; color: var(--muted); }
    td:first-child { color: var(--text); width: 280px; }
    @media (max-width: 860px) {
      main { grid-template-columns: 1fr; }
      nav { border-right: 0; border-bottom: 1px solid var(--line); }
      .grid { grid-template-columns: 1fr 1fr; }
    }
  </style>
</head>
<body>
  <header>
    <h1>Pera-X Utility Gateway</h1>
    <div id="clock" class="label"></div>
  </header>
  <main>
    <nav>
      <a class="active" href="/admin">Overview</a>
      <a href="/healthz">Health</a>
      <a href="/admin/api/status">Status JSON</a>
      <a href="/admin/api/burn-preview">Burn Preview</a>
    </nav>
    <section>
      <div class="grid">
        <div class="card"><div class="label">Health</div><div id="health" class="value ok">...</div></div>
        <div class="card"><div class="label">Cache</div><div id="cache" class="value">...</div></div>
        <div class="card"><div class="label">Accounts</div><div id="accounts" class="value">...</div></div>
        <div class="card"><div class="label">API Keys</div><div id="keys" class="value">...</div></div>
      </div>

      <div class="card">
        <h2>Local Dev Key</h2>
        <div class="row">
          <input id="name" value="Local Dev Account" aria-label="Account name">
          <input id="credits" type="number" value="1000" aria-label="Credits">
          <button id="create">Generate Key</button>
        </div>
        <pre id="result">Generate a local key to test protected gateway routes.</pre>
      </div>

      <div class="card" style="margin-top: 16px;">
        <h2>Mounted Routes</h2>
        <table id="routes"></table>
      </div>
    </section>
  </main>
  <script>
    const $ = (id) => document.getElementById(id);

    async function loadStatus() {
      const res = await fetch('/admin/api/status');
      const data = await res.json();
      $('health').textContent = data.health;
      $('cache').textContent = data.cache_mode;
      $('cache').className = 'value ' + (data.cache_mode === 'memory' ? 'warn' : 'ok');
      $('accounts').textContent = data.accounts;
      $('keys').textContent = data.api_keys;
      $('routes').innerHTML = data.routes.map((route) => `<tr><td>${route}</td><td>mounted</td></tr>`).join('');
    }

    $('create').addEventListener('click', async () => {
      $('result').textContent = 'Generating...';
      const res = await fetch('/admin/api/dev-key', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          name: $('name').value,
          credits: Number($('credits').value || 1000)
        })
      });
      const data = await res.json();
      $('result').textContent = JSON.stringify(data, null, 2);
      await loadStatus();
    });

    setInterval(() => $('clock').textContent = new Date().toLocaleString(), 1000);
    loadStatus().catch((err) => $('result').textContent = String(err));
  </script>
</body>
</html>"#;

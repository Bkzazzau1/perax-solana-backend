use uuid::Uuid;

use crate::{
    common::crypto::{generate_virtual_key, hash_api_key, key_prefix},
    domains::auth::middleware::AuthenticatedAccount,
    error::{GatewayError, GatewayResult},
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct EmailWalletSession {
    pub account_id: Uuid,
    pub email: String,
    pub pex_wallet_address: String,
    pub api_key: String,
    pub key_prefix: String,
    pub created: bool,
}

pub async fn verify_virtual_key(
    state: &AppState,
    api_key: &str,
) -> GatewayResult<AuthenticatedAccount> {
    if !api_key.starts_with("sk_perax_") {
        return Err(GatewayError::Unauthorized);
    }

    let key_hash = hash_api_key(api_key);
    let prefix = key_prefix(api_key);

    let account_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        select account_id
        from api_keys
        where key_prefix = $1 and key_hash = $2 and revoked_at is null
        "#,
    )
    .bind(prefix)
    .bind(key_hash)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GatewayError::Unauthorized)?;

    Ok(AuthenticatedAccount { account_id })
}

pub async fn signup_or_login_with_email_wallet(
    state: &AppState,
    email: &str,
    pex_wallet_address: &str,
) -> GatewayResult<EmailWalletSession> {
    let email = normalize_email(email)?;
    let pex_wallet_address = normalize_pex_wallet(pex_wallet_address)?;

    let existing = find_account_by_email_or_wallet(state, &email, &pex_wallet_address).await?;
    if let Some(account) = existing {
        ensure_account_matches(&account, &email, &pex_wallet_address)?;
        return issue_email_wallet_session(state, account.id, email, pex_wallet_address, false)
            .await;
    }

    let account_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into accounts (name, email, pex_wallet_address)
        values ($1, $2, $3)
        returning id
        "#,
    )
    .bind(&email)
    .bind(&email)
    .bind(&pex_wallet_address)
    .fetch_one(&state.db)
    .await?;

    issue_email_wallet_session(state, account_id, email, pex_wallet_address, true).await
}

pub async fn login_with_email_wallet(
    state: &AppState,
    email: &str,
    pex_wallet_address: &str,
) -> GatewayResult<EmailWalletSession> {
    let email = normalize_email(email)?;
    let pex_wallet_address = normalize_pex_wallet(pex_wallet_address)?;

    let account = sqlx::query_as::<_, AccountIdentity>(
        r#"
        select id, email, pex_wallet_address
        from accounts
        where lower(email) = $1
        limit 1
        "#,
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GatewayError::Unauthorized)?;

    ensure_account_matches(&account, &email, &pex_wallet_address)?;
    issue_email_wallet_session(state, account.id, email, pex_wallet_address, false).await
}

#[derive(Debug, sqlx::FromRow)]
struct AccountIdentity {
    id: Uuid,
    email: Option<String>,
    pex_wallet_address: Option<String>,
}

async fn find_account_by_email_or_wallet(
    state: &AppState,
    email: &str,
    pex_wallet_address: &str,
) -> GatewayResult<Option<AccountIdentity>> {
    let account = sqlx::query_as::<_, AccountIdentity>(
        r#"
        select id, email, pex_wallet_address
        from accounts
        where lower(email) = $1 or pex_wallet_address = $2
        order by created_at asc
        limit 1
        "#,
    )
    .bind(email)
    .bind(pex_wallet_address)
    .fetch_optional(&state.db)
    .await?;

    Ok(account)
}

async fn issue_email_wallet_session(
    state: &AppState,
    account_id: Uuid,
    email: String,
    pex_wallet_address: String,
    created: bool,
) -> GatewayResult<EmailWalletSession> {
    let api_key = generate_virtual_key();
    let prefix = key_prefix(&api_key);
    let hash = hash_api_key(&api_key);

    sqlx::query("insert into api_keys (account_id, key_prefix, key_hash) values ($1, $2, $3)")
        .bind(account_id)
        .bind(&prefix)
        .bind(&hash)
        .execute(&state.db)
        .await?;

    Ok(EmailWalletSession {
        account_id,
        email,
        pex_wallet_address,
        api_key,
        key_prefix: prefix,
        created,
    })
}

fn ensure_account_matches(
    account: &AccountIdentity,
    email: &str,
    pex_wallet_address: &str,
) -> GatewayResult<()> {
    let account_email = account
        .email
        .as_deref()
        .map(str::to_lowercase)
        .ok_or(GatewayError::Unauthorized)?;
    let account_wallet = account
        .pex_wallet_address
        .as_deref()
        .ok_or(GatewayError::Unauthorized)?;

    if account_email != email || account_wallet != pex_wallet_address {
        return Err(GatewayError::Unauthorized);
    }

    Ok(())
}

fn normalize_email(email: &str) -> GatewayResult<String> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| GatewayError::BadRequest("email address is invalid".to_string()))?;

    if email.len() > 254
        || local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || email.chars().any(char::is_whitespace)
    {
        return Err(GatewayError::BadRequest(
            "email address is invalid".to_string(),
        ));
    }

    Ok(email)
}

fn normalize_pex_wallet(pex_wallet_address: &str) -> GatewayResult<String> {
    let wallet = pex_wallet_address.trim();
    if wallet.is_empty() {
        return Err(GatewayError::BadRequest(
            "PEX wallet address is required".to_string(),
        ));
    }

    let decoded = bs58::decode(wallet)
        .into_vec()
        .map_err(|_| GatewayError::BadRequest("PEX wallet address is invalid".to_string()))?;

    if decoded.len() != 32 {
        return Err(GatewayError::BadRequest(
            "PEX wallet address must be a valid Solana wallet address".to_string(),
        ));
    }

    Ok(wallet.to_string())
}

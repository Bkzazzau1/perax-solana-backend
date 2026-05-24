use uuid::Uuid;

use crate::{
    common::crypto::{hash_api_key, key_prefix},
    domains::auth::middleware::AuthenticatedAccount,
    error::{GatewayError, GatewayResult},
    state::AppState,
};

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

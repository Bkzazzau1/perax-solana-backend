use sha2::{Digest, Sha256};
use uuid::Uuid;

#[allow(dead_code)]
pub fn generate_virtual_key() -> String {
    format!("sk_perax_{}", Uuid::new_v4().simple())
}

pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn key_prefix(api_key: &str) -> String {
    api_key.chars().take(16).collect()
}

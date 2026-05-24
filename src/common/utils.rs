use chrono::{DateTime, Utc};

#[allow(dead_code)]
pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

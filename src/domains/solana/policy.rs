use serde::{Deserialize, Serialize};

pub const MIN_BURN_RATE_BPS: u16 = 200;
pub const DEFAULT_BURN_RATE_BPS: u16 = 1_000;
pub const MAX_BURN_RATE_BPS: u16 = 3_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPolicyInput {
    /// 0.0 = weak market, 1.0 = very healthy market.
    pub market_health_score: f64,
    /// 0.0 = poor liquidity, 1.0 = deep liquidity.
    pub liquidity_score: f64,
    /// 0.0 = little real utility, 1.0 = strong real utility.
    pub utility_usage_score: f64,
    /// 0.0 = low sell/holder pressure, 1.0 = high holder pressure.
    pub holder_pressure_score: f64,
    /// 0.0 = weak revenue wallet, 1.0 = strong revenue wallet.
    pub trading_company_wallet_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBurnDecision {
    pub burn_rate: f64,
    pub burn_rate_bps: u16,
    pub burn_rate_percent: f64,
    /// Smart-contract compatible score from 0 to 100.
    pub market_health_score_u8: u8,
    pub market_health_score: f64,
    pub liquidity_score: f64,
    pub utility_usage_score: f64,
    pub holder_pressure_score: f64,
    pub trading_company_wallet_score: f64,
    pub reason: String,
}

impl Default for MarketPolicyInput {
    fn default() -> Self {
        Self {
            market_health_score: 0.60,
            liquidity_score: 0.60,
            utility_usage_score: 0.60,
            holder_pressure_score: 0.40,
            trading_company_wallet_score: 0.60,
        }
    }
}

pub fn calculate_daily_burn_decision(input: MarketPolicyInput) -> DailyBurnDecision {
    let market_health = clamp01(input.market_health_score);
    let liquidity = clamp01(input.liquidity_score);
    let utility_usage = clamp01(input.utility_usage_score);
    let holder_pressure = clamp01(input.holder_pressure_score);
    let trading_wallet = clamp01(input.trading_company_wallet_score);

    // Higher score means healthier market and therefore lower burn.
    // Holder pressure reduces health. Revenue wallet strength is included softly so the
    // backend avoids aggressive burn when the revenue wallet is still too small.
    let composite_health = weighted_average(&[
        (market_health, 0.35),
        (liquidity, 0.20),
        (utility_usage, 0.25),
        (1.0 - holder_pressure, 0.15),
        (trading_wallet, 0.05),
    ]);

    let market_health_score_u8 = (composite_health * 100.0).round().clamp(0.0, 100.0) as u8;
    let burn_rate_bps = burn_rate_bps_for_market_health(market_health_score_u8);
    let burn_rate = burn_rate_bps as f64 / 10_000.0;

    let reason = match market_health_score_u8 {
        0..=20 => "Critical weak-market condition: maximum 30% burn policy activated.".to_string(),
        21..=30 => "Weak-market condition: 25% burn policy activated.".to_string(),
        31..=45 => "Moderate weakness: 20% burn policy activated.".to_string(),
        46..=60 => "Normal market condition: default 10% burn policy activated.".to_string(),
        61..=75 => "Healthy market: reduced 8% burn policy activated.".to_string(),
        76..=85 => "Strong market: low 5% burn policy activated.".to_string(),
        86..=100 => "Very strong market: minimum 2% burn policy activated.".to_string(),
        _ => "Invalid market health score.".to_string(),
    };

    DailyBurnDecision {
        burn_rate,
        burn_rate_bps,
        burn_rate_percent: burn_rate * 100.0,
        market_health_score_u8,
        market_health_score: market_health,
        liquidity_score: liquidity,
        utility_usage_score: utility_usage,
        holder_pressure_score: holder_pressure,
        trading_company_wallet_score: trading_wallet,
        reason,
    }
}

pub fn burn_rate_bps_for_market_health(score: u8) -> u16 {
    match score {
        0..=20 => MAX_BURN_RATE_BPS,
        21..=30 => 2_500,
        31..=45 => 2_000,
        46..=60 => DEFAULT_BURN_RATE_BPS,
        61..=75 => 800,
        76..=85 => 500,
        86..=100 => MIN_BURN_RATE_BPS,
        _ => MIN_BURN_RATE_BPS,
    }
}

fn weighted_average(items: &[(f64, f64)]) -> f64 {
    let total_weight: f64 = items.iter().map(|(_, weight)| weight).sum();
    if total_weight <= 0.0 {
        return 0.0;
    }

    items
        .iter()
        .map(|(value, weight)| clamp01(*value) * weight)
        .sum::<f64>()
        / total_weight
}

fn clamp01(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burn_rate_is_always_inside_policy_range() {
        let decision = calculate_daily_burn_decision(MarketPolicyInput {
            market_health_score: -10.0,
            liquidity_score: 5.0,
            utility_usage_score: f64::NAN,
            holder_pressure_score: 2.0,
            trading_company_wallet_score: 1.0,
        });

        assert!(decision.burn_rate_bps >= MIN_BURN_RATE_BPS);
        assert!(decision.burn_rate_bps <= MAX_BURN_RATE_BPS);
    }

    #[test]
    fn normal_market_uses_default_ten_percent_burn() {
        let decision = calculate_daily_burn_decision(MarketPolicyInput::default());
        assert_eq!(decision.burn_rate_bps, DEFAULT_BURN_RATE_BPS);
    }

    #[test]
    fn weak_market_uses_higher_burn_than_healthy_market() {
        let healthy = calculate_daily_burn_decision(MarketPolicyInput {
            market_health_score: 0.9,
            liquidity_score: 0.9,
            utility_usage_score: 0.9,
            holder_pressure_score: 0.1,
            trading_company_wallet_score: 0.7,
        });

        let pressured = calculate_daily_burn_decision(MarketPolicyInput {
            market_health_score: 0.3,
            liquidity_score: 0.4,
            utility_usage_score: 0.2,
            holder_pressure_score: 0.95,
            trading_company_wallet_score: 0.7,
        });

        assert!(pressured.burn_rate_bps > healthy.burn_rate_bps);
    }

    #[test]
    fn smart_contract_rate_table_matches_backend() {
        assert_eq!(burn_rate_bps_for_market_health(10), 3_000);
        assert_eq!(burn_rate_bps_for_market_health(25), 2_500);
        assert_eq!(burn_rate_bps_for_market_health(40), 2_000);
        assert_eq!(burn_rate_bps_for_market_health(55), 1_000);
        assert_eq!(burn_rate_bps_for_market_health(70), 800);
        assert_eq!(burn_rate_bps_for_market_health(80), 500);
        assert_eq!(burn_rate_bps_for_market_health(95), 200);
    }
}

use serde::{Deserialize, Serialize};

const MIN_BURN_RATE: f64 = 0.02;
const MAX_BURN_RATE: f64 = 0.30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPolicyInput {
    /// 0.0 = weak market, 1.0 = very healthy market.
    pub market_health_score: f64,
    /// 0.0 = poor liquidity, 1.0 = deep liquidity.
    pub liquidity_score: f64,
    /// 0.0 = little real utility, 1.0 = strong real utility.
    pub utility_usage_score: f64,
    /// 0.0 = very few holders, 1.0 = many holders relative to users.
    pub holder_pressure_score: f64,
    /// 0.0 = weak trading company wallet, 1.0 = strong trading company wallet.
    pub trading_company_wallet_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBurnDecision {
    pub burn_rate: f64,
    pub burn_rate_percent: f64,
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
            market_health_score: 0.70,
            liquidity_score: 0.60,
            utility_usage_score: 0.50,
            holder_pressure_score: 0.50,
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

    // Higher burn pressure when utility is low, holder pressure is high,
    // liquidity is weak, market health is weak, or the trading company wallet is strong enough to support burns.
    let weak_market_pressure = 1.0 - market_health;
    let weak_liquidity_pressure = 1.0 - liquidity;
    let low_utility_pressure = 1.0 - utility_usage;
    let wallet_capacity_pressure = trading_wallet;

    let burn_pressure = weighted_average(&[
        (weak_market_pressure, 0.25),
        (weak_liquidity_pressure, 0.15),
        (low_utility_pressure, 0.25),
        (holder_pressure, 0.25),
        (wallet_capacity_pressure, 0.10),
    ]);

    let burn_rate = MIN_BURN_RATE + (MAX_BURN_RATE - MIN_BURN_RATE) * burn_pressure;
    let burn_rate = burn_rate.clamp(MIN_BURN_RATE, MAX_BURN_RATE);

    let reason = if burn_rate <= 0.05 {
        "Healthy market and utility conditions: low conservation burn.".to_string()
    } else if burn_rate <= 0.15 {
        "Balanced conditions: medium burn to support utility and holder balance.".to_string()
    } else if burn_rate <= 0.24 {
        "High holder pressure or weak utility: stronger burn required.".to_string()
    } else {
        "Stress condition: maximum-range burn policy activated within allowed limits.".to_string()
    };

    DailyBurnDecision {
        burn_rate,
        burn_rate_percent: burn_rate * 100.0,
        market_health_score: market_health,
        liquidity_score: liquidity,
        utility_usage_score: utility_usage,
        holder_pressure_score: holder_pressure,
        trading_company_wallet_score: trading_wallet,
        reason,
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

        assert!(decision.burn_rate >= MIN_BURN_RATE);
        assert!(decision.burn_rate <= MAX_BURN_RATE);
    }

    #[test]
    fn low_utility_and_high_holder_pressure_increases_burn() {
        let healthy = calculate_daily_burn_decision(MarketPolicyInput {
            market_health_score: 0.9,
            liquidity_score: 0.9,
            utility_usage_score: 0.9,
            holder_pressure_score: 0.1,
            trading_company_wallet_score: 0.7,
        });

        let pressured = calculate_daily_burn_decision(MarketPolicyInput {
            market_health_score: 0.6,
            liquidity_score: 0.5,
            utility_usage_score: 0.1,
            holder_pressure_score: 0.95,
            trading_company_wallet_score: 0.7,
        });

        assert!(pressured.burn_rate > healthy.burn_rate);
    }
}

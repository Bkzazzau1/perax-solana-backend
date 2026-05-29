#[derive(Debug, Clone, Copy)]
pub struct PexTokenomics {
    pub token_name: &'static str,
    pub token_symbol: &'static str,
    pub total_supply: u64,
    pub decimals: u8,
    pub fixed_supply: bool,
    pub initial_price_usd: f64,
    pub initial_valuation_usd: f64,
    pub liquidity_venue: &'static str,
    pub liquidity_pair: &'static str,
    pub initial_liquidity_percentage: f64,
    pub initial_liquidity_pex_amount: u64,
    pub initial_liquidity_quote_usd: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct UnlockStage {
    pub stage: u8,
    pub base_price_usd: f64,
    pub trigger_price_usd: f64,
    pub calm_down_price_usd: f64,
    pub new_base_price_usd: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct UnlockPolicy {
    pub monitoring_interval_minutes: u64,
    pub twap_confirmation_minutes_min: u64,
    pub twap_confirmation_minutes_max: u64,
    pub cooldown_hours_min: u64,
    pub cooldown_hours_max: u64,
    pub max_daily_unlock_percentage_of_total_supply: f64,
    pub max_daily_unlock_amount: u64,
    pub max_monthly_unlock_percentage_of_total_supply: f64,
    pub max_monthly_unlock_amount: u64,
    pub trigger_multiplier: f64,
    pub trading_company_release_share_percentage: f64,
    pub other_release_wallets_share_percentage: f64,
    pub requires_manual_or_multisig_approval: bool,
    pub release_authority: &'static str,
    pub safety_authority: &'static str,
    pub emergency_pause_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ReleaseAllocationPreview {
    pub total_release_amount: u64,
    pub trading_company_amount: u64,
    pub other_release_wallets_amount: u64,
    pub trading_company_share_percentage: f64,
    pub other_release_wallets_share_percentage: f64,
}

#[derive(Debug, Clone)]
pub struct UnlockReview {
    pub should_release: bool,
    pub stage: UnlockStage,
    pub current_price_usd: f64,
    pub calm_down_price_usd: f64,
    pub next_base_price_usd: f64,
    pub next_trigger_price_usd: f64,
    pub allocation_preview: ReleaseAllocationPreview,
    pub message: String,
}

pub const PEX_TOKENOMICS: PexTokenomics = PexTokenomics {
    token_name: "Pera-X",
    token_symbol: "PEX",
    total_supply: 1_000_000_000,
    decimals: 6,
    fixed_supply: true,
    initial_price_usd: 0.000012,
    initial_valuation_usd: 12_000.0,
    liquidity_venue: "Meteora DLMM",
    liquidity_pair: "PEX/USDC",
    initial_liquidity_percentage: 38.0,
    initial_liquidity_pex_amount: 380_000_000,
    initial_liquidity_quote_usd: 4_560.0,
};

pub const UNLOCK_POLICY: UnlockPolicy = UnlockPolicy {
    monitoring_interval_minutes: 10,
    twap_confirmation_minutes_min: 30,
    twap_confirmation_minutes_max: 60,
    cooldown_hours_min: 2,
    cooldown_hours_max: 6,
    max_daily_unlock_percentage_of_total_supply: 1.0,
    max_daily_unlock_amount: 10_000_000,
    max_monthly_unlock_percentage_of_total_supply: 15.0,
    max_monthly_unlock_amount: 150_000_000,
    trigger_multiplier: 3.0,
    trading_company_release_share_percentage: 50.0,
    other_release_wallets_share_percentage: 50.0,
    requires_manual_or_multisig_approval: false,
    release_authority: "market_condition_oracle_only",
    safety_authority: "emergency_pause_and_system_maintenance_only",
    emergency_pause_enabled: true,
};

pub const UNLOCK_STAGES: [UnlockStage; 4] = [
    UnlockStage {
        stage: 1,
        base_price_usd: 0.000012,
        trigger_price_usd: 0.000036,
        calm_down_price_usd: 0.000020,
        new_base_price_usd: 0.000020,
    },
    UnlockStage {
        stage: 2,
        base_price_usd: 0.000020,
        trigger_price_usd: 0.000060,
        calm_down_price_usd: 0.000040,
        new_base_price_usd: 0.000040,
    },
    UnlockStage {
        stage: 3,
        base_price_usd: 0.000040,
        trigger_price_usd: 0.000120,
        calm_down_price_usd: 0.000080,
        new_base_price_usd: 0.000080,
    },
    UnlockStage {
        stage: 4,
        base_price_usd: 0.000080,
        trigger_price_usd: 0.000240,
        calm_down_price_usd: 0.000160,
        new_base_price_usd: 0.000160,
    },
];

pub fn evaluate_unlock_review(current_price_usd: f64) -> UnlockReview {
    let stage = current_stage_for_price(current_price_usd);
    let should_release = current_price_usd >= stage.trigger_price_usd;
    let allocation_preview = release_allocation_preview(UNLOCK_POLICY.max_daily_unlock_amount);
    let next_trigger_price_usd = next_trigger_price_for_stage(stage);

    let message = if should_release {
        format!(
            "PEX reached stage {} 3x trigger. Market-condition release can proceed only if TWAP, liquidity, buy-pressure, cooldown, daily cap, monthly cap, and emergency-pause gates pass. Calm-down/new base price becomes {:.8}. Next 3x trigger becomes {:.8}. Trading Company receives 50% of the approved release; all other eligible release wallets share the remaining 50%.",
            stage.stage, stage.new_base_price_usd, next_trigger_price_usd,
        )
    } else {
        format!(
            "PEX remains below stage {} 3x trigger. Current base is {:.8}; trigger is {:.8}; calm-down/new base after release is {:.8}. No release is required.",
            stage.stage, stage.base_price_usd, stage.trigger_price_usd, stage.new_base_price_usd,
        )
    };

    UnlockReview {
        should_release,
        stage,
        current_price_usd,
        calm_down_price_usd: stage.calm_down_price_usd,
        next_base_price_usd: stage.new_base_price_usd,
        next_trigger_price_usd,
        allocation_preview,
        message,
    }
}

pub fn release_allocation_preview(total_release_amount: u64) -> ReleaseAllocationPreview {
    let trading_company_amount = ((total_release_amount as f64)
        * (UNLOCK_POLICY.trading_company_release_share_percentage / 100.0))
        .round() as u64;

    let other_release_wallets_amount = total_release_amount.saturating_sub(trading_company_amount);

    ReleaseAllocationPreview {
        total_release_amount,
        trading_company_amount,
        other_release_wallets_amount,
        trading_company_share_percentage: UNLOCK_POLICY.trading_company_release_share_percentage,
        other_release_wallets_share_percentage: UNLOCK_POLICY
            .other_release_wallets_share_percentage,
    }
}

pub fn current_stage_for_price(current_price_usd: f64) -> UnlockStage {
    UNLOCK_STAGES
        .iter()
        .copied()
        .find(|stage| current_price_usd <= stage.trigger_price_usd)
        .unwrap_or(*UNLOCK_STAGES.last().expect("unlock stages are configured"))
}

fn next_trigger_price_for_stage(stage: UnlockStage) -> f64 {
    UNLOCK_STAGES
        .iter()
        .copied()
        .find(|candidate| candidate.stage == stage.stage.saturating_add(1))
        .map(|candidate| candidate.trigger_price_usd)
        .unwrap_or(stage.new_base_price_usd * UNLOCK_POLICY.trigger_multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_one_uses_exact_three_x_trigger_and_calm_down_price() {
        let review = evaluate_unlock_review(0.000036);
        assert!(review.should_release);
        assert_eq!(review.stage.stage, 1);
        assert_eq!(review.stage.trigger_price_usd, 0.000036);
        assert_eq!(review.calm_down_price_usd, 0.000020);
        assert_eq!(review.next_base_price_usd, 0.000020);
        assert_eq!(review.next_trigger_price_usd, 0.000060);
    }

    #[test]
    fn stage_two_new_base_becomes_next_trigger_base() {
        let review = evaluate_unlock_review(0.000060);
        assert!(review.should_release);
        assert_eq!(review.stage.stage, 2);
        assert_eq!(review.stage.base_price_usd, 0.000020);
        assert_eq!(review.calm_down_price_usd, 0.000040);
        assert_eq!(review.next_base_price_usd, 0.000040);
        assert_eq!(review.next_trigger_price_usd, 0.000120);
    }

    #[test]
    fn stage_three_trigger_is_three_x_of_new_base() {
        let review = evaluate_unlock_review(0.000120);
        assert!(review.should_release);
        assert_eq!(review.stage.stage, 3);
        assert_eq!(review.stage.base_price_usd, 0.000040);
        assert_eq!(review.calm_down_price_usd, 0.000080);
        assert_eq!(review.next_base_price_usd, 0.000080);
        assert_eq!(review.next_trigger_price_usd, 0.000240);
    }

    #[test]
    fn trading_company_always_gets_fifty_percent_of_release() {
        let allocation = release_allocation_preview(10_000_000);
        assert_eq!(allocation.trading_company_amount, 5_000_000);
        assert_eq!(allocation.other_release_wallets_amount, 5_000_000);
    }
}

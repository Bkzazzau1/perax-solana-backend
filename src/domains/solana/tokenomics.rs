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
    pub target_support_price_usd: f64,
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
    pub requires_manual_or_multisig_approval: bool,
    pub release_authority: &'static str,
    pub safety_authority: &'static str,
    pub emergency_pause_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct UnlockReview {
    pub should_review: bool,
    pub stage: UnlockStage,
    pub current_price_usd: f64,
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
    requires_manual_or_multisig_approval: false,
    release_authority: "market_condition_oracle_only",
    safety_authority: "emergency_pause_and_system_maintenance_only",
    emergency_pause_enabled: true,
};

pub const UNLOCK_STAGES: [UnlockStage; 3] = [
    UnlockStage { stage: 1, base_price_usd: 0.000012, trigger_price_usd: 0.00003, target_support_price_usd: 0.00002, new_base_price_usd: 0.00002 },
    UnlockStage { stage: 2, base_price_usd: 0.00002, trigger_price_usd: 0.00006, target_support_price_usd: 0.00004, new_base_price_usd: 0.00004 },
    UnlockStage { stage: 3, base_price_usd: 0.00004, trigger_price_usd: 0.00008, target_support_price_usd: 0.00006, new_base_price_usd: 0.00006 },
];

pub fn evaluate_unlock_review(current_price_usd: f64) -> UnlockReview {
    let stage = current_stage_for_price(current_price_usd);
    let should_review = current_price_usd >= stage.trigger_price_usd;
    let message = if should_review {
        format!("PEX price reached stage {} trigger on {}. Oracle-controlled release approval can be recorded only after TWAP, liquidity, volume, cooldown, daily cap, monthly cap, and business-purpose gates are satisfied.", stage.stage, PEX_TOKENOMICS.liquidity_venue)
    } else {
        format!("PEX price remains below stage {} trigger. No oracle release approval required.", stage.stage)
    };

    UnlockReview { should_review, stage, current_price_usd, message }
}

fn current_stage_for_price(current_price_usd: f64) -> UnlockStage {
    if current_price_usd >= UNLOCK_STAGES[2].base_price_usd {
        UNLOCK_STAGES[2]
    } else if current_price_usd >= UNLOCK_STAGES[1].base_price_usd {
        UNLOCK_STAGES[1]
    } else {
        UNLOCK_STAGES[0]
    }
}

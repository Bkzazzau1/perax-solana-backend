-- Align daily realized burn schedule with perax_core.execute_market_condition_burn.
-- Smart contract requires:
-- amount, eligible_revenue_amount, burn_rate_bps, market_health_score, observed_at, decision_id.

alter table pex_daily_realized_burns
    add column if not exists eligible_revenue_amount_pex numeric(30, 6) not null default 0 check (eligible_revenue_amount_pex >= 0),
    add column if not exists burn_rate_bps integer not null default 1000 check (burn_rate_bps >= 0 and burn_rate_bps <= 10000),
    add column if not exists market_health_score integer not null default 60 check (market_health_score >= 0 and market_health_score <= 100),
    add column if not exists decision_id_hex text,
    add column if not exists observed_at timestamptz not null default now(),
    add column if not exists onchain_burn_record text;

create unique index if not exists idx_pex_daily_realized_burns_decision_id_hex
    on pex_daily_realized_burns (decision_id_hex)
    where decision_id_hex is not null;

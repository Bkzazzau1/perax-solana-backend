create extension if not exists pgcrypto;

create table if not exists pex_revenue_events (
    id uuid primary key default gen_random_uuid(),
    reference_hex text not null unique,
    payer_wallet text,
    token_mint text,
    trading_company_settlement_account text,
    trading_company_second_wallet text not null,
    pex_received numeric(38, 6) not null default 0,
    credits_granted numeric(38, 6) not null default 0,
    immediate_burn_percentage numeric(10, 4) not null default 10,
    pex_burn_amount numeric(38, 6) not null default 0,
    pex_remaining_amount numeric(38, 6) not null default 0,
    burn_status text not null default 'declared',
    revenue_month date not null,
    revenue_day date not null,
    realized_after_credit boolean not null default false,
    service_code text,
    raw_event jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_pex_revenue_events_reference_hex
    on pex_revenue_events (reference_hex);

create index if not exists idx_pex_revenue_events_revenue_month
    on pex_revenue_events (revenue_month);

create index if not exists idx_pex_revenue_events_revenue_day
    on pex_revenue_events (revenue_day);

create index if not exists idx_pex_revenue_events_burn_status
    on pex_revenue_events (burn_status);

create table if not exists pex_monthly_sell_cap_ledger (
    revenue_month date primary key,
    trading_company_second_wallet text not null,
    monthly_revenue_pex numeric(38, 6) not null default 0,
    monthly_burned_pex numeric(38, 6) not null default 0,
    monthly_remaining_pex numeric(38, 6) not null default 0,
    sell_cap_percentage numeric(10, 4) not null default 50,
    monthly_sell_cap_pex numeric(38, 6) not null default 0,
    monthly_sold_pex numeric(38, 6) not null default 0,
    monthly_sell_allowance_remaining_pex numeric(38, 6) not null default 0,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_pex_monthly_sell_cap_wallet
    on pex_monthly_sell_cap_ledger (trading_company_second_wallet);

create table if not exists pex_daily_realized_burns (
    id uuid primary key default gen_random_uuid(),
    revenue_day date not null unique,
    trading_company_revenue_account text not null,
    realized_revenue_pex numeric(38, 6) not null default 0,
    eligible_revenue_amount_pex numeric(38, 6) not null default 0,
    burn_percentage numeric(10, 4) not null default 10,
    burn_rate_bps integer not null default 1000,
    market_health_score integer not null default 60,
    burn_amount_pex numeric(38, 6) not null default 0,
    remaining_revenue_pex numeric(38, 6) not null default 0,
    decision_id_hex text not null,
    observed_at timestamptz not null default now(),
    burn_status text not null default 'scheduled',
    last_revenue_event_id uuid,
    onchain_tx_signature text,
    executed_at timestamptz,
    execution_error text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_pex_daily_realized_burns_status
    on pex_daily_realized_burns (burn_status);

create index if not exists idx_pex_daily_realized_burns_day
    on pex_daily_realized_burns (revenue_day);

create index if not exists idx_pex_daily_realized_burns_decision_id
    on pex_daily_realized_burns (decision_id_hex);

create index if not exists idx_pex_daily_realized_burns_onchain_tx_signature
    on pex_daily_realized_burns (onchain_tx_signature);
create table if not exists daily_burn_decisions (
    id uuid primary key default gen_random_uuid(),
    declared_at timestamptz not null default now(),
    burn_rate numeric(10, 8) not null,
    burn_rate_percent numeric(8, 4) not null,
    market_health_score numeric(8, 4) not null,
    liquidity_score numeric(8, 4) not null,
    utility_usage_score numeric(8, 4) not null,
    holder_pressure_score numeric(8, 4) not null,
    trading_company_wallet_score numeric(8, 4) not null,
    trading_company_balance numeric(38, 9) not null default 0,
    tokens_to_burn numeric(38, 9) not null default 0,
    reason text not null,
    tx_signature text,
    status text not null default 'declared',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_daily_burn_decisions_declared_at
    on daily_burn_decisions (declared_at desc);

create index if not exists idx_daily_burn_decisions_status
    on daily_burn_decisions (status);

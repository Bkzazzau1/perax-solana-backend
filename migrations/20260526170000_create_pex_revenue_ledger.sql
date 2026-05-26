-- PEX revenue ledger and monthly sell-cap tracking
-- This migration supports the approved Trading Company second-wallet model:
-- 1. User pays PEX for Credits.
-- 2. Backend credits user Credits.
-- 3. Immediate burn portion is recorded/triggered.
-- 4. Remaining PEX revenue is assigned to Trading Company second wallet.
-- 5. Monthly PEX sales from the second wallet are capped at 50%.

create table if not exists pex_revenue_events (
    id uuid primary key default gen_random_uuid(),
    reference_hex text not null unique,
    payer_wallet text,
    token_mint text,
    trading_company_settlement_account text not null,
    trading_company_second_wallet text not null,
    pex_received numeric(30, 6) not null check (pex_received >= 0),
    credits_granted numeric(30, 6) not null check (credits_granted >= 0),
    immediate_burn_percentage numeric(8, 4) not null check (immediate_burn_percentage >= 0 and immediate_burn_percentage <= 100),
    pex_burn_amount numeric(30, 6) not null check (pex_burn_amount >= 0),
    pex_remaining_amount numeric(30, 6) not null check (pex_remaining_amount >= 0),
    burn_decision_id uuid,
    burn_status text not null default 'pending' check (burn_status in ('pending', 'declared', 'approved', 'executed', 'failed', 'cancelled')),
    burn_tx_signature text,
    revenue_month date not null,
    source text not null default 'pex_credit_purchase',
    service_code text,
    raw_event jsonb,
    credited_at timestamptz not null default now(),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint pex_revenue_amount_check check (pex_received = pex_burn_amount + pex_remaining_amount)
);

create index if not exists idx_pex_revenue_events_month
    on pex_revenue_events (revenue_month);

create index if not exists idx_pex_revenue_events_burn_status
    on pex_revenue_events (burn_status);

create index if not exists idx_pex_revenue_events_second_wallet
    on pex_revenue_events (trading_company_second_wallet);

create table if not exists pex_monthly_sell_cap_ledger (
    id uuid primary key default gen_random_uuid(),
    revenue_month date not null unique,
    trading_company_second_wallet text not null,
    monthly_revenue_pex numeric(30, 6) not null default 0 check (monthly_revenue_pex >= 0),
    monthly_burned_pex numeric(30, 6) not null default 0 check (monthly_burned_pex >= 0),
    monthly_remaining_pex numeric(30, 6) not null default 0 check (monthly_remaining_pex >= 0),
    sell_cap_percentage numeric(8, 4) not null default 50 check (sell_cap_percentage = 50),
    monthly_sell_cap_pex numeric(30, 6) not null default 0 check (monthly_sell_cap_pex >= 0),
    monthly_sold_pex numeric(30, 6) not null default 0 check (monthly_sold_pex >= 0),
    monthly_sell_allowance_remaining_pex numeric(30, 6) not null default 0 check (monthly_sell_allowance_remaining_pex >= 0),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint pex_monthly_sell_cap_not_exceeded check (monthly_sold_pex <= monthly_sell_cap_pex),
    constraint pex_monthly_sell_allowance_math check (monthly_sell_allowance_remaining_pex = monthly_sell_cap_pex - monthly_sold_pex)
);

create index if not exists idx_pex_monthly_sell_cap_wallet
    on pex_monthly_sell_cap_ledger (trading_company_second_wallet);

create table if not exists pex_second_wallet_sell_events (
    id uuid primary key default gen_random_uuid(),
    revenue_month date not null,
    trading_company_second_wallet text not null,
    pex_sell_amount numeric(30, 6) not null check (pex_sell_amount > 0),
    sell_reason text,
    approval_status text not null default 'declared' check (approval_status in ('declared', 'approved', 'executed', 'failed', 'cancelled')),
    tx_signature text,
    raw_event jsonb,
    declared_at timestamptz not null default now(),
    approved_at timestamptz,
    executed_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_pex_second_wallet_sell_events_month
    on pex_second_wallet_sell_events (revenue_month);

create index if not exists idx_pex_second_wallet_sell_events_status
    on pex_second_wallet_sell_events (approval_status);

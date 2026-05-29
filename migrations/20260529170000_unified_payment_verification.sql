create table if not exists payment_intents (
    id uuid primary key default gen_random_uuid(),
    intent_reference text not null unique,
    quote_reference text not null references credit_purchase_quotes (quote_reference),
    funding_method text not null check (funding_method in ('pex', 'card', 'stablecoin', 'virtual_account')),
    user_id text,
    payer_wallet text,
    provider text,
    provider_reference text,
    expected_asset_code text not null,
    expected_amount numeric(38, 9) not null check (expected_amount >= 0),
    expected_usd_value numeric(38, 9) not null check (expected_usd_value >= 0),
    expected_credits numeric(38, 9) not null check (expected_credits > 0),
    pex_price_usd numeric(30, 12),
    burn_percentage numeric(8, 4) not null default 0 check (burn_percentage >= 0 and burn_percentage <= 100),
    burn_usd_value numeric(38, 9) not null default 0 check (burn_usd_value >= 0),
    reference_hex text,
    status text not null default 'pending_verification'
        check (status in ('pending_verification', 'verified', 'credited', 'failed', 'cancelled')),
    idempotency_key text,
    created_at timestamptz not null default now(),
    verified_at timestamptz,
    credited_at timestamptz,
    updated_at timestamptz not null default now()
);

create unique index if not exists idx_payment_intents_idempotency
    on payment_intents (idempotency_key)
    where idempotency_key is not null;

create unique index if not exists idx_payment_intents_reference_hex
    on payment_intents (reference_hex)
    where reference_hex is not null;

create unique index if not exists idx_payment_intents_provider_reference
    on payment_intents (provider, provider_reference)
    where provider is not null and provider_reference is not null;

create index if not exists idx_payment_intents_status
    on payment_intents (status, created_at desc);

create table if not exists payment_confirmations (
    id uuid primary key default gen_random_uuid(),
    payment_intent_id uuid not null references payment_intents (id),
    method text not null check (method in ('pex', 'card', 'stablecoin', 'virtual_account')),
    verification_source text not null,
    status text not null check (status in ('verified', 'failed')),
    provider text,
    provider_reference text,
    reference_hex text,
    tx_signature text,
    payer_wallet text,
    token_mint text,
    trading_company_token_account text,
    trading_company_revenue_token_account text,
    amount_paid numeric(38, 9) not null default 0,
    currency text not null,
    raw_confirmation jsonb,
    failure_reason text,
    verified_at timestamptz not null default now(),
    created_at timestamptz not null default now()
);

create unique index if not exists idx_payment_confirmations_pex_reference
    on payment_confirmations (reference_hex)
    where reference_hex is not null and status = 'verified';

create unique index if not exists idx_payment_confirmations_provider_reference
    on payment_confirmations (provider, provider_reference)
    where provider is not null and provider_reference is not null and status = 'verified';

create table if not exists credit_ledger (
    id uuid primary key default gen_random_uuid(),
    payment_intent_id uuid not null unique references payment_intents (id),
    quote_reference text not null,
    user_id text,
    credits_granted numeric(38, 9) not null check (credits_granted > 0),
    ledger_status text not null default 'posted' check (ledger_status in ('posted', 'reversed')),
    created_at timestamptz not null default now()
);

create table if not exists revenue_ledger (
    id uuid primary key default gen_random_uuid(),
    payment_intent_id uuid not null unique references payment_intents (id),
    quote_reference text not null,
    funding_method text not null,
    asset_code text not null,
    asset_amount numeric(38, 9) not null check (asset_amount >= 0),
    usd_value numeric(38, 9) not null check (usd_value >= 0),
    pex_price_usd numeric(30, 12),
    revenue_status text not null default 'realized' check (revenue_status in ('realized', 'pending_settlement', 'reversed')),
    created_at timestamptz not null default now()
);

create table if not exists burn_liabilities (
    id uuid primary key default gen_random_uuid(),
    payment_intent_id uuid not null unique references payment_intents (id),
    quote_reference text not null,
    funding_method text not null,
    fiat_revenue_usd numeric(38, 9) not null default 0 check (fiat_revenue_usd >= 0),
    burn_percentage numeric(8, 4) not null check (burn_percentage >= 0 and burn_percentage <= 100),
    burn_usd_value numeric(38, 9) not null default 0 check (burn_usd_value >= 0),
    pex_price_usd numeric(30, 12) not null check (pex_price_usd > 0),
    pex_burn_required numeric(38, 9) not null default 0 check (pex_burn_required >= 0),
    status text not null check (status in ('not_required', 'pending_pex_funding', 'funded', 'scheduled', 'executed', 'cancelled')),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

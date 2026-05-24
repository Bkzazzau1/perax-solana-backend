create table if not exists utility_payments (
    id uuid primary key default gen_random_uuid(),
    reference bytea not null unique,
    reference_hex text not null unique,
    payer_wallet text,
    token_mint text,
    trading_company_token_account text not null,
    amount numeric(38, 9) not null,
    source text not null default 'solana_contract',
    service_code text,
    status text not null default 'detected',
    tx_signature text unique,
    raw_event jsonb,
    detected_at timestamptz not null default now(),
    confirmed_at timestamptz,
    granted_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    check (status in ('detected', 'confirmed', 'granted', 'failed', 'cancelled'))
);

create index if not exists idx_utility_payments_status_detected_at
    on utility_payments (status, detected_at desc);

create index if not exists idx_utility_payments_payer_wallet
    on utility_payments (payer_wallet);

create index if not exists idx_utility_payments_service_code
    on utility_payments (service_code);

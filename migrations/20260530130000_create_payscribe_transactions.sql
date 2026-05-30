create table if not exists payscribe_transactions (
    id uuid primary key default gen_random_uuid(),
    account_id uuid,
    service_type text not null,
    provider_reference text not null unique,
    network text,
    plan_code text,
    recipient jsonb,
    charge_credits numeric(18, 6),
    provider_status text not null default 'created',
    provider_trans_id text,
    provider_payload jsonb not null default '{}'::jsonb,
    requery_payload jsonb,
    last_requery_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_payscribe_transactions_account_id
    on payscribe_transactions (account_id);

create index if not exists idx_payscribe_transactions_provider_trans_id
    on payscribe_transactions (provider_trans_id);

create index if not exists idx_payscribe_transactions_status
    on payscribe_transactions (provider_status);

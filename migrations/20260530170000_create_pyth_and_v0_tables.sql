create table if not exists pyth_price_snapshots (
    id uuid primary key default gen_random_uuid(),
    feed_id text not null,
    symbol text,
    price numeric(38, 18),
    confidence numeric(38, 18),
    exponent integer,
    publish_time bigint,
    provider_payload jsonb not null default '{}'::jsonb,
    created_at timestamptz not null default now()
);

create index if not exists idx_pyth_price_snapshots_feed_id_created_at
    on pyth_price_snapshots (feed_id, created_at desc);

create table if not exists v0_generation_requests (
    id uuid primary key default gen_random_uuid(),
    account_id uuid not null,
    request_reference text not null unique,
    v0_chat_id text,
    status text not null default 'created',
    prompt text not null,
    mode text not null default 'create',
    credit_cost numeric(18, 6) not null default 0,
    request_payload jsonb not null default '{}'::jsonb,
    provider_response jsonb not null default '{}'::jsonb,
    error_message text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_v0_generation_requests_account_id
    on v0_generation_requests (account_id);

create index if not exists idx_v0_generation_requests_v0_chat_id
    on v0_generation_requests (v0_chat_id);

insert into utility_pricing_settings (
    service_code,
    service_name,
    category,
    credit_cost,
    billing_unit,
    is_active
) values (
    'v0_code_generation',
    'Vercel v0 Code Generation',
    'ai',
    100,
    'per_generation',
    true
)
on conflict (service_code) do nothing;

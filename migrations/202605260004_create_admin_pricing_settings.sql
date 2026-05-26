create table if not exists utility_pricing_settings (
    id uuid primary key default gen_random_uuid(),
    service_code text not null unique,
    service_name text not null,
    category text not null,
    credit_cost numeric(18, 6) not null default 0,
    billing_unit text not null default 'per_action',
    is_active boolean not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_utility_pricing_settings_category
    on utility_pricing_settings (category);

insert into utility_pricing_settings (service_code, service_name, category, credit_cost, billing_unit, is_active)
values
    ('ai_detector', 'AI Detector', 'ai', 6, 'per_request', true),
    ('plagiarism_checker', 'Plagiarism Checker', 'ai', 8, 'per_request', true),
    ('humanizer', 'Humanizer AI', 'ai', 10, 'per_request', true),
    ('local_call', 'Local Call', 'call', 1, 'per_minute', true),
    ('global_call', 'Global Call', 'call', 3, 'per_minute', true),
    ('sms_outbound', 'Outbound SMS', 'sms', 0.02, 'per_segment', true),
    ('bill_payment', 'Bill Payment', 'bill', 1, 'per_transaction', true)
on conflict (service_code) do nothing;

create table if not exists credit_exchange_rates (
    id uuid primary key default gen_random_uuid(),
    asset_code text not null unique,
    asset_name text not null,
    credits_per_unit numeric(18, 6) not null default 0,
    unit_label text not null default '1',
    is_active boolean not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

insert into credit_exchange_rates (asset_code, asset_name, credits_per_unit, unit_label, is_active)
values
    ('PEX', 'Pera-X Token', 100, '1 PEX', true),
    ('USDT', 'Tether USD', 100, '1 USDT', true),
    ('USDC', 'USD Coin', 100, '1 USDC', true),
    ('FIAT_USD', 'US Dollar', 100, '1 USD', true)
on conflict (asset_code) do nothing;

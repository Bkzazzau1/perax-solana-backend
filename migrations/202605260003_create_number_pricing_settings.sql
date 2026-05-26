create table if not exists number_pricing_settings (
    id uuid primary key default gen_random_uuid(),
    country text not null,
    number_type text not null default 'local',
    setup_fee_credits numeric(18, 6) not null default 0,
    monthly_fee_credits numeric(18, 6) not null default 0,
    annual_fee_credits numeric(18, 6) not null default 0,
    currency text not null default 'CREDITS',
    is_active boolean not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    unique (country, number_type)
);

create index if not exists idx_number_pricing_settings_active
    on number_pricing_settings (is_active);

insert into number_pricing_settings (
    country,
    number_type,
    setup_fee_credits,
    monthly_fee_credits,
    annual_fee_credits,
    currency,
    is_active
)
values
    ('United States', 'local', 10, 30, 300, 'CREDITS', true),
    ('United Kingdom', 'local', 10, 35, 350, 'CREDITS', true),
    ('Canada', 'local', 10, 30, 300, 'CREDITS', true)
on conflict (country, number_type) do nothing;

alter table provisioned_numbers
    add column if not exists setup_fee_credits numeric(18, 6),
    add column if not exists monthly_fee_credits numeric(18, 6),
    add column if not exists next_renewal_at timestamptz,
    add column if not exists billing_status text not null default 'active';

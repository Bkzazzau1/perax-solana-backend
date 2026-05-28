-- Credit pricing, promo, and fiat/stablecoin burn policy
-- Credits are stable internal service units. Default policy: 1,000 Credits = $1.
-- All funding methods must pass through this pricing layer before Credits are granted.

create table if not exists credit_pricing_policy (
    id uuid primary key default gen_random_uuid(),
    policy_key text not null unique,
    credits_per_usd numeric(30, 6) not null check (credits_per_usd > 0),
    default_discount_percentage numeric(8, 4) not null default 0 check (default_discount_percentage >= 0 and default_discount_percentage <= 100),
    pex_discount_percentage numeric(8, 4) not null default 0 check (pex_discount_percentage >= 0 and pex_discount_percentage <= 100),
    fiat_discount_percentage numeric(8, 4) not null default 0 check (fiat_discount_percentage >= 0 and fiat_discount_percentage <= 100),
    stablecoin_discount_percentage numeric(8, 4) not null default 0 check (stablecoin_discount_percentage >= 0 and stablecoin_discount_percentage <= 100),
    virtual_account_discount_percentage numeric(8, 4) not null default 0 check (virtual_account_discount_percentage >= 0 and virtual_account_discount_percentage <= 100),
    pex_price_usd numeric(30, 12) not null default 0.000012 check (pex_price_usd > 0),
    pex_price_source text not null default 'admin_override',
    fiat_revenue_burn_percentage numeric(8, 4) not null default 10 check (fiat_revenue_burn_percentage >= 0 and fiat_revenue_burn_percentage <= 100),
    stablecoin_revenue_burn_percentage numeric(8, 4) not null default 10 check (stablecoin_revenue_burn_percentage >= 0 and stablecoin_revenue_burn_percentage <= 100),
    pex_immediate_burn_percentage numeric(8, 4) not null default 10 check (pex_immediate_burn_percentage >= 0 and pex_immediate_burn_percentage <= 100),
    is_active bool not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

insert into credit_pricing_policy (
    policy_key,
    credits_per_usd,
    default_discount_percentage,
    pex_discount_percentage,
    fiat_discount_percentage,
    stablecoin_discount_percentage,
    virtual_account_discount_percentage,
    pex_price_usd,
    pex_price_source,
    fiat_revenue_burn_percentage,
    stablecoin_revenue_burn_percentage,
    pex_immediate_burn_percentage,
    is_active
) values (
    'default',
    1000,
    0,
    0,
    0,
    0,
    0,
    0.000012,
    'admin_override',
    10,
    10,
    10,
    true
)
on conflict (policy_key) do nothing;

create table if not exists promo_codes (
    id uuid primary key default gen_random_uuid(),
    code text not null unique,
    description text,
    discount_percentage numeric(8, 4) not null check (discount_percentage >= 0 and discount_percentage <= 100),
    max_uses integer check (max_uses is null or max_uses > 0),
    used_count integer not null default 0 check (used_count >= 0),
    min_credit_amount numeric(30, 6) not null default 0 check (min_credit_amount >= 0),
    starts_at timestamptz,
    expires_at timestamptz,
    is_active bool not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint promo_code_usage_cap check (max_uses is null or used_count <= max_uses)
);

create table if not exists credit_purchase_quotes (
    id uuid primary key default gen_random_uuid(),
    quote_reference text not null unique,
    funding_method text not null check (funding_method in ('pex', 'card', 'stablecoin', 'virtual_account')),
    requested_credits numeric(30, 6) not null check (requested_credits > 0),
    discount_percentage numeric(8, 4) not null default 0 check (discount_percentage >= 0 and discount_percentage <= 100),
    promo_code text,
    final_credits numeric(30, 6) not null check (final_credits > 0),
    usd_value numeric(30, 6) not null check (usd_value >= 0),
    pex_price_usd numeric(30, 12),
    pex_required numeric(30, 6) not null default 0 check (pex_required >= 0),
    fiat_required numeric(30, 6) not null default 0 check (fiat_required >= 0),
    burn_percentage numeric(8, 4) not null default 0 check (burn_percentage >= 0 and burn_percentage <= 100),
    burn_usd_value numeric(30, 6) not null default 0 check (burn_usd_value >= 0),
    status text not null default 'quoted' check (status in ('quoted', 'accepted', 'credited', 'cancelled', 'expired')),
    idempotency_key text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index if not exists idx_credit_purchase_quotes_idempotency
    on credit_purchase_quotes (idempotency_key)
    where idempotency_key is not null;

create index if not exists idx_credit_purchase_quotes_method_status
    on credit_purchase_quotes (funding_method, status);

alter table pex_revenue_events
    add column if not exists funding_method text not null default 'pex',
    add column if not exists usd_value numeric(30, 6) not null default 0,
    add column if not exists pex_price_usd numeric(30, 12),
    add column if not exists fiat_revenue_burn_usd numeric(30, 6) not null default 0,
    add column if not exists promo_code text,
    add column if not exists idempotency_key text;

create unique index if not exists idx_pex_revenue_events_idempotency
    on pex_revenue_events (idempotency_key)
    where idempotency_key is not null;

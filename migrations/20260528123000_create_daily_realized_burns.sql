-- Daily realized-revenue burn schedule
-- Burn is calculated only after payment is realized and Credits are credited.
-- It is based on the Trading Company revenue account and the configured burn proportion for that day.

create table if not exists pex_daily_realized_burns (
    id uuid primary key default gen_random_uuid(),
    revenue_day date not null unique,
    trading_company_revenue_account text not null,
    realized_revenue_pex numeric(30, 6) not null default 0 check (realized_revenue_pex >= 0),
    burn_percentage numeric(8, 4) not null check (burn_percentage >= 0 and burn_percentage <= 100),
    burn_amount_pex numeric(30, 6) not null default 0 check (burn_amount_pex >= 0),
    remaining_revenue_pex numeric(30, 6) not null default 0 check (remaining_revenue_pex >= 0),
    burn_status text not null default 'scheduled' check (burn_status in ('scheduled', 'approved', 'executed', 'failed', 'cancelled')),
    burn_tx_signature text,
    last_revenue_event_id uuid,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    constraint pex_daily_realized_burns_math check (realized_revenue_pex = burn_amount_pex + remaining_revenue_pex)
);

create index if not exists idx_pex_daily_realized_burns_status
    on pex_daily_realized_burns (burn_status);

create index if not exists idx_pex_daily_realized_burns_account
    on pex_daily_realized_burns (trading_company_revenue_account);

alter table pex_revenue_events
    add column if not exists revenue_day date not null default current_date,
    add column if not exists realized_after_credit bool not null default true;

create index if not exists idx_pex_revenue_events_revenue_day
    on pex_revenue_events (revenue_day);

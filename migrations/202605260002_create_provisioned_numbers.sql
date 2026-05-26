create table if not exists provisioned_numbers (
    id uuid primary key default gen_random_uuid(),
    account_id uuid,
    phone_number text not null unique,
    telnyx_order_id text,
    country text,
    plan text,
    status text not null default 'reserved',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_provisioned_numbers_account_id
    on provisioned_numbers (account_id);

create index if not exists idx_provisioned_numbers_status
    on provisioned_numbers (status);

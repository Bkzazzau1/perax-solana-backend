create table if not exists paystack_virtual_accounts (
    id uuid primary key default gen_random_uuid(),
    account_id uuid not null unique,
    email text not null,
    customer_code text,
    account_name text,
    account_number text,
    bank_name text,
    bank_slug text,
    currency text not null default 'NGN',
    provider_status text not null default 'pending',
    provider_payload jsonb not null default '{}'::jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index if not exists idx_paystack_virtual_accounts_account_number
    on paystack_virtual_accounts (account_number)
    where account_number is not null;

create index if not exists idx_paystack_virtual_accounts_customer_code
    on paystack_virtual_accounts (customer_code);

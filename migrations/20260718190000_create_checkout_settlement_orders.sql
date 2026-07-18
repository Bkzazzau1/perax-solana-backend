create table if not exists checkout_settlement_orders (
    id uuid primary key default gen_random_uuid(),
    order_reference text not null unique,
    idempotency_key text,
    account_id uuid not null references accounts(id),
    service_code text not null,
    service_name text not null,
    service_category text not null,
    quantity bigint not null check (quantity > 0),
    unit_credit_cost numeric(30, 6) not null check (unit_credit_cost > 0),
    total_credit_cost numeric(30, 6) not null check (total_credit_cost > 0),
    credits_per_usd numeric(30, 6) not null check (credits_per_usd > 0),
    quote_value_usd numeric(30, 6) not null check (quote_value_usd > 0),
    credit_ledger_id uuid,
    settlement_id_hex char(64) not null unique,
    settlement_product_id_hex char(64) not null,
    beneficiary_wallet text,
    order_status text not null default 'created'
        check (order_status in ('created', 'credits_reserved', 'failed', 'cancelled')),
    settlement_status text not null default 'pending'
        check (settlement_status in ('pending', 'planned', 'funding', 'ready', 'finalized', 'failed')),
    settlement_record_address text,
    settlement_transaction_signature text,
    settlement_error text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    settled_at timestamptz
);

create unique index if not exists idx_checkout_settlement_orders_account_idempotency
    on checkout_settlement_orders (account_id, idempotency_key)
    where idempotency_key is not null;

create index if not exists idx_checkout_settlement_orders_pending
    on checkout_settlement_orders (settlement_status, created_at)
    where settlement_status in ('pending', 'planned', 'funding', 'ready');

create index if not exists idx_checkout_settlement_orders_account_created
    on checkout_settlement_orders (account_id, created_at desc);

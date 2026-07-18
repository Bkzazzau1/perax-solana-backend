alter table checkout_settlement_orders
    add column if not exists settlement_funding_method text not null default 'stablecoin',
    add column if not exists settlement_claimed_at timestamptz,
    add column if not exists settlement_attempt_count integer not null default 0,
    add column if not exists settlement_last_attempt_at timestamptz,
    add column if not exists refund_ledger_id uuid,
    add column if not exists refunded_at timestamptz;

create index if not exists idx_checkout_settlement_orders_worker_claim
    on checkout_settlement_orders (settlement_status, settlement_claimed_at, created_at)
    where order_status = 'credits_reserved'
      and settlement_status in ('pending', 'planned', 'funding', 'ready');

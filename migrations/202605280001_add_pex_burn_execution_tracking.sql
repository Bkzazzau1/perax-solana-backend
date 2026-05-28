alter table if exists pex_daily_realized_burns
    add column if not exists onchain_tx_signature text,
    add column if not exists executed_at timestamptz,
    add column if not exists execution_error text;

create index if not exists idx_pex_daily_realized_burns_status
    on pex_daily_realized_burns (burn_status);

create index if not exists idx_pex_daily_realized_burns_onchain_tx_signature
    on pex_daily_realized_burns (onchain_tx_signature);

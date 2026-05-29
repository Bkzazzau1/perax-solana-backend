alter table if exists pex_daily_realized_burns
    add column if not exists onchain_burn_record text;

create index if not exists idx_pex_daily_realized_burns_onchain_burn_record
    on pex_daily_realized_burns (onchain_burn_record);

-- Fix daily burn execution schema to match system/oracle-controlled burn lifecycle.
-- Admin remains view-only. Burn execution is performed by backend/oracle executor.

alter table pex_daily_realized_burns
    add column if not exists onchain_tx_signature text,
    add column if not exists executed_at timestamptz,
    add column if not exists execution_error text;

-- Keep legacy burn_tx_signature populated for existing admin views while using
-- onchain_tx_signature as the clearer execution field going forward.
update pex_daily_realized_burns
set onchain_tx_signature = coalesce(onchain_tx_signature, burn_tx_signature)
where burn_tx_signature is not null;

-- Remove old manual/admin lifecycle values from existing data before tightening check.
update pex_daily_realized_burns
set burn_status = case
    when burn_status = 'approved' then 'scheduled'
    when burn_status = 'cancelled' then 'failed'
    else burn_status
end
where burn_status in ('approved', 'cancelled');

-- Replace the old check constraint that allowed approved/cancelled.
do $$
declare
    constraint_name text;
begin
    select conname into constraint_name
    from pg_constraint
    where conrelid = 'pex_daily_realized_burns'::regclass
      and contype = 'c'
      and pg_get_constraintdef(oid) like '%burn_status%';

    if constraint_name is not null then
        execute format('alter table pex_daily_realized_burns drop constraint %I', constraint_name);
    end if;
end $$;

alter table pex_daily_realized_burns
    add constraint pex_daily_realized_burns_burn_status_check
    check (burn_status in ('scheduled', 'submitted', 'executed', 'failed'));

create index if not exists idx_pex_daily_realized_burns_execution_status
    on pex_daily_realized_burns (burn_status, revenue_day);

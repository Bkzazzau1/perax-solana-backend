do $$
begin
    if not exists (
        select 1
        from pg_constraint
        where conname = 'daily_burn_decisions_status_check'
    ) then
        alter table daily_burn_decisions
            add constraint daily_burn_decisions_status_check
            check (status in ('declared', 'approved', 'executed', 'failed', 'cancelled'));
    end if;
end $$;

create index if not exists idx_daily_burn_decisions_status_declared_at
    on daily_burn_decisions (status, declared_at desc);

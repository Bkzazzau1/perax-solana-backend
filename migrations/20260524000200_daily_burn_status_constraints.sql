alter table daily_burn_decisions
    add constraint if not exists daily_burn_decisions_status_check
    check (status in ('declared', 'approved', 'executed', 'failed', 'cancelled'));

create index if not exists idx_daily_burn_decisions_status_declared_at
    on daily_burn_decisions (status, declared_at desc);

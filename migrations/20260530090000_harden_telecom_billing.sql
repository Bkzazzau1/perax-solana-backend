do $$
declare
    constraint_name text;
begin
    select con.conname into constraint_name
    from pg_constraint con
    join pg_class rel on rel.oid = con.conrelid
    join pg_attribute att on att.attrelid = rel.oid and att.attnum = any(con.conkey)
    where rel.relname = 'credit_ledger'
      and con.contype = 'u'
      and att.attname = 'payment_intent_id'
    limit 1;

    if constraint_name is not null then
        execute format('alter table credit_ledger drop constraint %I', constraint_name);
    end if;

    select con.conname into constraint_name
    from pg_constraint con
    join pg_class rel on rel.oid = con.conrelid
    where rel.relname = 'credit_ledger'
      and con.contype = 'c'
      and pg_get_constraintdef(con.oid) ilike '%credits_granted%'
    limit 1;

    if constraint_name is not null then
        execute format('alter table credit_ledger drop constraint %I', constraint_name);
    end if;
end $$;

alter table credit_ledger
    alter column payment_intent_id drop not null,
    alter column credits_granted drop not null,
    add column if not exists account_id uuid references accounts(id) on delete set null,
    add column if not exists ledger_direction text,
    add column if not exists credit_delta numeric(38, 9),
    add column if not exists balance_after numeric(38, 9),
    add column if not exists source text,
    add column if not exists source_reference text,
    add column if not exists description text,
    add column if not exists metadata jsonb not null default '{}'::jsonb;

update credit_ledger cl
set account_id = case
        when pi.user_id ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        then pi.user_id::uuid
        else cl.account_id
    end,
    ledger_direction = coalesce(cl.ledger_direction, 'credit'),
    credit_delta = coalesce(cl.credit_delta, cl.credits_granted, 0),
    source = coalesce(cl.source, 'payment'),
    source_reference = coalesce(cl.source_reference, cl.quote_reference)
from payment_intents pi
where cl.payment_intent_id = pi.id;

update credit_ledger
set ledger_direction = coalesce(ledger_direction, case when coalesce(credit_delta, credits_granted, 0) >= 0 then 'credit' else 'debit' end),
    credit_delta = coalesce(credit_delta, credits_granted, 0),
    source = coalesce(source, 'legacy'),
    source_reference = coalesce(source_reference, quote_reference);

alter table credit_ledger
    alter column ledger_direction set not null,
    alter column credit_delta set not null,
    add constraint credit_ledger_direction_check check (ledger_direction in ('credit', 'debit')),
    add constraint credit_ledger_delta_check check (
        (ledger_direction = 'credit' and credit_delta > 0)
        or (ledger_direction = 'debit' and credit_delta < 0)
    );

create unique index if not exists idx_credit_ledger_source_reference
    on credit_ledger (source, source_reference)
    where source is not null and source_reference is not null;

create index if not exists idx_credit_ledger_account_created_at
    on credit_ledger (account_id, created_at desc)
    where account_id is not null;

alter table telnyx_voice_calls
    add column if not exists service_code text,
    add column if not exists rate_per_minute numeric(38, 9),
    add column if not exists billed_seconds integer,
    add column if not exists billed_minutes numeric(38, 9),
    add column if not exists credits_charged numeric(38, 9),
    add column if not exists billing_status text not null default 'not_billed'
        check (billing_status in ('not_billed', 'pending', 'posted', 'failed', 'waived')),
    add column if not exists billing_ledger_id uuid references credit_ledger(id) on delete set null,
    add column if not exists billing_error text;

create table if not exists provider_transactions (
    id uuid primary key default gen_random_uuid(),
    provider text not null,
    provider_action text not null,
    account_id uuid references accounts(id) on delete set null,
    source text,
    source_reference text,
    request_payload jsonb,
    response_payload jsonb,
    http_status integer,
    success boolean not null default false,
    error_message text,
    created_at timestamptz not null default now()
);

create index if not exists idx_provider_transactions_provider_created_at
    on provider_transactions (provider, created_at desc);

create index if not exists idx_provider_transactions_account_created_at
    on provider_transactions (account_id, created_at desc)
    where account_id is not null;

alter table provisioned_numbers
    add column if not exists regulatory_status text not null default 'not_required'
        check (regulatory_status in ('not_required', 'required', 'pending_review', 'approved', 'rejected')),
    add column if not exists regulatory_requirements jsonb,
    add column if not exists regulatory_documents jsonb;

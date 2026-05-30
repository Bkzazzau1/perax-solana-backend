alter table provider_transactions
    add column if not exists provider_carrier_fee_usd numeric(38, 9),
    add column if not exists provider_carrier_fee_currency text,
    add column if not exists provider_rate_usd numeric(38, 9),
    add column if not exists provider_rate_currency text;

alter table telnyx_voice_calls
    add column if not exists cdr_report_payload jsonb,
    add column if not exists cdr_synced_at timestamptz;

create table if not exists telnyx_usage_report_syncs (
    id uuid primary key default gen_random_uuid(),
    report_type text not null check (report_type in ('cdr', 'mdr')),
    start_time timestamptz not null,
    end_time timestamptz not null,
    records_processed integer not null default 0,
    records_matched integer not null default 0,
    provider_response jsonb,
    status text not null default 'completed',
    error_message text,
    created_at timestamptz not null default now()
);

create index if not exists idx_telnyx_usage_report_syncs_type_created_at
    on telnyx_usage_report_syncs (report_type, created_at desc);

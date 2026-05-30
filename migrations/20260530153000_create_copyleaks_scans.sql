create table if not exists copyleaks_scans (
    id uuid primary key default gen_random_uuid(),
    account_id uuid not null,
    scan_reference text not null unique,
    copyleaks_scan_id text,
    scan_type text not null default 'plagiarism',
    status text not null default 'created',
    credit_cost numeric(18, 6) not null default 0,
    title text,
    submitted_text text,
    submitted_file_name text,
    submit_payload jsonb not null default '{}'::jsonb,
    submit_response jsonb not null default '{}'::jsonb,
    result_payload jsonb,
    webhook_payload jsonb,
    error_message text,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    completed_at timestamptz
);

create index if not exists idx_copyleaks_scans_account_id
    on copyleaks_scans (account_id);

create index if not exists idx_copyleaks_scans_copyleaks_scan_id
    on copyleaks_scans (copyleaks_scan_id);

create index if not exists idx_copyleaks_scans_status
    on copyleaks_scans (status);

insert into utility_pricing_settings (
    service_code,
    service_name,
    category,
    credit_cost,
    billing_unit,
    is_active
) values (
    'copyleaks_premium_scan',
    'Copyleaks Premium Plagiarism Scan',
    'ai',
    50,
    'per_scan',
    true
)
on conflict (service_code) do nothing;

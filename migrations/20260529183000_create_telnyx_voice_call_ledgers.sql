create table if not exists telnyx_voice_calls (
    id uuid primary key default gen_random_uuid(),
    account_id uuid references accounts(id) on delete set null,
    call_id text not null unique,
    command_id text,
    call_control_id text unique,
    call_leg_id text,
    call_session_id text,
    connection_id text,
    direction text not null default 'outgoing',
    from_number text not null,
    to_number text not null,
    status text not null default 'created',
    telnyx_state text,
    hangup_cause text,
    hangup_source text,
    started_at timestamptz,
    answered_at timestamptz,
    ended_at timestamptz,
    last_event_type text,
    last_webhook_id text,
    last_raw_event jsonb,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create index if not exists idx_telnyx_voice_calls_account_created_at
    on telnyx_voice_calls (account_id, created_at desc);

create index if not exists idx_telnyx_voice_calls_call_control_id
    on telnyx_voice_calls (call_control_id)
    where call_control_id is not null;

create index if not exists idx_telnyx_voice_calls_status
    on telnyx_voice_calls (status);

create table if not exists telnyx_voice_events (
    id uuid primary key default gen_random_uuid(),
    webhook_id text unique,
    event_type text not null,
    call_id text,
    call_control_id text,
    call_leg_id text,
    call_session_id text,
    occurred_at timestamptz,
    payload jsonb not null,
    raw_event jsonb not null,
    received_at timestamptz not null default now()
);

create index if not exists idx_telnyx_voice_events_call_control_id_received_at
    on telnyx_voice_events (call_control_id, received_at desc);

create index if not exists idx_telnyx_voice_events_call_id_received_at
    on telnyx_voice_events (call_id, received_at desc)
    where call_id is not null;

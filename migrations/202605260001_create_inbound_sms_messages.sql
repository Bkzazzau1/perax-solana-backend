create table if not exists inbound_sms_messages (
    id uuid primary key default gen_random_uuid(),
    phone_number text not null,
    sender text not null,
    body text not null,
    provider_message_id text,
    provider_payload jsonb,
    received_at timestamptz not null default now(),
    created_at timestamptz not null default now()
);

create index if not exists idx_inbound_sms_messages_phone_received_at
    on inbound_sms_messages (phone_number, received_at desc);

create unique index if not exists idx_inbound_sms_messages_provider_message_id
    on inbound_sms_messages (provider_message_id)
    where provider_message_id is not null;

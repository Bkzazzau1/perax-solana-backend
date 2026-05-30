alter table provisioned_numbers
    add column if not exists telnyx_phone_number_id text,
    add column if not exists provider_status text,
    add column if not exists provider_payload jsonb,
    add column if not exists messaging_profile_id text,
    add column if not exists messaging_product text,
    add column if not exists last_provider_sync_at timestamptz,
    add column if not exists cancelled_at timestamptz;

create index if not exists idx_provisioned_numbers_telnyx_phone_number_id
    on provisioned_numbers (telnyx_phone_number_id)
    where telnyx_phone_number_id is not null;

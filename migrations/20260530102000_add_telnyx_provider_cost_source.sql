alter table provider_transactions
    add column if not exists provider_cost_source text;

alter table telnyx_voice_calls
    add column if not exists provider_cost_source text;

alter table provisioned_numbers
    add column if not exists provider_cost_source text;

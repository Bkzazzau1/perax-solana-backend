alter table provider_transactions
    add column if not exists credits_charged numeric(38, 9),
    add column if not exists estimated_usd_cost numeric(38, 9),
    add column if not exists provider_cost_currency text,
    add column if not exists margin_credits numeric(38, 9),
    add column if not exists margin_usd numeric(38, 9);

alter table telnyx_voice_calls
    add column if not exists estimated_usd_cost numeric(38, 9),
    add column if not exists provider_cost_currency text,
    add column if not exists margin_credits numeric(38, 9),
    add column if not exists margin_usd numeric(38, 9);

alter table provisioned_numbers
    add column if not exists estimated_usd_cost numeric(38, 9),
    add column if not exists provider_cost_currency text,
    add column if not exists margin_credits numeric(38, 9),
    add column if not exists margin_usd numeric(38, 9);

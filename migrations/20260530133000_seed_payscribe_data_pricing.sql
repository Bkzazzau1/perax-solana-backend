insert into utility_pricing_settings (
    service_code,
    service_name,
    category,
    credit_cost,
    billing_unit,
    is_active
) values (
    'payscribe_data_service_fee',
    'Payscribe Data Service Fee',
    'data',
    0,
    'per_transaction',
    true
)
on conflict (service_code) do nothing;

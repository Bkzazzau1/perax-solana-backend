insert into utility_pricing_settings (
    service_code,
    service_name,
    category,
    credit_cost,
    billing_unit,
    is_active
) values (
    'copy_ai_generate',
    'Copy AI Generation',
    'ai',
    10,
    'per_generation',
    true
)
on conflict (service_code) do nothing;

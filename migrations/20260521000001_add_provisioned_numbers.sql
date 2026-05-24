CREATE TABLE IF NOT EXISTS provisioned_numbers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    phone_number VARCHAR(32) NOT NULL UNIQUE,
    telnyx_order_id VARCHAR(64) NOT NULL,
    status VARCHAR(32) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_provisioned_numbers_account_id
ON provisioned_numbers (account_id);

alter table accounts
    add column if not exists email text,
    add column if not exists pex_wallet_address text;

create unique index if not exists idx_accounts_email
    on accounts (lower(email))
    where email is not null;

create unique index if not exists idx_accounts_pex_wallet_address
    on accounts (pex_wallet_address)
    where pex_wallet_address is not null;

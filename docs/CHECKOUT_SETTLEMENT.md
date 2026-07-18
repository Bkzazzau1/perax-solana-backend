# Authoritative Checkout Settlement Queue

## What changed

`POST /checkout/confirm` no longer trusts a product name, product cost, or credit balance supplied by the client.

The route now:

1. Requires an authenticated account.
2. Loads the active product price from `utility_pricing_settings`.
3. Loads the active `credits_per_usd` policy.
4. Verifies the beneficiary wallet against the authenticated account wallet when one is stored.
5. Generates a unique 32-byte settlement ID.
6. Derives the on-chain product ID as SHA-256 of the UTF-8 service code.
7. Creates a permanent `checkout_settlement_orders` row.
8. Acquires a PostgreSQL transaction advisory lock for the account.
9. Locks the checkout order row.
10. Calculates the real posted Credits balance from `credit_ledger`.
11. Inserts one idempotent debit and marks the order `credits_reserved` in the same transaction.
12. Returns the settlement ID and product-policy ID required by the on-chain settlement worker.

Legacy request fields remain accepted temporarily for user-interface compatibility, but they are ignored for all financial decisions.

## Idempotency and crash recovery

A unique partial index protects `(account_id, idempotency_key)`.

When the same key is submitted again:

- A `credits_reserved` order is returned without another debit.
- A `created` or `failed` order resumes the original atomic reservation under the same account lock.
- A `cancelled` order cannot be charged.

This handles a process stopping after the order insert but before the Credits reservation.

## Concurrent spending protection

Checkout reservations use:

```sql
select pg_advisory_xact_lock(hashtextextended(account_id, 0));
```

Every checkout debit for the same account is serialized until the transaction commits or rolls back. The balance is recalculated after the lock is acquired, preventing two concurrent orders from spending the same Credits.

## Settlement status

`checkout_settlement_orders.settlement_status` supports:

```text
pending
planned
funding
ready
finalized
failed
```

The checkout route only creates and reserves the order. It does not claim an on-chain settlement occurred.

The market-engine settlement worker must later:

1. Select a fresh APC observation.
2. Call `plan_settlement` using the stored settlement ID and SHA-256 product ID.
3. Follow the contract-returned mode.
4. Execute direct PEX, atomic market purchase, policy-vault funding, or hybrid funding.
5. Call `finalize_settlement`.
6. Store the settlement record address and final transaction signature.
7. Mark the order `finalized` only after confirmed on-chain finalization.

## Failure handling

A product must not be activated merely because Credits were reserved.

Service activation should require `settlement_status = 'finalized'` unless the product has a separately approved asynchronous-delivery policy.

A refund workflow is still required for orders that permanently fail after Credits reservation. It must insert a new idempotent positive `credit_ledger` entry; it must never delete or edit the original debit.

## Validation

```bash
python3 scripts/validate-checkout-settlement-source.py
sqlx migrate run
cargo test --all
cargo check --all-targets
```

The existing backend CI runs the source guard, migrations, tests, build check, and runtime smoke tests.

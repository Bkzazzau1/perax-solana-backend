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
12. Returns the settlement ID and product-policy ID used by the settlement worker.

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

## Settlement worker

The backend starts `spawn_checkout_settlement_worker` during application startup. The worker remains disabled when `PERAX_SETTLEMENT_EXECUTOR_URL` is empty.

When enabled, it:

1. Claims pending work with `FOR UPDATE SKIP LOCKED`.
2. Reclaims abandoned jobs after the claim timeout.
3. Sends the permanent settlement ID, SHA-256 product ID, factual funding method, quantity, and beneficiary to `POST /execute/settlement`.
4. Keeps transport and non-terminal failures retryable.
5. Records `planned`, `funding`, or `ready` progress without granting service.
6. Before recording `finalized`, checks the transaction signature through Solana RPC and confirms the settlement-record account is owned by the configured Pera-X program.
7. Stores the settlement record address and transaction signature.

Executor requests are idempotent because every retry uses the same settlement ID. The on-chain settlement PDA prevents a second independent settlement record from being created for the same ID.

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

A product must not be activated merely because Credits were reserved. Service activation should require `settlement_status = 'finalized'` unless the product has a separately approved asynchronous-delivery policy.

## Terminal-failure refunds

Only an executor response with both:

```text
status = failed
terminalFailure = true
```

can trigger an automatic refund.

The worker then:

1. Acquires the same account advisory lock used for checkout debits.
2. Locks the order row.
3. Refuses to refund a finalized order.
4. Checks whether a refund ledger entry already exists.
5. Inserts a new positive `credit_ledger` entry with source `checkout_refund`.
6. Marks the order and settlement failed in the same transaction.

The original debit is never deleted or edited.

## Executor configuration

```env
PERAX_SETTLEMENT_EXECUTOR_URL=http://127.0.0.1:8790
PERAX_SETTLEMENT_EXECUTOR_TOKEN=replace-with-a-private-service-token
PERAX_SETTLEMENT_INTERVAL_SECONDS=30
```

The backend appends `/execute/settlement` when the URL does not already include it.

## Validation

```bash
python3 scripts/validate-checkout-settlement-source.py
sqlx migrate run
cargo test --all
cargo check --all-targets
```

The existing backend CI runs the source guard, migrations, tests, build check, and runtime smoke tests.

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def require(text: str, value: str, label: str) -> None:
    if value not in text:
        raise SystemExit(f"{label}: missing required source guard: {value}")


def forbid(text: str, value: str, label: str) -> None:
    if value in text:
        raise SystemExit(f"{label}: forbidden source pattern remains: {value}")


routes = read("src/domains/checkout/routes.rs")
ledger = read("src/domains/checkout/ledger.rs")
migration = read("migrations/20260718190000_create_checkout_settlement_orders.sql")

require(routes, "account: AuthenticatedAccount", "checkout routes")
require(routes, "pricing::get_utility_price", "checkout routes")
require(routes, "active credit pricing policy", "checkout routes")
require(routes, "reserve_checkout_credits", "checkout routes")
require(routes, "reserve_or_return_existing", "checkout routes")
require(routes, "settlement_product_id_hex = sha256_hex", "checkout routes")
require(routes, "beneficiaryWallet must match the authenticated account wallet", "checkout routes")
require(routes, "Older clients may still send these fields. They are deliberately ignored.", "checkout routes")
forbid(routes, "remaining_credits = payload.credit_balance", "checkout routes")
forbid(routes, "let confirmed = credit_cost > 0.0", "checkout routes")
forbid(routes, "payload.credit_cost.max", "checkout routes")

require(ledger, "pg_advisory_xact_lock", "checkout ledger")
require(ledger, "for update", "checkout ledger")
require(ledger, "ledger_status = 'posted'", "checkout ledger")
require(ledger, "current_balance + 0.000001 < amount", "checkout ledger")
require(ledger, "on conflict (source, source_reference)", "checkout ledger")
require(ledger, "order_status = 'credits_reserved'", "checkout ledger")
require(ledger, "tx.commit().await?", "checkout ledger")

require(migration, "checkout_settlement_orders", "checkout migration")
require(migration, "settlement_id_hex char(64) not null unique", "checkout migration")
require(migration, "settlement_product_id_hex char(64) not null", "checkout migration")
require(migration, "credit_ledger_id uuid", "checkout migration")
require(migration, "idx_checkout_settlement_orders_account_idempotency", "checkout migration")
require(migration, "settlement_status in ('pending', 'planned', 'funding', 'ready', 'finalized', 'failed')", "checkout migration")

print("Authoritative checkout settlement source guards passed.")

# Pera-X Utility Gateway

Axum 0.8 service scaffold for high-throughput WebRTC signaling, Solana settlement tracking, and downstream B2B proxy integrations.

## Layout

- `src/domains/auth`: virtual API key extraction and account verification.
- `src/domains/b2b_gateway`: Claude and Copyleaks proxy entrypoints.
- `src/domains/payments`: utility payment confirmation, grant flow, and admin test routes.
- `src/domains/solana`: background treasury listener, dynamic burn policy, payment event ingestion, and settlement burner workers.
- `src/domains/telecom`: Telnyx-facing voice, WebRTC, and SMS routes.
- `src/infra`: PostgreSQL and Redis connection setup.
- `scripts`: local helper scripts for runtime testing.

## Anchor Contract Link

The backend is linked to the local Anchor workspace at:

```text
C:\PROJECTS\smartcontract PEX\perax-ecosystem\perax-contracts
```

The current Anchor program id is still the starter placeholder:

```text
11111111111111111111111111111111
```

Install the Solana and Anchor CLIs, then run `anchor keys sync` and `anchor build` in the Anchor workspace. After that, copy the real program id into `PERAX_PROGRAM_ID` in `.env`.

## Trading Company SPL Token Account

`TRADING_CO_TREASURY` must be the **Trading Company SPL token account** that holds Pera-X tokens.

It is **not** the normal wallet address. On Solana, the wallet owner controls one or more SPL token accounts. For Pera-X utility flow, the backend needs the SPL token account that receives, holds, and later burns approved Pera-X tokens.

This account is used for:

```text
1. Confirming Pera-X utility payments
2. Matching payments sent by the smart contract
3. Holding Trading Company utility tokens
4. Approved burn execution from Trading Company balance
5. Future buyback/top-up tracking
```

Example environment value:

```env
TRADING_CO_TREASURY=replace-with-trading-company-spl-token-account
```

## Burn Execution Policy

Pera-X uses a controlled burn workflow:

```text
manual   = declare/store burn decisions only; no execution
approved = execute only decisions already marked as approved
```

Use `manual` for development and early testing. Use `approved` only when production wallets, approval controls, and real SPL burn execution are ready.

## Utility Payment Confirmation Flow

The smart contract sends Pera-X utility payments to the Trading Company token account and emits a reference. The backend stores and confirms that reference before granting service access.

```text
User pays Pera-X on-chain
        ↓
Smart contract sends tokens to Trading Company SPL token account
        ↓
Backend ingests the utility payment reference
        ↓
Payment status becomes confirmed
        ↓
Backend grants the requested service/access
        ↓
Payment status becomes granted
```

Utility payment admin/test endpoints:

```text
GET  /admin/api/utility-payments
POST /admin/api/utility-payments/ingest
POST /admin/api/utility-payments/grant
```

These routes are for controlled testing and admin operations before full Solana event parsing is connected.

## Local Run

Copy the environment template:

```bash
cp .env.example .env
```

Set the required environment values:

```powershell
$env:DATABASE_URL="postgres://postgres:postgres@localhost:5432/perax"
$env:TRADING_CO_TREASURY="replace-with-trading-company-spl-token-account"
$env:JWT_SECRET="replace-with-at-least-32-characters"
$env:BURN_EXECUTION_MODE="manual"
cargo run
```

The service listens on `0.0.0.0:8080` by default.

## Admin Burn Endpoints

```text
GET  /admin/api/burn-preview
GET  /admin/api/burn-decisions
POST /admin/api/burn-decisions/declare
POST /admin/api/burn-decisions/status
```

These endpoints let the team preview burn decisions, declare test decisions, review history, and approve/cancel decisions before real execution is enabled.

## Runtime Burn Admin Test

After the backend is running locally, use the helper script to test the admin burn flow:

```bash
chmod +x scripts/test-burn-admin.sh
./scripts/test-burn-admin.sh
```

The script will call:

```text
/healthz
/admin/api/burn-preview
/admin/api/burn-decisions/declare
/admin/api/burn-decisions/status
/admin/api/burn-decisions
```

You can also set a custom backend URL:

```bash
BASE_URL="http://127.0.0.1:8080" ./scripts/test-burn-admin.sh
```

This test declares and approves a safe test burn decision. It does not execute real token burning while `BURN_EXECUTION_MODE=manual`.

## Runtime Utility Payment Test

After the backend is running locally, use the helper script to test the utility payment confirmation flow:

```bash
chmod +x scripts/test-utility-payments.sh
./scripts/test-utility-payments.sh
```

The script will call:

```text
/healthz
/admin/api/utility-payments/ingest
/admin/api/utility-payments/grant
/admin/api/utility-payments
```

You can also set a custom backend URL or custom payment reference:

```bash
BASE_URL="http://127.0.0.1:8080" \
REFERENCE_HEX="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" \
./scripts/test-utility-payments.sh
```

This test ingests a safe admin utility payment, marks it as granted, and lists granted payments. It does not require real token movement while testing locally or in CI.

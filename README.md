# Pera-X Utility Gateway

Axum 0.8 service scaffold for high-throughput WebRTC signaling, Solana settlement tracking, and downstream B2B proxy integrations.

## Layout

- `src/domains/auth`: virtual API key extraction and account verification.
- `src/domains/b2b_gateway`: Claude and Copyleaks proxy entrypoints.
- `src/domains/solana`: background treasury listener, dynamic burn policy, and settlement burner workers.
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

## Burn Execution Policy

Pera-X uses a controlled burn workflow:

```text
manual   = declare/store burn decisions only; no execution
approved = execute only decisions already marked as approved
```

Use `manual` for development and early testing. Use `approved` only when production wallets, approval controls, and real SPL burn execution are ready.

## Local Run

Copy the environment template:

```bash
cp .env.example .env
```

Set the required environment values:

```powershell
$env:DATABASE_URL="postgres://postgres:postgres@localhost:5432/perax"
$env:TRADING_CO_TREASURY="treasury-public-key"
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

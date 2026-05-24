# Pera-X Utility Gateway

Axum 0.8 service scaffold for high-throughput WebRTC signaling, Solana settlement tracking, and downstream B2B proxy integrations.

## Layout

- `src/domains/auth`: virtual API key extraction and account verification.
- `src/domains/b2b_gateway`: Claude and Copyleaks proxy entrypoints.
- `src/domains/solana`: background treasury listener, dynamic burn policy, and settlement burner workers.
- `src/domains/telecom`: Telnyx-facing voice, WebRTC, and SMS routes.
- `src/infra`: PostgreSQL and Redis connection setup.

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

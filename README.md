# Pera-X Utility Gateway

Axum 0.8 service scaffold for high-throughput WebRTC signaling, Solana settlement tracking, and downstream B2B proxy integrations.

## Layout

- `src/domains/auth`: virtual API key extraction and account verification.
- `src/domains/b2b_gateway`: Claude and Copyleaks proxy entrypoints.
- `src/domains/solana`: background treasury listener and settlement burner workers.
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

## Local Run

Set the required environment values:

```powershell
$env:DATABASE_URL="postgres://postgres:postgres@localhost:5432/perax"
$env:TRADING_CO_TREASURY="treasury-public-key"
$env:JWT_SECRET="replace-with-at-least-32-characters"
cargo run
```

The service listens on `0.0.0.0:8080` by default.

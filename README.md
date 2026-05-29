# Pera-X Utility Gateway

Axum 0.8 service scaffold for high-throughput WebRTC signaling, Solana settlement tracking, Credits funding, Trading Company revenue tracking, and downstream B2B proxy integrations.

## Layout

- `src/domains/auth`: virtual API key extraction and account verification.
- `src/domains/b2b_gateway`: Claude and Copyleaks proxy entrypoints.
- `src/domains/payments`: utility payment confirmation, grant flow, Trading Company status, and admin test routes.
- `src/domains/solana`: background treasury listener, dynamic burn policy, payment event ingestion, market-condition monitoring, revenue ledger, and settlement burner workers.
- `src/domains/telecom`: Telnyx-facing voice, WebRTC, and SMS routes.
- `src/domains/credits`: Credits purchase flow for PEX, card, stablecoin, and virtual account funding.
- `src/infra`: PostgreSQL and Redis connection setup.
- `scripts`: local helper scripts for runtime testing.

## Smart Contract Alignment

The backend is aligned with the current Pera-X Anchor program.

Current Pera-X program id:

```text
FqEiSx5vujh2vi3yk12NaZMXhjMSaKovGUuzcKiAgshn
```

Current devnet state PDA:

```text
8LNUe8ud9Lrtt1HmuS132YoGs5tBNEeWeviNJwWDkHWT
```

Current devnet PEX mint:

```text
DnkAW3B1ckzW6eimgSBNPK3XTt83wMiZRETy8iF3gdsn
```

The backend must never use the old placeholder program id:

```text
11111111111111111111111111111111
```

## PEX Token Policy

| Item | Value |
|---|---:|
| Token | Pera-X |
| Symbol | PEX |
| Supply | 1,000,000,000 PEX |
| Decimals | 6 |
| Initial Price | $0.000012 |
| Initial Valuation | $12,000 |
| Initial Liquidity | 380,000,000 PEX + $4,560 USDC |
| Liquidity Venue | Meteora DLMM |
| Unlock Authority | Market-condition oracle only |
| Manual/Multisig Release Approval | Disabled |

## Trading Company SPL Token Accounts

The backend separates the Trading Company locked/strategic SPL token account from the Trading Company revenue SPL token account.

```text
TRADING_COMPANY_TOKEN_ACCOUNT = locked/strategic account
TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT = revenue account for PEX-for-Credits payments and burns
```

They must be different.

These accounts are used for:

```text
1. Confirming Pera-X utility payments
2. Matching payments sent by the smart contract
3. Holding Trading Company utility/revenue tokens
4. Approved burn execution from Trading Company revenue balance
5. Future buyback/top-up tracking
```

Trading Company status endpoint:

```text
GET /admin/api/trading-company-status
```

## Market-Condition Release Policy

Pera-X uses oracle-controlled market-condition release approval.

```text
Market bot monitors market every 10 minutes
Oracle verifies TWAP, liquidity, volume, buy pressure, cooldown, daily cap, monthly cap, and business purpose
Oracle records release approval on-chain when all gates are satisfied
ReleaseRecord PDA prevents duplicate release IDs
Emergency pause can stop the system if market/security risk is detected
```

The backend config must keep:

```env
PEX_UNLOCK_REQUIRES_MANUAL_APPROVAL=false
```

## Burn Execution Policy

Pera-X uses a controlled burn workflow:

```text
manual   = declare/store burn decisions only; no execution
approved = execute only decisions already marked as approved
```

Use `manual` for development and early testing. Use `approved` only when production wallets, approval controls, and real SPL burn execution are ready.

## Utility Payment Confirmation Flow

The smart contract sends Pera-X utility payments to the Trading Company revenue token account and emits a reference. The backend stores and confirms that reference before granting service access.

```text
User pays Pera-X on-chain
        ↓
Smart contract sends tokens to Trading Company revenue SPL token account
        ↓
Backend ingests/verifies the utility payment reference
        ↓
Payment status becomes confirmed
        ↓
Backend grants the requested service/access or Credits
        ↓
Payment status becomes granted
```

Utility payment admin/test endpoints:

```text
GET  /admin/api/trading-company-status
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
$env:SOLANA_RPC_URL="https://api.devnet.solana.com"
$env:PERAX_ANCHOR_WORKSPACE="C:/PROJECTS/Pera-X-ecosystem/perax-contracts"
$env:PERAX_PROGRAM_ID="FqEiSx5vujh2vi3yk12NaZMXhjMSaKovGUuzcKiAgshn"
$env:PERAX_STATE_PDA="8LNUe8ud9Lrtt1HmuS132YoGs5tBNEeWeviNJwWDkHWT"
$env:PEX_MINT_ADDRESS="DnkAW3B1ckzW6eimgSBNPK3XTt83wMiZRETy8iF3gdsn"
$env:TRADING_COMPANY_TOKEN_ACCOUNT="replace-with-locked-strategic-spl-token-account"
$env:TRADING_COMPANY_REVENUE_TOKEN_ACCOUNT="replace-with-revenue-spl-token-account"
$env:PEX_UNLOCK_REQUIRES_MANUAL_APPROVAL="false"
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
/admin/api/trading-company-status
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

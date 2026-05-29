# Pera-X Supply-Control Executor

This service is the isolated automatic executor for Pera-X market-condition burns.

It receives burn parameters from the backend worker and calls the smart contract instruction:

```text
execute_market_condition_burn
```

Admin does not approve or cancel burns. The backend/oracle system calculates the daily burn schedule, and this executor signs the transaction using secured system keypairs.

## Why this is separate from the main backend

The executor holds signing access for:

```text
PERAX_AUTHORITY_KEYPAIR_PATH
TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH
```

These keys should not live inside the public API process, frontend, logs, or GitHub.

## Install

```bash
cd supply-control-executor
npm install
```

## Environment

Copy the template:

```bash
cp .env.example .env
```

Set:

```env
PORT=8787
SOLANA_RPC_URL=https://api.devnet.solana.com
PERAX_SUPPLY_CONTROL_EXECUTOR_TOKEN=replace-with-long-random-token
PERAX_AUTHORITY_KEYPAIR_PATH=/secure/perax/authority.json
TRADING_COMPANY_AUTHORITY_KEYPAIR_PATH=/secure/perax/trading-company-authority.json
```

Never commit real keypair files.

## Run

```bash
npm start
```

Health check:

```bash
curl http://127.0.0.1:8787/health
```

## Backend configuration

In the main backend `.env`:

```env
BURN_EXECUTION_MODE=automatic
PERAX_SUPPLY_CONTROL_EXECUTOR_URL=http://127.0.0.1:8787/execute/market-condition-burn
PERAX_SUPPLY_CONTROL_EXECUTOR_TOKEN=replace-with-same-long-random-token
```

Use `BURN_EXECUTION_MODE=disabled` for local prepare/view-only mode.

## Request payload

The backend worker sends:

```json
{
  "solanaRpcUrl": "https://api.devnet.solana.com",
  "programId": "FqEiSx5vujh2vi3yk12NaZMXhjMSaKovGUuzcKiAgshn",
  "statePda": "8LNUe8ud9Lrtt1HmuS132YoGs5tBNEeWeviNJwWDkHWT",
  "pexMintAddress": "DnkAW3B1ckzW6eimgSBNPK3XTt83wMiZRETy8iF3gdsn",
  "tradingCompanyRevenueTokenAccount": "...",
  "decisionIdHex": "64-hex-character-decision-id",
  "amountBaseUnits": 1000000,
  "eligibleRevenueBaseUnits": 10000000,
  "burnRateBps": 1000,
  "marketHealthScore": 55,
  "observedAtUnix": 1760000000
}
```

## Safety checks

The executor checks:

```text
1. Bearer token if configured.
2. Program ID and state PDA are valid.
3. State PDA equals derived [b"perax-state"] PDA.
4. Trading Company revenue token account is the ATA of trading company authority for PEX mint.
5. decisionIdHex is 32 bytes / 64 hex characters.
6. Burn record PDA does not already exist.
7. Amounts and observedAt are positive.
```

The smart contract performs the final enforcement:

```text
1. Program is not paused.
2. Emergency pause is off.
3. burnRateBps matches marketHealthScore.
4. amount = eligibleRevenueAmount × burnRateBps / 10,000.
5. Trading Company revenue token account matches state.
6. Trading Company authority owns the revenue token account.
7. BurnExecutionRecord PDA prevents duplicate decision IDs.
```

## Current status

This executor is ready for devnet integration testing after the correct authority and Trading Company keypair paths are configured on the machine that runs the executor.

# Pera-X Solana Event Parser Plan

This document defines how the backend will automatically convert smart contract events into `utility_payments` records.

## Target Smart Contract Event

The backend will focus first on the smart contract event below:

```text
UtilityPaymentReceived
```

Expected event fields:

```text
payer
 token_mint
 trading_company_token_account
 amount
 reference
```

The `reference` is the most important field. It connects the on-chain payment to the service request created by the backend or frontend.

## End-to-End Flow

```text
1. User selects a Pera-X utility service
2. Backend/frontend generates a 32-byte reference
3. User pays Pera-X through the smart contract
4. Smart contract transfers tokens to Trading Company SPL token account
5. Smart contract emits UtilityPaymentReceived
6. Backend scans confirmed Solana transactions/logs
7. Backend decodes the event
8. Backend inserts/updates utility_payments
9. Backend marks payment as confirmed
10. Backend grants the requested service/access
```

## Parser Responsibilities

The Solana event parser should:

```text
1. Read recent signatures involving the Pera-X program id and/or Trading Company SPL token account
2. Fetch confirmed transactions with getTransaction
3. Read transaction logs
4. Detect Anchor event logs
5. Decode UtilityPaymentReceived
6. Validate the Trading Company SPL token account matches TRADING_CO_TREASURY
7. Normalize the reference as 64-character lowercase hex
8. Call ingest_utility_payment_event()
9. Avoid duplicate processing using tx_signature and reference_hex
```

## Safety Rules

```text
1. Do not grant service just because a transaction touched the wallet
2. Only grant after matching a valid UtilityPaymentReceived event
3. Always verify trading_company_token_account
4. Always verify token_mint when PERAX_TOKEN_MINT is introduced
5. Treat duplicate references as idempotent, not as new payments
6. Store raw_event for auditability
7. Never execute burn from event parser directly
```

## Current Temporary Flow

Until real Solana event parsing is connected, the admin/test route can simulate an event:

```text
POST /admin/api/utility-payments/ingest
POST /admin/api/utility-payments/grant
```

This lets us test the backend confirmation and grant pipeline before real on-chain event decoding.

## Next Implementation Steps

```text
1. Add PERAX_TOKEN_MINT environment variable
2. Add event parser module for Anchor program logs
3. Add processed transaction checkpoint table
4. Decode UtilityPaymentReceived events
5. Connect parser to ingest_utility_payment_event()
6. Add CI runtime test using sample event payload
7. Later, connect real Solana RPC event scan
```

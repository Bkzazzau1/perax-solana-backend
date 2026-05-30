# Pyth Network and Vercel v0 Smoke Test Commands

This guide covers the backend foundation for:

- Pyth Network decentralized price oracle reads
- Vercel v0 Platform API code/app generation

Run after:

```powershell
git pull origin main
cargo check
cargo run
```

## Required env

```env
PYTH_PRICE_SERVICE_URL=https://hermes.pyth.network
PYTH_SOL_PRICE_FEED_ID=replace-with-real-sol-feed-id
PYTH_BTC_PRICE_FEED_ID=replace-with-real-btc-feed-id
PYTH_ETH_PRICE_FEED_ID=replace-with-real-eth-feed-id
PYTH_USDC_PRICE_FEED_ID=replace-with-real-usdc-feed-id
PYTH_PEX_PRICE_FEED_ID=replace-with-real-pex-feed-id

V0_BASE_URL=https://api.v0.dev
V0_API_KEY=replace-with-v0-api-key
```

You can test Pyth with direct `feedId` even before setting symbol envs.

## Pyth Network

### 1. Check Pyth status

```powershell
curl http://127.0.0.1:8080/pyth/status
```

Expected:

```text
configured = true
priceServiceUrl = https://hermes.pyth.network
knownSymbols includes SOL, BTC, ETH, USDC, PEX
```

### 2. Latest price by feed ID

```powershell
curl "http://127.0.0.1:8080/pyth/latest?feedId=YOUR_FEED_ID_WITHOUT_0X"
```

The backend calls Hermes using this format:

```text
GET {PYTH_PRICE_SERVICE_URL}/v2/updates/price/latest?ids[]={feedId}&parsed=true
```

Expected:

```text
accepted = true
feedId
price
confidence
exponent
publishTime
providerResponse
```

The backend stores a row in:

```text
pyth_price_snapshots
```

### 3. Latest price by symbol

This requires the matching env value, for example `PYTH_SOL_PRICE_FEED_ID`.

```powershell
curl "http://127.0.0.1:8080/pyth/latest?symbol=SOL"
```

Supported symbol shortcuts:

```text
SOL
BTC
ETH
USDC
PEX
```

### 4. Pyth notes

- PEX may not have a Pyth feed immediately.
- Until PEX has a feed, backend can use Pyth for SOL, USDC, BTC, ETH and use internal/DEX price for PEX.
- Feed IDs may be provided with or without `0x`; backend removes `0x`.

## Vercel v0

### 1. Check v0 status

```powershell
curl http://127.0.0.1:8080/v0/status
```

Expected:

```text
configured = true when V0_API_KEY is set
baseUrl = https://api.v0.dev
```

### 2. Quote v0 code generation

```powershell
curl -X POST http://127.0.0.1:8080/v0/chats/quote `
  -H "Content-Type: application/json" `
  -d '{"mode":"create_chat"}'
```

Expected:

```text
serviceCode = v0_code_generation
creditCost = backend configured price
```

No Credits are debited here.

### 3. Create v0 chat/code generation

Use a real `accountId` that has enough posted Credits.

```powershell
curl -X POST http://127.0.0.1:8080/v0/chats/create `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","message":"Build a responsive landing page for PeraX with dark navy background, cyan accents, hero section, utility cards, pricing section, and a modern call to action.","system":"You are building production-ready React and Tailwind UI for PeraX.","chatPrivacy":"private","modelConfiguration":{"model":"v0-1.5-md"},"refId":"v0-test-001"}'
```

Backend builds the provider payload as:

```json
{
  "message": "Build a responsive landing page...",
  "system": "optional system instruction",
  "chatPrivacy": "private",
  "modelConfiguration": {
    "model": "v0-1.5-md"
  }
}
```

Expected backend response:

```text
requestReference = v0-test-001
status = submitted if live v0 accepted request
status = created if saved but provider call failed/deferred
providerResponse contains v0 API response
```

Ledger check:

```text
credit_ledger.source = v0_code_generation
credit_ledger.source_reference = v0-test-001
```

Database check:

```text
v0_generation_requests.request_reference = v0-test-001
```

### 4. Get v0 result by reference

```powershell
curl http://127.0.0.1:8080/v0/chats/result/v0-test-001
```

Or by returned v0 chat ID:

```powershell
curl http://127.0.0.1:8080/v0/chats/result/RETURNED_CHAT_ID
```

## Important implementation rules

Pyth:

```text
- Price reads do not debit Credits.
- Price snapshots are stored for audit and later pricing logic.
- PEX feed may be unavailable at early stage.
```

v0:

```text
- Quote does not debit Credits.
- Create chat debits Credits before provider submission.
- Provider response is stored even if deferred or failed.
- Final payload may need adjustment after real V0_API_KEY testing.
```

# Payscribe Smoke Test Commands

Payscribe is currently wired for Nigerian data vending only. Electricity will be added after data vending is confirmed.

Run these after:

```powershell
git pull origin main
cargo check
cargo run
```

Required local env:

```env
PAYSCRIBE_BASE_URL=https://sandbox.payscribe.ng/api/v1
PAYSCRIBE_API_KEY=your_payscribe_api_token
PAYSCRIBE_DATA_CREDITS_PER_NAIRA=1
PAYSCRIBE_DATA_SERVICE_FEE_CREDITS=0
```

## 1. Check Payscribe backend status

```powershell
curl http://127.0.0.1:8080/payscribe/status
```

Expected:

```text
configured = true
supportedServices includes data
```

## 2. Lookup data plans

```powershell
curl "http://127.0.0.1:8080/payscribe/data/lookup?network=mtn"
```

Supported networks:

```text
mtn
glo
airtel
9mobile
smile
dstvshowmax
```

Save a valid plan code such as:

```text
PSPLAN_177
```

## 3. Quote data plan

```powershell
curl "http://127.0.0.1:8080/payscribe/data/quote?network=mtn&plan=PSPLAN_177"
```

Expected response:

```text
planAmount
chargeCredits
pricingPolicy
```

No Credits are debited here.

## 4. Vend data to one recipient

Replace `accountId` with a real account/user UUID that already has enough posted Credits in `credit_ledger`.

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/data/vend `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","network":"mtn","plan":"PSPLAN_177","recipient":"08169254598","refId":"my-data-ref-001"}'
```

Expected:

```text
providerStatus = processing or success-like provider status
chargeCredits populated
providerReference = my-data-ref-001
providerTransId may be populated
```

## 5. Vend data to multiple recipients

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/data/vend `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","network":"mtn","plan":"PSPLAN_177","recipient":["08169254598","07038067493"],"refId":"my-data-bulk-ref-001"}'
```

Payscribe validates each recipient independently. Invalid numbers may be returned in provider response errors.

## 6. Requery transaction

You can use either Payscribe `trans_id` or your own `refId`.

```powershell
curl "http://127.0.0.1:8080/payscribe/requery?trans_id=my-data-ref-001"
```

Payscribe recommends requerying pending transactions at most once per minute.

## Ledger checks

After successful vend request, confirm:

```text
credit_ledger.source = payscribe_data
credit_ledger.source_reference = your refId
payscribe_transactions.provider_reference = your refId
payscribe_transactions.charge_credits is populated
payscribe_transactions.provider_status is processing/success/failure depending on provider response
```

## Reversal check

If Payscribe rejects the vend request immediately, backend should create:

```text
credit_ledger.source = payscribe_data_reversal
```

This protects the user from losing Credits when provider rejects the order.

## Important implementation rule

Frontend must not decide the data charge.

Backend calculates charge by:

```text
plan amount from Payscribe /data/lookup
× PAYSCRIBE_DATA_CREDITS_PER_NAIRA
+ PAYSCRIBE_DATA_SERVICE_FEE_CREDITS
```

The user should see `/payscribe/data/quote` before confirming `/payscribe/data/vend`.

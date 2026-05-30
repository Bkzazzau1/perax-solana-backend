# Payscribe Electricity Smoke Tests

Payscribe electricity is now wired for:

- customer/meter validation
- electricity quote
- electricity vending
- requery
- Credit debit before vend
- Credit reversal if Payscribe rejects immediately

## Required env

```env
PAYSCRIBE_BASE_URL=https://sandbox.payscribe.ng/api/v1
PAYSCRIBE_API_KEY=your_payscribe_api_token
PAYSCRIBE_ELECTRICITY_VALIDATE_PATH=/electricity/validate
PAYSCRIBE_ELECTRICITY_VEND_PATH=/electricity/vend
PAYSCRIBE_ELECTRICITY_CREDITS_PER_NAIRA=1
PAYSCRIBE_ELECTRICITY_SERVICE_FEE_CREDITS=0
```

## 1. Check electricity status

```powershell
curl http://127.0.0.1:8080/payscribe/electricity/status
```

Expected:

```text
readyForValidation = true
readyForVending = true
```

## 2. Validate prepaid meter/customer

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/validate `
  -H "Content-Type: application/json" `
  -d '{"disco":"ikedc","meterNumber":"54150143102","meterType":"prepaid","amount":1000,"customerPhone":"07038067493"}'
```

Expected:

```text
accepted = true or false based on provider response
providerStatus = HTTP status code
providerResponse.message.details.customer_name
providerResponse.message.details.address
```

## 3. Quote electricity purchase

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/quote `
  -H "Content-Type: application/json" `
  -d '{"service":"ikedc","meterNumber":"54150143102","meterType":"prepaid","amount":1000}'
```

Expected:

```text
accepted = true
amount = 1000
chargeCredits calculated by backend
validation contains Payscribe validation response
```

No Credits are debited here.

## 4. Vend electricity

Replace `accountId` with a real account/user UUID that has enough posted Credits.

Use `customerName` from validation response.

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/vend `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","meterNumber":"54150143102","meterType":"prepaid","amount":1000,"service":"ikedc","phone":"07038067493","customerName":"FEMI AGBEBUNMI","address":"26, DAISI OKEOWO AGBALA ,IKORODU","refId":"my-electricity-ref-001"}'
```

Expected response may include:

```text
providerStatus = success or processing/pending provider status
providerTransId populated
providerPayload.message.details.token
providerPayload.message.details.unit
providerPayload.message.details.tariff
providerPayload.message.details.tax_amount
```

## 5. Requery electricity transaction

You can requery by your `refId` or Payscribe `trans_id`.

```powershell
curl "http://127.0.0.1:8080/payscribe/requery?trans_id=my-electricity-ref-001"
```

Payscribe recommends requerying pending transactions at most once per minute.

## Ledger checks

After a successful vend request, confirm:

```text
credit_ledger.source = payscribe_electricity
credit_ledger.source_reference = your refId
payscribe_transactions.service_type = electricity
payscribe_transactions.provider_reference = your refId
payscribe_transactions.charge_credits is populated
payscribe_transactions.provider_status is success/processing/failure depending on provider response
```

## Reversal check

If Payscribe rejects the vend request immediately, backend should create:

```text
credit_ledger.source = payscribe_electricity_reversal
```

## Supported discos

```text
ikedc
ekedc
eedc
phedc
aedc
ibedc
kedco
jed
```

## Important rules

Electricity amount must be at least NGN 1,000.

Safe flow:

```text
validate customer/meter
quote electricity charge
debit Credits
vend electricity
store token/receipt response
requery pending transaction
reverse Credits only if provider rejects immediately
```

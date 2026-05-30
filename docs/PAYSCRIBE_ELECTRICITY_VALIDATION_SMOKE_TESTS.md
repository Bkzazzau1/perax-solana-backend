# Payscribe Electricity Validation Smoke Tests

This document is for the current electricity foundation only.

Current electricity scope:

- customer/meter validation only
- no Credit debit yet
- no electricity vending yet

Electricity vending should be added only after the exact Payscribe vend payload is confirmed from the Payscribe dashboard/docs.

## Required env

```env
PAYSCRIBE_BASE_URL=https://sandbox.payscribe.ng/api/v1
PAYSCRIBE_API_KEY=your_payscribe_api_token
PAYSCRIBE_ELECTRICITY_VALIDATE_PATH=/electricity/validate
PAYSCRIBE_ELECTRICITY_VEND_PATH=
```

`PAYSCRIBE_ELECTRICITY_VALIDATE_PATH` is configurable because the exact Payscribe electricity validation route may differ from the default.

## 1. Check electricity status

```powershell
curl http://127.0.0.1:8080/payscribe/electricity/status
```

Expected:

```text
readyForValidation = true
readyForVending = false
```

## 2. Validate prepaid meter/customer

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/validate `
  -H "Content-Type: application/json" `
  -d '{"disco":"ikeja-electric","meterNumber":"12345678901","meterType":"prepaid","amount":1000,"customerPhone":"08123456789"}'
```

Expected:

```text
accepted = true or false based on provider response
providerStatus = HTTP status code
providerResponse = Payscribe validation response
```

## 3. Validate postpaid meter/customer

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/validate `
  -H "Content-Type: application/json" `
  -d '{"disco":"eko-electric","meterNumber":"12345678901","meterType":"postpaid","amount":1000,"customerPhone":"08123456789"}'
```

## 4. Alternative field names

The backend accepts either:

```text
disco or provider
meterNumber or meterNo
```

Example:

```powershell
curl -X POST http://127.0.0.1:8080/payscribe/electricity/validate `
  -H "Content-Type: application/json" `
  -d '{"provider":"abuja-electric","meterNo":"12345678901","meterType":"prepaid"}'
```

## Important rules

Do not add electricity vend until we confirm the exact Payscribe payload for:

```text
disco/provider field name
meter number field name
meter type field values
amount field
customer phone/name rules
reference field
response token/receipt format
requery behavior
```

When vending is added, the safe flow must be:

```text
validate customer/meter
quote electricity charge
debit Credits
vend electricity
store transaction
requery pending transaction
reverse Credits only if provider rejects immediately
```

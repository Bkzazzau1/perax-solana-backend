# Payscribe Production Readiness Runbook

Payscribe is used in Pera-X for:

- Data vending
- Electricity validation
- Electricity vending

Paystack remains responsible for deposits. Payscribe should not fund user Credits. Payscribe only consumes user Credits for utility purchases.

## Required env

```env
PAYSCRIBE_BASE_URL=https://sandbox.payscribe.ng/api/v1
PAYSCRIBE_API_KEY=replace-with-real-api-token
PAYSCRIBE_SECRET_KEY=replace-if-used
PAYSCRIBE_WEBHOOK_SECRET=replace-if-used

PAYSCRIBE_DATA_CREDITS_PER_NAIRA=1
PAYSCRIBE_DATA_SERVICE_FEE_CREDITS=0

PAYSCRIBE_ELECTRICITY_VALIDATE_PATH=/electricity/validate
PAYSCRIBE_ELECTRICITY_VEND_PATH=/electricity/vend
PAYSCRIBE_ELECTRICITY_CREDITS_PER_NAIRA=1
PAYSCRIBE_ELECTRICITY_SERVICE_FEE_CREDITS=0
```

## Active backend routes

```text
GET  /payscribe/status
GET  /payscribe/data/lookup?network=mtn
GET  /payscribe/data/quote?network=mtn&plan=PSPLAN_177
POST /payscribe/data/vend
POST /payscribe/electricity/validate
POST /payscribe/electricity/quote
POST /payscribe/electricity/vend
GET  /payscribe/requery?trans_id=REFERENCE_OR_TRANS_ID
```

## Data vending flow

Safe flow:

```text
lookup data plans
quote selected plan
check user Credit balance implicitly through debit
vend data
store transaction
requery if pending
reverse Credits if provider rejects immediately
```

### Data lookup

```http
GET /payscribe/data/lookup?network=mtn
```

### Data quote

```http
GET /payscribe/data/quote?network=mtn&plan=PSPLAN_177
```

Expected response:

```text
planAmount
chargeCredits
pricingPolicy
```

### Data vend

```json
{
  "accountId": "00000000-0000-0000-0000-000000000000",
  "network": "mtn",
  "plan": "PSPLAN_177",
  "recipient": "08169254598",
  "refId": "my-data-ref-001"
}
```

Bulk recipient payload is also supported:

```json
{
  "accountId": "00000000-0000-0000-0000-000000000000",
  "network": "mtn",
  "plan": "PSPLAN_177",
  "recipient": ["08169254598", "07038067493"],
  "refId": "my-data-bulk-ref-001"
}
```

## Electricity vending flow

Safe flow:

```text
validate meter/customer
quote electricity charge
vend electricity
store token/receipt response
requery pending transaction
reverse Credits if provider rejects immediately
```

### Electricity validation

```json
{
  "disco": "ikedc",
  "meterNumber": "54150143102",
  "meterType": "prepaid",
  "amount": 1000,
  "customerPhone": "07038067493"
}
```

Expected Payscribe validation details can include:

```text
customer_name
address
outstanding_balance
account_number
minimum_amount
debt_amount
minimum_debt
```

### Electricity quote

```json
{
  "service": "ikedc",
  "meterNumber": "54150143102",
  "meterType": "prepaid",
  "amount": 1000
}
```

Expected backend response:

```text
accepted = true
chargeCredits
validation.providerResponse
pricingPolicy
```

No Credits are debited during quote.

### Electricity vend

Use the `customerName` from validation.

```json
{
  "accountId": "00000000-0000-0000-0000-000000000000",
  "meterNumber": "54150143102",
  "meterType": "prepaid",
  "amount": 1000,
  "service": "ikedc",
  "phone": "07038067493",
  "customerName": "FEMI AGBEBUNMI",
  "address": "26, DAISI OKEOWO AGBALA ,IKORODU",
  "refId": "my-electricity-ref-001"
}
```

Expected Payscribe vend details can include:

```text
trans_id
token
token_amount
unit
reset_token
configure_token
tax_amount
tariff
meter_number
meter_type
service
customer_name
created_at
```

## Requery policy

Payscribe says pending transactions may occur and should be requeried after a few minutes. The same requery endpoint accepts either Payscribe `trans_id` or our own `refId`.

```http
GET /payscribe/requery?trans_id=my-electricity-ref-001
```

Do not requery more than once per minute.

## Ledger expectations

### Data

After data vend:

```text
credit_ledger.source = payscribe_data
credit_ledger.source_reference = refId
payscribe_transactions.service_type = data
payscribe_transactions.provider_reference = refId
payscribe_transactions.charge_credits is populated
```

Immediate provider rejection should create:

```text
credit_ledger.source = payscribe_data_reversal
```

### Electricity

After electricity vend:

```text
credit_ledger.source = payscribe_electricity
credit_ledger.source_reference = refId
payscribe_transactions.service_type = electricity
payscribe_transactions.provider_reference = refId
payscribe_transactions.provider_payload contains token/receipt details where applicable
```

Immediate provider rejection should create:

```text
credit_ledger.source = payscribe_electricity_reversal
```

## Completion criteria

Payscribe backend can be considered production-ready after these pass with real sandbox/live responses:

- data lookup returns live plans
- data quote extracts correct amount from live plan payload
- data vend succeeds and stores transaction
- data vend rejects invalid plan without debiting permanently
- data vend immediate failure creates reversal
- electricity validation returns customer details
- electricity quote validates and calculates Credits correctly
- electricity vend succeeds and stores token/receipt payload
- pending electricity/data transaction can be requeried
- duplicate ref does not double-charge unexpectedly
- invalid or unsupported disco/network is rejected before provider call

## Current implementation note

The backend currently calculates utility charges using env policy:

```text
plan/amount in NGN × credits_per_naira + service_fee_credits
```

This is safe for MVP. Later this should be moved into admin pricing policy so admins can update utility pricing without redeploying.

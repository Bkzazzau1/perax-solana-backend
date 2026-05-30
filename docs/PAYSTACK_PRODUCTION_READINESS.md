# Paystack Production Readiness Runbook

Paystack is used in Pera-X for Nigerian deposits only:

- card deposits
- dedicated virtual account deposits

Paystack must not credit users directly. It must verify the payment first, then pass through the existing provider verification flow so these ledgers are posted:

- `credit_ledger`
- `revenue_ledger`
- `burn_liabilities`
- `payment_confirmations`

## Required env

```env
PAYSTACK_BASE_URL=https://api.paystack.co
PAYSTACK_SECRET_KEY=replace-with-real-paystack-secret-key
PAYSTACK_PUBLIC_KEY=replace-with-real-paystack-public-key
PAYSTACK_NGN_PER_USD=1500
PERAX_INTERNAL_API_BASE_URL=http://127.0.0.1:8080
```

`PAYSTACK_NGN_PER_USD` is the backend conversion rate used to convert the USD-denominated Credit quote into NGN kobo for Paystack. This should later be moved to admin pricing/FX policy.

## Card deposit test

### 1. Create quote

```http
POST /credits/quote
{
  "method": "card",
  "creditAmount": 1000
}
```

Save:

```text
quoteReference
```

### 2. Create payment intent

```http
POST /credits/buy
{
  "method": "card",
  "creditAmount": 1000,
  "quoteReference": "quote_xxx"
}
```

Save:

```text
intentReference
```

Credits must not be posted yet.

### 3. Initialize Paystack transaction

```http
POST /payments/paystack/initialize
{
  "intentReference": "pi_xxx",
  "email": "user@example.com",
  "channels": ["card"],
  "callbackUrl": "https://app.perax.xyz/payment/callback"
}
```

Expected:

```text
authorizationUrl
accessCode
providerReference
```

### 4. Pay on Paystack checkout

Use Paystack checkout URL from `authorizationUrl`.

### 5. Verify manually

```http
POST /payments/paystack/verify
{
  "intentReference": "pi_xxx",
  "reference": "psk_xxx"
}
```

Expected:

- Paystack transaction status is `success`
- currency is `NGN`
- amount in kobo is equal to or greater than expected kobo
- existing provider verification posts Credits once

### 6. Confirm ledgers

Confirm:

- `payment_intents.status = credited`
- `payment_confirmations.provider = paystack`
- `credit_ledger.source = payment`
- `revenue_ledger.funding_method = card`
- `burn_liabilities.status = pending_pex_funding`

## Paystack webhook test

Paystack webhook route:

```http
POST /payments/paystack/webhook
```

The backend verifies:

```text
x-paystack-signature = HMAC_SHA512(raw_body, PAYSTACK_SECRET_KEY)
```

Only this event should credit:

```text
charge.success
```

Test duplicate webhook delivery. Expected:

- first delivery credits the user
- duplicate delivery does not double-credit

## Dedicated virtual account test

### 1. Assign virtual account

```http
POST /payments/paystack/virtual-account/assign
{
  "accountId": "00000000-0000-0000-0000-000000000000",
  "email": "user@example.com",
  "firstName": "Ibrahim",
  "lastName": "Bashir",
  "phone": "+2347000000000",
  "preferredBank": "wema-bank"
}
```

Expected saved fields:

- `account_number`
- `account_name`
- `bank_name`
- `bank_slug`
- `customer_code`
- `currency = NGN`
- `provider_status`
- `provider_payload`

### 2. Fetch user virtual account

```http
GET /payments/paystack/virtual-account/mine?accountId=00000000-0000-0000-0000-000000000000
```

### 3. Fund the virtual account

Transfer NGN into the Paystack dedicated account.

Expected:

- Paystack sends `charge.success`
- backend verifies webhook signature
- backend verifies transaction reference with Paystack
- backend posts Credits once using provider verification

## Current implementation notes

The first Paystack module uses `PERAX_INTERNAL_API_BASE_URL` to call the existing provider verification route internally after Paystack verification succeeds.

This keeps the Credit posting logic centralized and prevents Paystack from bypassing:

- double-credit protection
- `credit_ledger`
- `revenue_ledger`
- `burn_liabilities`

## Completion checklist

Paystack backend is complete when all these pass:

- card initialize returns Paystack authorization URL
- successful card verify posts Credits once
- failed card transaction does not post Credits
- duplicate verify/webhook does not double-credit
- webhook signature verification rejects invalid signatures
- dedicated virtual account assignment stores account details
- virtual account transfer webhook posts Credits once
- `provider_transactions` logs Paystack initialize/verify attempts
- revenue and burn liability are created for every successful Paystack payment

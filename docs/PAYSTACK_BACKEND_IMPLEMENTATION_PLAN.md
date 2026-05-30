# Paystack Backend Implementation Plan

This is the approved Paystack scope for Pera-X backend.

## Provider policy

Paystack will be used for:

1. Nigerian card deposits into Credits.
2. Nigerian dedicated virtual account deposits into Credits.

Payscribe will be handled later for:

1. Electricity bills.
2. Data bundles.

## Required backend rule

Paystack must never credit users directly.

The flow must remain:

```text
/credits/quote
/credits/buy -> creates payment_intent only
Paystack payment confirmation
backend verifies Paystack reference/webhook
backend posts through existing provider verification
credit_ledger + revenue_ledger + burn_liabilities
```

## Required env

```env
PAYSTACK_BASE_URL=https://api.paystack.co
PAYSTACK_SECRET_KEY=replace-with-paystack-secret-key
PAYSTACK_PUBLIC_KEY=replace-with-paystack-public-key
PAYSTACK_NGN_PER_USD=1500
```

## Required routes

```text
POST /payments/paystack/initialize
POST /payments/paystack/verify
POST /payments/paystack/webhook
POST /payments/paystack/virtual-account/assign
GET  /payments/paystack/virtual-account/mine
POST /payments/paystack/virtual-account/requery
```

## Transaction initialize

Input:

```json
{
  "intentReference": "pi_xxx",
  "email": "user@example.com",
  "callbackUrl": "https://app.perax.xyz/payment/callback",
  "channels": ["card"]
}
```

Backend should call Paystack transaction initialize using:

```text
POST /transaction/initialize
```

Amount must be sent in kobo.

## Transaction verify

Backend should verify with:

```text
GET /transaction/verify/{reference}
```

Verification must check:

- Paystack status is true.
- Transaction status is success.
- Currency is NGN.
- Amount paid is equal to or greater than expected quote amount converted to NGN kobo.
- Provider reference belongs to a pending payment_intent.
- Payment has not already been credited.

After this, backend should call existing provider verification logic with:

```text
provider = paystack
providerReference = Paystack reference
amountPaid = payment_intent.expected_amount
currency = payment_intent.expected_asset_code
status = successful
```

## Webhook

Paystack webhook must verify:

```text
x-paystack-signature = HMAC SHA512(raw_body, PAYSTACK_SECRET_KEY)
```

Only `charge.success` should trigger credit posting.

## Dedicated virtual account

Virtual account is only for Nigerian users.

Backend should store:

- account_id
- email
- customer_code
- account_name
- account_number
- bank_name
- bank_slug
- currency = NGN
- provider_status
- provider_payload

## Completion criteria

Paystack is complete when these pass:

1. Card initialize returns authorization URL.
2. Card verify credits user once.
3. Duplicate webhook does not double credit.
4. Failed transaction does not credit.
5. Virtual account assign returns account number and bank.
6. Virtual account payment webhook credits user once.
7. Paystack provider transactions are logged.
8. Revenue ledger and burn liability are created for every verified Paystack deposit.

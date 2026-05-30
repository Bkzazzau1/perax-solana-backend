# Paystack Smoke Test Commands

Run these after:

```powershell
git pull origin main
cargo check
cargo run
```

Required local env:

```env
PAYSTACK_BASE_URL=https://api.paystack.co
PAYSTACK_SECRET_KEY=your_real_key
PAYSTACK_NGN_PER_USD=1500
PERAX_INTERNAL_API_BASE_URL=http://127.0.0.1:8080
```

## 1. Create card quote

```powershell
curl -X POST http://127.0.0.1:8080/credits/quote `
  -H "Content-Type: application/json" `
  -d '{"method":"card","creditAmount":1000}'
```

Save `quoteReference`.

## 2. Create payment intent

```powershell
curl -X POST http://127.0.0.1:8080/credits/buy `
  -H "Content-Type: application/json" `
  -d '{"method":"card","creditAmount":1000,"quoteReference":"quote_xxx"}'
```

Save `intentReference` from `paymentIntent.intentReference`.

Credits must not be posted yet.

## 3. Initialize Paystack card checkout

```powershell
curl -X POST http://127.0.0.1:8080/payments/paystack/initialize `
  -H "Content-Type: application/json" `
  -d '{"intentReference":"pi_xxx","email":"user@example.com","channels":["card"],"callbackUrl":"https://app.perax.xyz/payment/callback"}'
```

Save:

```text
authorizationUrl
providerReference
```

Open `authorizationUrl` and complete Paystack payment.

## 4. Verify Paystack reference

```powershell
curl -X POST http://127.0.0.1:8080/payments/paystack/verify `
  -H "Content-Type: application/json" `
  -d '{"intentReference":"pi_xxx","reference":"psk_xxx"}'
```

Expected:

```text
paystackVerified = true
providerVerification.accepted = true
```

## 5. Virtual account quote

```powershell
curl -X POST http://127.0.0.1:8080/credits/quote `
  -H "Content-Type: application/json" `
  -d '{"method":"virtual_account","creditAmount":1000}'
```

## 6. Virtual account payment intent

```powershell
curl -X POST http://127.0.0.1:8080/credits/buy `
  -H "Content-Type: application/json" `
  -d '{"method":"virtual_account","creditAmount":1000,"quoteReference":"quote_xxx"}'
```

## 7. Initialize Paystack bank transfer checkout

```powershell
curl -X POST http://127.0.0.1:8080/payments/paystack/initialize `
  -H "Content-Type: application/json" `
  -d '{"intentReference":"pi_xxx","email":"user@example.com","channels":["bank_transfer"]}'
```

## 8. Assign dedicated virtual account

```powershell
curl -X POST http://127.0.0.1:8080/payments/paystack/virtual-account/assign `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","email":"user@example.com","firstName":"Ibrahim","lastName":"Bashir","phone":"+2347000000000","preferredBank":"wema-bank"}'
```

## 9. Fetch saved virtual account

```powershell
curl "http://127.0.0.1:8080/payments/paystack/virtual-account/mine?accountId=00000000-0000-0000-0000-000000000000"
```

## Ledger checks

After a successful verification, confirm these exist:

```text
payment_intents.status = credited
payment_confirmations.provider = paystack
credit_ledger.source = payment
revenue_ledger.funding_method = card or virtual_account
burn_liabilities.status = pending_pex_funding
```

## Duplicate protection check

Run the same `/payments/paystack/verify` request twice.

Expected second result:

```text
payment intent already verified or credited
```

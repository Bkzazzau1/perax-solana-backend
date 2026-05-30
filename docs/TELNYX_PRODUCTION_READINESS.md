# Telnyx Production Readiness Runbook

This document captures the remaining live validation steps for the Pera-X Telnyx backend.

The backend already has the core Telnyx architecture in place:

- voice call creation and call-control actions
- Telnyx voice webhooks
- SMS sending and inbound SMS webhooks
- number search, buy, reserve, cancel, reactivate, and sync
- credit-ledger billing and reversals
- provider transaction logging
- estimated provider cost and margin tracking
- CDR/MDR usage report sync

## Required environment variables

```env
TELNYX_BASE_URL=https://api.telnyx.com
TELNYX_API_KEY=replace-with-real-key
TELNYX_MESSAGING_PROFILE_ID=replace-with-profile-id
TELNYX_CONNECTION_ID=replace-with-call-control-connection-id
TELNYX_FROM_NUMBER=+10000000000

# Production must set at least one of these.
TELNYX_WEBHOOK_PUBLIC_KEY=replace-with-telnyx-public-key
TELNYX_WEBHOOK_SIGNING_SECRET=replace-with-hmac-secret-if-used

# Cost/profit estimates until usage reports provide final actual cost.
CREDIT_USD_VALUE=1
TELNYX_SMS_ESTIMATED_USD_COST_PER_SEGMENT=0
TELNYX_VOICE_ESTIMATED_USD_COST_PER_MINUTE=0
TELNYX_NUMBER_ESTIMATED_USD_COST=0

# Optional automatic Telnyx report reconciliation.
TELNYX_USAGE_REPORT_SYNC_ENABLED=false
TELNYX_USAGE_REPORT_SYNC_INTERVAL_SECONDS=3600
TELNYX_USAGE_REPORT_SYNC_WINDOW_HOURS=24
```

## Production webhook rule

When `APP_ENV=production`, `RUST_ENV=production`, or `ENV=production`, Telnyx webhooks must be signed.

The backend accepts either:

- Ed25519 verification using `TELNYX_WEBHOOK_PUBLIC_KEY`
- HMAC verification using `TELNYX_WEBHOOK_SIGNING_SECRET`

If neither is configured in production, webhook processing should fail.

## Live SMS test

1. Confirm provider status:

```bash
curl http://127.0.0.1:8080/admin/api/providers/status
```

2. Send a real SMS using an authenticated account with enough Credits:

```http
POST /telecom/sms
{
  "to": "+234...",
  "from": "+1...",
  "body": "Pera-X Telnyx test"
}
```

3. Confirm:

- `credit_ledger` has one posted debit with `source = telnyx_sms`
- `provider_transactions` has `provider = telnyx`, `provider_action = send_sms`
- if Telnyx rejects the SMS, a reversal appears with `source = telnyx_sms_reversal`
- inbound webhook stores received messages in `inbound_sms_messages`

## Live voice test

1. Start call/WebRTC offer:

```http
POST /telecom/webrtc/offer
{
  "sdp": "...",
  "destination_number": "+234..."
}
```

2. Confirm Telnyx webhooks arrive:

- `call.initiated`
- `call.answered`
- `call.hangup`

3. Confirm final billing:

- `telnyx_voice_calls.billing_status = posted`
- `billed_seconds` is populated from provider event timing
- `billed_minutes` is calculated correctly
- `credits_charged` matches backend pricing
- `credit_ledger` has one posted debit with `source = telnyx_voice_call`
- repeated webhook delivery does not double-bill the call

## Number lifecycle test

1. Search available numbers:

```http
GET /telecom/numbers/search?country_code=US&limit=5
```

2. Buy/order number:

```http
POST /telecom/numbers/buy
{
  "phone_number": "+1..."
}
```

3. Sync provider status:

```http
POST /telecom/numbers/{id}/sync
```

Confirm local fields are populated:

- `telnyx_phone_number_id`
- `provider_status`
- `messaging_profile_id`
- `messaging_product`
- `provider_payload`
- `last_provider_sync_at`

4. Cancel/release number:

```http
POST /telecom/numbers/{id}/cancel
```

Confirm:

- Telnyx messaging profile is unassigned first
- Telnyx number release succeeds
- local `billing_status = cancelled`
- local `provider_status = released`
- `next_renewal_at = null`

5. Reactivate number:

```http
POST /telecom/numbers/{id}/reactivate
```

Confirm:

- Credits are debited first
- if Telnyx rejects the order, Credits are reversed
- if Telnyx accepts, provider fields are updated
- messaging profile is assigned when configured

## Usage report reconciliation test

Manual sync endpoints:

```http
POST /telecom/usage-reports/cdr/sync
POST /telecom/usage-reports/mdr/sync
```

Confirm:

- `telnyx_usage_report_syncs` records the sync result
- CDR records reconcile voice-call cost fields
- MDR records are stored/logged for SMS cost reconciliation
- `provider_cost_source` changes from estimate to usage-report source where applicable

## Backend completion criteria

Telnyx can be considered production-ready after these pass with real Telnyx responses:

- real outbound SMS sends successfully
- real inbound SMS webhook is accepted and stored
- real outbound call completes and bills once
- repeated call hangup webhook does not double bill
- number buy/order works
- number sync updates provider fields
- number cancel releases at Telnyx
- number reactivation either succeeds or reverses Credits safely
- CDR/MDR sync can run without crashing on Telnyx payload shape
- production webhook verification rejects unsigned requests

## Current status

The GitHub/backend work is strong enough for MVP live testing. The remaining work is live Telnyx validation and provider-response adjustment if Telnyx returns a different payload shape or requires a different identifier format for some endpoints.

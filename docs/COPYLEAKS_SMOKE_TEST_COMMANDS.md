# Copyleaks Smoke Test Commands

Copyleaks is the premium plagiarism / historical-alignment layer.

Current policy:

- Humanizer stays local.
- Basic AI detector stays local.
- Basic plagiarism risk checker stays local.
- Copyleaks is used only for premium deep scans.

## Run backend

```powershell
git pull origin main
cargo check
cargo run
```

## Required env

```env
COPYLEAKS_BASE_URL=https://api.copyleaks.com
COPYLEAKS_EMAIL=replace-with-copyleaks-email
COPYLEAKS_API_KEY=replace-with-copyleaks-api-key
COPYLEAKS_WEBHOOK_SECRET=replace-with-copyleaks-webhook-secret
COPYLEAKS_WEBHOOK_URL=https://your-public-domain.com/ai/copyleaks/webhook
```

For local webhook testing, expose your local backend with a tunnel and set `COPYLEAKS_WEBHOOK_URL` to the public URL.

## 1. Check Copyleaks status

```powershell
curl http://127.0.0.1:8080/ai/copyleaks/status
```

Expected:

```text
configured = true when COPYLEAKS_EMAIL and COPYLEAKS_API_KEY are set
```

## 2. Quote premium scan

```powershell
curl -X POST http://127.0.0.1:8080/ai/copyleaks/quote `
  -H "Content-Type: application/json" `
  -d '{"scanType":"plagiarism"}'
```

Supported scan types:

```text
plagiarism
historical_alignment
ai_detection
```

Expected:

```text
serviceCode = copyleaks_premium_scan
creditCost = backend configured price
```

No Credits are debited here.

## 3. Submit premium scan

Use a real `accountId` that has enough posted Credits.

```powershell
curl -X POST http://127.0.0.1:8080/ai/copyleaks/submit `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","title":"test-scan.txt","scanType":"plagiarism","text":"This is a long enough test document for Copyleaks premium plagiarism scanning. It must contain at least fifty characters so the backend accepts it for a premium scan.","refId":"copyleaks-test-001"}'
```

Expected:

```text
scanReference = copyleaks-test-001
copyleaksScanId = perax-copyleaks-test-001
creditCost populated
status = submitted if live credentials/payload worked
status = created if stored but live submission failed/deferred
```

Ledger check:

```text
credit_ledger.source = copyleaks_premium_scan
credit_ledger.source_reference = copyleaks-test-001
```

## 4. Get result by reference

```powershell
curl http://127.0.0.1:8080/ai/copyleaks/result/copyleaks-test-001
```

Or by Copyleaks scan ID:

```powershell
curl http://127.0.0.1:8080/ai/copyleaks/result/perax-copyleaks-test-001
```

## 5. Simulate webhook locally

If `COPYLEAKS_WEBHOOK_SECRET` is set, include it in `x-perax-webhook-secret`.

```powershell
curl -X POST http://127.0.0.1:8080/ai/copyleaks/webhook `
  -H "Content-Type: application/json" `
  -H "x-perax-webhook-secret: replace-with-copyleaks-webhook-secret" `
  -d '{"scanId":"perax-copyleaks-test-001","status":"completed","results":{"score":12,"matches":[]}}'
```

Expected:

```text
accepted = true
matchedRows >= 1
```

## 6. Re-check result

```powershell
curl http://127.0.0.1:8080/ai/copyleaks/result/copyleaks-test-001
```

Expected:

```text
status = completed
resultPayload contains webhook payload
```

## Important notes

The current implementation already:

- creates `copyleaks_scans`
- charges Credits before scan submission
- stores scan reference and Copyleaks scan ID
- accepts webhooks
- returns scan result/status

Live submission may still need adjustment after testing with real Copyleaks credentials because provider payload and auth response can differ by account/API version.

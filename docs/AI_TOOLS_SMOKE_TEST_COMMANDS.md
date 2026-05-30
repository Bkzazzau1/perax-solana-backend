# AI Tools Smoke Test Commands

This covers the current MVP backend AI tools:

- Copy AI
- Humanizer
- AI Detector
- Plagiarism Checker

Current AI engines are MVP backend engines. Live model/provider integration will come later.

## Run backend

```powershell
git pull origin main
cargo check
cargo run
```

Use a real `accountId` that already has enough posted Credits in `credit_ledger`.

## 1. Copy AI quote

```powershell
curl -X POST http://127.0.0.1:8080/ai/copy/quote `
  -H "Content-Type: application/json" `
  -d '{"copyKind":"ad_copy","variants":2}'
```

Expected:

```text
accepted = true
serviceCode = copy_ai_generate
unitCreditCost
totalCreditCost
```

No Credits are debited here.

## 2. Copy AI generate

```powershell
curl -X POST http://127.0.0.1:8080/ai/copy/generate `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","copyKind":"social_caption","businessName":"PeraX","productOrService":"cheap international calls and utility payments","targetAudience":"Africans who need cheaper digital services","keyPoints":["pay with Credits","fast service","simple app"],"tone":"professional and friendly","platform":"Instagram","callToAction":"Download PeraX today","variants":2,"refId":"copy-test-001"}'
```

Expected:

```text
accepted = true
reference = copy-test-001
outputs contains generated copy
creditCost is debited from credit_ledger
```

Ledger check:

```text
credit_ledger.source = copy_ai_generate
credit_ledger.source_reference = copy-test-001
```

## 3. AI Detector

```powershell
curl -X POST http://127.0.0.1:8080/ai/documents/analyze `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","tool":"ai_detector","text":"Furthermore, it is important to note that the digital landscape is rapidly evolving. In conclusion, this solution provides innovative opportunities for users.","inputMode":"text","refId":"ai-detector-test-001"}'
```

Expected:

```text
title = AI Detection Report
score
findings
engine = heuristic_mvp
reference = ai-detector-test-001
```

Ledger check:

```text
credit_ledger.source = ai_detector
credit_ledger.source_reference = ai-detector-test-001
```

## 4. Plagiarism Checker

```powershell
curl -X POST http://127.0.0.1:8080/ai/documents/analyze `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","tool":"plagiarism_checker","text":"This project provides digital payments. This project provides digital payments. This project provides digital payments.","inputMode":"text","refId":"plagiarism-test-001"}'
```

Expected:

```text
title = Plagiarism Check Report
score
findings
engine = heuristic_mvp
reference = plagiarism-test-001
```

Ledger check:

```text
credit_ledger.source = plagiarism_checker
credit_ledger.source_reference = plagiarism-test-001
```

## 5. Humanizer

```powershell
curl -X POST http://127.0.0.1:8080/ai/documents/analyze `
  -H "Content-Type: application/json" `
  -d '{"accountId":"00000000-0000-0000-0000-000000000000","tool":"humanizer","text":"Furthermore, it is important to note that our application provides a reliable and innovative solution. In conclusion, users will benefit from the platform.","inputMode":"text","refId":"humanizer-test-001"}'
```

Expected:

```text
title = Humanized Draft
output contains rewritten text
engine = rewrite_mvp
reference = humanizer-test-001
```

Ledger check:

```text
credit_ledger.source = humanizer
credit_ledger.source_reference = humanizer-test-001
```

## Supported Copy AI kinds

```text
ad_copy
social_caption
product_description
email_copy
sms_copy
landing_hero
business_bio
```

## Important rules

- `/ai/copy/quote` does not debit Credits.
- `/ai/copy/generate` debits Credits before generation.
- `/ai/documents/analyze` now debits Credits for Humanizer, AI Detector, and Plagiarism Checker.
- Current engines are MVP and deterministic.
- Production-grade quality requires connecting live AI/plagiarism providers later.

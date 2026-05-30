# Pera-X AI Provider Policy

This document defines the approved AI service strategy for Pera-X.

## Core decision

Pera-X will not send every AI request to an external provider.

The backend will use a two-layer model:

1. Local low-cost MVP tools.
2. Premium provider-backed scans only when required.

This protects cost, keeps the app fast, and allows us to launch earlier.

## Local AI tools

The following tools remain local for now:

```text
Humanizer
Basic AI detector
Basic plagiarism risk checker
Copywriting generator
```

### Humanizer

Humanizer uses local backend rewriting rules.

No Claude/OpenAI is required for MVP.

Reason:

```text
- low cost
- simple to maintain
- enough for early product testing
- avoids paying model fees for every rewrite
```

### Basic AI detector

Basic AI detector uses local heuristic scoring.

This is not a final academic detector. It is a quick user-facing risk screen.

### Basic plagiarism checker

Basic plagiarism checker uses local heuristic risk scoring.

This is not a final plagiarism report. It only checks early signs such as repetition, generic phrasing, and citation weakness.

## Premium provider layer

Copyleaks is the approved premium provider layer for:

```text
Plagiarism checking
Historical alignment
Optional AI-content report
```

Copyleaks should only be used when the user chooses a premium deep scan.

Backend route group:

```text
GET  /ai/copyleaks/status
POST /ai/copyleaks/quote
POST /ai/copyleaks/submit
POST /ai/copyleaks/webhook
GET  /ai/copyleaks/result/{reference}
```

## Billing policy

Local tools have lower Credit costs.

Premium Copyleaks scans have higher Credit costs because they use an external paid provider.

Current service codes:

```text
humanizer
ai_detector
plagiarism_checker
copy_ai_generate
copyleaks_premium_scan
```

## Recommended frontend positioning

Use these names in the app:

```text
Humanizer = Rewrite my text
AI Detector = Basic AI risk check
Plagiarism Checker = Basic similarity risk check
Premium Plagiarism Scan = Copyleaks deep scan
Copywriting Generator = Marketing copy tool
```

Avoid calling the copywriting generator `CopyAI` because in this project `CopyAI` refers to Copyleaks.

## Upgrade path

Stage 1:

```text
Local Humanizer
Local AI detector
Local plagiarism risk checker
Local copywriting generator
Copyleaks premium scan foundation
```

Stage 2:

```text
Improve local Humanizer with tone, strength, and rewrite modes
Improve local detector and checker outputs
Test Copyleaks live scan with real credentials
```

Stage 3:

```text
Connect live Copyleaks scan result parsing
Add admin controls for scan pricing
Add stronger result UI for sources, matches, and reports
```

## Permanent rule

Humanizer should remain local until product flow and pricing prove that a paid AI provider is necessary.

Copyleaks should be used only for premium deep scan services, not for every basic plagiarism request.

#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"

printf '\n== Pera-X Admin Burn Runtime Test ==\n'
printf 'Base URL: %s\n\n' "$BASE_URL"

printf '1) Health check\n'
curl -fsS "$BASE_URL/healthz"
printf '\n\n'

printf '2) Burn preview\n'
curl -fsS "$BASE_URL/admin/api/burn-preview"
printf '\n\n'

printf '3) Declare a safe test burn decision\n'
DECLARE_RESPONSE=$(curl -fsS -X POST "$BASE_URL/admin/api/burn-decisions/declare" \
  -H "Content-Type: application/json" \
  -d '{"trading_company_balance":100000,"utility_usage_score":0.2,"holder_pressure_score":0.9}')
printf '%s\n\n' "$DECLARE_RESPONSE"

DECISION_ID=$(python3 - <<'PY' "$DECLARE_RESPONSE"
import json
import sys
print(json.loads(sys.argv[1])["id"])
PY
)

printf '4) Approve the test burn decision: %s\n' "$DECISION_ID"
curl -fsS -X POST "$BASE_URL/admin/api/burn-decisions/status" \
  -H "Content-Type: application/json" \
  -d "{\"id\":\"$DECISION_ID\",\"status\":\"approved\"}"
printf '\n\n'

printf '5) List recent burn decisions\n'
curl -fsS "$BASE_URL/admin/api/burn-decisions?limit=5"
printf '\n\n'

printf 'Runtime test completed successfully.\n'

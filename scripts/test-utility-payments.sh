#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
REFERENCE_HEX="${REFERENCE_HEX:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}"

printf '\n== Pera-X Utility Payment Runtime Test ==\n'
printf 'Base URL: %s\n' "$BASE_URL"
printf 'Reference: %s\n\n' "$REFERENCE_HEX"

printf '1) Health check\n'
curl -fsS "$BASE_URL/healthz"
printf '\n\n'

printf '2) Trading Company SPL token account status\n'
curl -fsS "$BASE_URL/admin/api/trading-company-status"
printf '\n\n'

printf '3) Ingest a safe test utility payment\n'
INGEST_RESPONSE=$(curl -fsS -X POST "$BASE_URL/admin/api/utility-payments/ingest" \
  -H "Content-Type: application/json" \
  -d "{\"reference_hex\":\"$REFERENCE_HEX\",\"payer_wallet\":\"test-payer-wallet\",\"token_mint\":\"test-token-mint\",\"amount\":2500,\"service_code\":\"TEST_UTILITY\",\"tx_signature\":\"test-signature-$REFERENCE_HEX\"}")
printf '%s\n\n' "$INGEST_RESPONSE"

printf '4) Grant the confirmed utility payment\n'
curl -fsS -X POST "$BASE_URL/admin/api/utility-payments/grant" \
  -H "Content-Type: application/json" \
  -d "{\"reference_hex\":\"$REFERENCE_HEX\"}"
printf '\n\n'

printf '5) List recent granted utility payments\n'
curl -fsS "$BASE_URL/admin/api/utility-payments?status=granted&limit=5"
printf '\n\n'

printf 'Utility payment runtime test completed successfully.\n'

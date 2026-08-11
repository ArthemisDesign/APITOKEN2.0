#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

cat >"$TEMP/engine-accounts" <<'EOF'
acct-a	5000	active
acct-b	9000	disabled
engine-only	10000	active
EOF
cat >"$TEMP/commerce-accounts" <<'EOF'
acct-a	5000	active
acct-b	8000	active
commerce-only	4000	pending
EOF
cat >"$TEMP/commerce-overrides" <<'EOF'
acct-a	openai	6000
acct-b	google	7000
acct-b	openai	5000
commerce-only	glm	9000
EOF
cat >"$TEMP/engine-overrides" <<'EOF'
acct-a	google	10000
acct-a	openai	6000
acct-b	google	8000
acct-b	openai	6000
engine-only	openai	10000
EOF

output=$(awk \
  -v engine_accounts="$TEMP/engine-accounts" \
  -v commerce_accounts="$TEMP/commerce-accounts" \
  -v commerce_overrides="$TEMP/commerce-overrides" \
  -v engine_overrides="$TEMP/engine-overrides" \
  -f "$ROOT/deploy/monitoring-authority-drift.awk" \
  "$TEMP/engine-accounts" "$TEMP/commerce-accounts" \
  "$TEMP/commerce-overrides" "$TEMP/engine-overrides")

grep -Fxq 'apitoken_pricing_authority_drift{dimension="default"} 2' <<<"$output"
grep -Fxq 'apitoken_pricing_authority_drift{dimension="status"} 2' <<<"$output"
grep -Fxq 'apitoken_business_reconciliation_up{scope="pricing_authority"} 1' <<<"$output"
! grep -Fq 'acct-' <<<"$output"
! grep -Fq 'commerce-only' <<<"$output"
! grep -Fq 'engine-only' <<<"$output"

# Multiple mismatching provider rows on one account are one affected-account drift, not a
# cardinality-dependent total of override rows.
grep -Fxq 'apitoken_pricing_authority_drift{dimension="provider"} 3' <<<"$output"

# Empty override snapshots are valid and must remain zero rather than shifting the file roles.
: >"$TEMP/commerce-overrides"
: >"$TEMP/engine-overrides"
output=$(awk \
  -v engine_accounts="$TEMP/engine-accounts" \
  -v commerce_accounts="$TEMP/engine-accounts" \
  -v commerce_overrides="$TEMP/commerce-overrides" \
  -v engine_overrides="$TEMP/engine-overrides" \
  -f "$ROOT/deploy/monitoring-authority-drift.awk" \
  "$TEMP/engine-accounts" "$TEMP/engine-accounts" \
  "$TEMP/commerce-overrides" "$TEMP/engine-overrides")
grep -Fxq 'apitoken_pricing_authority_drift{dimension="default"} 0' <<<"$output"
grep -Fxq 'apitoken_pricing_authority_drift{dimension="provider"} 0' <<<"$output"
grep -Fxq 'apitoken_pricing_authority_drift{dimension="status"} 0' <<<"$output"

printf 'monitoring authority drift tests passed\n'

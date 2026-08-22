#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
SCRIPT="$ROOT/tools/refresh-fingerprint.sh"

extract_cc_version() {
  printf '%s' "$1" \
    | grep -oE 'cc_version=[^;]+' \
    | head -1 \
    | sed -E 's/^cc_version=//'
}

for version in 2.1.195.d49 2.1.220.a6e 2.1.231.408 2.1.239.0f1; do
  body="{\"system\":[{\"type\":\"text\",\"text\":\"x-anthropic-billing-header: cc_version=$version; cc_entrypoint=sdk-cli;\"}]}"
  actual=$(extract_cc_version "$body")
  [[ $actual == "$version" ]] || {
    printf 'full cc_version changed: expected=%s actual=%s\n' "$version" "$actual" >&2
    exit 1
  }
  [[ $actual != "$version.d42" ]] || {
    printf 'cc_version received a synthetic suffix: %s\n' "$actual" >&2
    exit 1
  }
done

# The production parser must use the same exact extraction and must never strip or generate one
# particular build-suffix shape. This textual guard makes script/code drift fail in the Rust lane.
grep -Fq "grep -oE 'cc_version=[^;]+'" "$SCRIPT"
grep -Fq 'mv "$NEXT" "$CONFIG_ENV"' "$SCRIPT"
grep -Fq 'set_kv CLAUDE_API_CC_VERSION "$CCVER"' "$SCRIPT"
! grep -Eq 's/\\\.d\[0-9\].*//|persona_ccbuild|cc_version=\{\}\.\{\}|systemctl restart' "$SCRIPT"

echo 'refresh fingerprint full-version contract passed'

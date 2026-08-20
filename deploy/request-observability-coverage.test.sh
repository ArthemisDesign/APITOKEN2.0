#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/crates/registry/src/request_facts.rs"
SERVER="$ROOT/crates/server/src/http.rs"
ROUTER="$ROOT/crates/router/src/main.rs"
FORWARD="$ROOT/crates/forward/src"

fail() { printf 'request observability coverage: %s\n' "$*" >&2; exit 1; }

expected=(
  'anthropic|native|count_tokens'
  'anthropic|native|messages'
  'anthropic|universal|chat'
  'anthropic|universal|responses'
  'openai|native|input_tokens'
  'openai|native|chat'
  'openai|native|responses'
  'openai|universal|count_tokens'
  'openai|universal|messages'
  'gemini|native|count_tokens'
  'gemini|native|generate'
  'gemini|native|stream_generate'
  'gemini|universal|chat'
  'gemini|universal|responses'
  'gemini|universal|messages'
)

actual=$(awk '
  /pub const REQUEST_FACT_V1_SCOPES/ { in_manifest=1; next }
  in_manifest && /^];/ { exit }
  in_manifest && /provider_plane:/ { gsub(/[\",]/, "", $2); provider=$2 }
  in_manifest && /route_class:/ { gsub(/[\",]/, "", $2); route=$2 }
  in_manifest && /request_class:/ { gsub(/[\",]/, "", $2); print provider "|" route "|" $2 }
' "$MANIFEST")
[[ $(printf '%s\n' "$actual" | sed '/^$/d' | wc -l | tr -d ' ') -eq 15 ]] \
  || fail 'manifest contains a route outside the 15 locked v1 scopes'
for scope in "${expected[@]}"; do
  [[ $(grep -Fxc "$scope" <<<"$actual") -eq 1 ]] || fail "manifest does not own exactly one $scope entry"
done

for route in '/v1/messages' '/v1/chat/completions' '/v1/responses' '/v1/responses/input_tokens'; do
  grep -Fq ".route(\"$route\"" "$SERVER" || fail "server dispatch misses $route"
  grep -Fq ".route(\"$route\"" "$ROUTER" || fail "router dispatch misses $route"
done
for route in '/v1/messages/count_tokens'; do
  grep -Fq "\"$route\"" "$SERVER" || fail "server dispatch misses $route"
  grep -Fq ".route(\"$route\"" "$ROUTER" || fail "router dispatch misses $route"
done
for producer in \
  'CodexBillableRequestSpec::native_chat' \
  'CodexBillableRequestSpec::native_responses' \
  'CodexBillableRequestSpec::universal_messages' \
  'SynthesizedMessagesOrigin' \
  'GeminiBillableRequestSpec::universal_chat' \
  'GeminiBillableRequestSpec::universal_responses' \
  'GeminiBillableRequestSpec::universal_messages' \
  'GeminiBillableRequestSpec::native'; do
  grep -R -Fq "$producer" "$FORWARD" || fail "producer marker missing: $producer"
done

for forbidden in \
  'provider_plane: "combined"' \
  'request_class: "images"' \
  'request_class: "batch"' \
  'request_class: "embeddings"' \
  'request_class: "files"'; do
  ! grep -Fq "$forbidden" "$MANIFEST" || fail "excluded surface entered manifest: $forbidden"
done

printf 'request observability coverage: 15 scoped leaves and exclusions pinned\n'

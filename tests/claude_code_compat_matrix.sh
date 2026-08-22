#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/claude-code-compat-manifest.json"
CACHE_ROOT=${CLAUDE_CODE_COMPAT_CACHE_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/apitoken/claude-code-compat}
CHANNELS=${CLAUDE_CODE_COMPAT_CHANNELS:-stable,latest}
PLACEHOLDER_KEY=claude-code-compat-placeholder
CASE_TIMEOUT_SECONDS=${CLAUDE_CODE_COMPAT_TIMEOUT_SECONDS:-120}
RUNTIME_BASE_URL=${CLAUDE_CODE_COMPAT_RUNTIME_BASE_URL:-}
RUNTIME_API_KEY=${CLAUDE_CODE_COMPAT_RUNTIME_API_KEY:-}
ACTIVE_SERVER_PID=
ACTIVE_TEMP=

cleanup_active() {
  if [[ -n ${ACTIVE_SERVER_PID:-} ]]; then
    kill "$ACTIVE_SERVER_PID" 2>/dev/null || true
    wait "$ACTIVE_SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n ${ACTIVE_TEMP:-} ]]; then
    rm -rf "$ACTIVE_TEMP"
  fi
  ACTIVE_SERVER_PID=
  ACTIVE_TEMP=
}
trap cleanup_active EXIT INT TERM HUP

for command_name in node npm python3 tar perl; do
  command -v "$command_name" >/dev/null 2>&1 || {
    printf 'required command is missing: %s\n' "$command_name" >&2
    exit 2
  }
done

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) platform=darwin-arm64 ;;
  Darwin-x86_64) platform=darwin-x64 ;;
  Linux-x86_64) platform=linux-x64 ;;
  Linux-aarch64|Linux-arm64) platform=linux-arm64 ;;
  *) printf 'unsupported exact-client platform: %s-%s\n' "$(uname -s)" "$(uname -m)" >&2; exit 2 ;;
esac

json_get() {
  node -e 'const d=require(process.argv[1]); let v=d; for (const p of process.argv.slice(2)) v=v[p]; if (typeof v!=="string") process.exit(2); process.stdout.write(v)' "$MANIFEST" "$@"
}

verify_integrity() {
  local tarball=$1 expected=$2 actual
  actual=$(node - "$tarball" <<'NODE'
const fs=require('fs'), crypto=require('crypto');
const data=fs.readFileSync(process.argv[2]);
process.stdout.write('sha512-'+crypto.createHash('sha512').update(data).digest('base64'));
NODE
)
  [[ $actual == "$expected" ]] || {
    printf 'Claude Code package integrity mismatch\nexpected: %s\nactual:   %s\n' "$expected" "$actual" >&2
    return 1
  }
}

resolve_binary() {
  local channel=$1 version package integrity dir tgz
  version=$(json_get channels "$channel")
  package=$(json_get platforms "$platform" package)
  integrity=$(json_get platforms "$platform" "$version")
  dir="$CACHE_ROOT/$platform/$version"
  tgz="$dir/package.tgz"
  mkdir -p "$dir"
  if [[ ! -s $tgz ]]; then
    local packed
    packed=$(npm pack "$package@$version" --pack-destination "$dir" --silent)
    mv -f "$dir/$packed" "$tgz"
  fi
  verify_integrity "$tgz" "$integrity"
  # Re-extract verified bytes every run. An extracted binary is never an authority of its own;
  # this also makes interrupted/concurrent extraction a cache miss instead of trusted state.
  local extract="$dir/package.$$.tmp"
  rm -rf "$extract"
  mkdir -p "$extract"
  tar -xzf "$tgz" -C "$extract"
  rm -rf "$dir/package"
  mv "$extract/package" "$dir/package"
  rmdir "$extract"
  chmod +x "$dir/package/claude"
  local reported
  reported=$("$dir/package/claude" --version)
  [[ $reported == "$version (Claude Code)" ]] || {
    printf 'Claude Code binary version mismatch: expected %s, got %s\n' "$version" "$reported" >&2
    return 1
  }
  printf '%s\t%s\n' "$version" "$dir/package/claude"
}

run_case() {
  local channel=$1 discovery=$2 resolved version binary temp server_pid port output_schema
  resolved=$(resolve_binary "$channel")
  version=${resolved%%$'\t'*}
  binary=${resolved#*$'\t'}
  temp=$(mktemp -d "${TMPDIR:-/tmp}/claude-code-compat.XXXXXX")
  ACTIVE_TEMP=$temp
  python3 "$ROOT/tests/claude_code_compat_mock.py" \
    --ready-file "$temp/ready" --evidence-file "$temp/evidence.jsonl" &
  server_pid=$!
  ACTIVE_SERVER_PID=$server_pid
  for _ in $(seq 1 100); do
    [[ -s $temp/ready ]] && break
    kill -0 "$server_pid" 2>/dev/null || break
    sleep 0.05
  done
  [[ -s $temp/ready ]] || { printf 'compat mock did not start\n' >&2; return 1; }
  port=$(cat "$temp/ready")
  mkdir -p "$temp/config"
  output_schema='{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}'
  local -a discovery_env=()
  if [[ $discovery == 1 ]]; then
    discovery_env=(CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1)
  else
    discovery_env=(CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1)
  fi
  perl -e 'alarm shift; exec @ARGV' "$CASE_TIMEOUT_SECONDS" \
    env -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_OAUTH_TOKEN \
      -u CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR -u CCR_OAUTH_TOKEN_FILE \
      -u ANTHROPIC_CUSTOM_HEADERS -u HTTPS_PROXY -u HTTP_PROXY -u ALL_PROXY \
      -u https_proxy -u http_proxy -u all_proxy \
      ANTHROPIC_API_KEY="$PLACEHOLDER_KEY" \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$port" \
      CLAUDE_CONFIG_DIR="$temp/config" \
      DISABLE_AUTOUPDATER=1 DISABLE_ERROR_REPORTING=1 DISABLE_TELEMETRY=1 \
      "${discovery_env[@]}" \
      "$binary" --tools '' --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
        --setting-sources '' --print --output-format json --no-session-persistence \
        --max-turns 1 --max-budget-usd 1 --effort low --model claude-sonnet-4-6 \
        --json-schema "$output_schema" 'Return only {"ok":true}. Do not call tools.' \
        >"$temp/result.json" 2>"$temp/stderr"
  python3 "$ROOT/tests/claude_code_compat_assert.py" \
    --evidence-file "$temp/evidence.jsonl" --version "$version" \
    $([[ $discovery == 1 ]] && printf '%s' '--require-discovery')
  if [[ $discovery == 0 && -n $RUNTIME_BASE_URL ]]; then
    [[ -n $RUNTIME_API_KEY ]] || {
      printf 'CLAUDE_CODE_COMPAT_RUNTIME_API_KEY is required with runtime replay\n' >&2
      return 2
    }
    python3 "$ROOT/tests/claude_code_runtime_replay.py" \
      --evidence-file "$temp/evidence.jsonl" \
      --base-url "$RUNTIME_BASE_URL" --api-key "$RUNTIME_API_KEY"
  fi
  cleanup_active
}

IFS=, read -r -a channels <<<"$CHANNELS"
for channel in "${channels[@]}"; do
  [[ $channel == stable || $channel == latest ]] || {
    printf 'unknown Claude Code compatibility channel: %s\n' "$channel" >&2
    exit 2
  }
  run_case "$channel" 0
  run_case "$channel" 1
done

echo 'Claude Code stable/latest exact-client matrix passed'

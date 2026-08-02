#!/usr/bin/env bash
# Run real installed agent harnesses through a credential-blind loopback proxy.
#
# Every client receives only PLACEHOLDER_API_KEY. The production key is removed from the
# caller environment before a client starts and is exported only to the evidence proxy,
# which removes it from its own environment and records protocol metadata rather than content.
set -euo pipefail
set +x

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROUTER_BASE_URL=${APITOKEN_ROUTER_BASE_URL:-https://router.apitoken.sale}
CASE_TIMEOUT_SECONDS=${APITOKEN_HARNESS_CASE_TIMEOUT_SECONDS:-240}
OPENAI_MODEL=${APITOKEN_HARNESS_OPENAI_MODEL:-openai/gpt-5.4}
GEMINI_MODEL=${APITOKEN_HARNESS_GEMINI_MODEL:-gemini-2.5-flash-lite}
OPENCODE_GEMINI_MODEL=${APITOKEN_HARNESS_OPENCODE_GEMINI_MODEL:-google/gemini-3.1-flash-lite}
OPENCODE_CLAUDE_MODEL=${APITOKEN_HARNESS_OPENCODE_CLAUDE_MODEL:-anthropic/claude-sonnet-4-6}
OPENCODE_CLAUDE_SMALL_MODEL=${APITOKEN_HARNESS_OPENCODE_CLAUDE_SMALL_MODEL:-anthropic/claude-haiku-4-5-20251001}
OPENCODE_CLAUDE_EFFORT_MODEL=${APITOKEN_HARNESS_OPENCODE_CLAUDE_EFFORT_MODEL:-anthropic/claude-opus-5}
PLACEHOLDER_API_KEY=router-harness-placeholder-key
PROMPT='Reply exactly OK. Do not call tools, inspect files, or change anything.'
GEMINI_TOOL_PROMPT="Use the bash tool to run exactly: printf 'OPENCODE_GEMINI_TOOL_OK\\n' > opencode-gemini-tool-proof.txt. Do not use another tool. After the tool succeeds, reply exactly OPENCODE_GEMINI_TOOL_OK."
CASE_FILTER=${APITOKEN_HARNESS_CASES:-}

: "${APITOKEN_API_KEY:?APITOKEN_API_KEY must already be set in the caller environment}"
[[ $CASE_TIMEOUT_SECONDS =~ ^[1-9][0-9]*$ ]] || {
  printf 'APITOKEN_HARNESS_CASE_TIMEOUT_SECONDS must be a positive integer\n' >&2
  exit 2
}
case $ROUTER_BASE_URL in
  http://*|https://*) ;;
  *)
    printf 'APITOKEN_ROUTER_BASE_URL must be an absolute HTTP(S) URL\n' >&2
    exit 2
    ;;
esac

ROUTER_API_KEY=$APITOKEN_API_KEY
unset APITOKEN_API_KEY
export -n ROUTER_API_KEY 2>/dev/null || true

TEMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/router-harness-live.XXXXXX")
PROXY_PID=
PROXY_BASE_URL=
EVIDENCE_FILE=
CASE_DIR=
PASSED_CASES=0

cleanup() {
  local status=$?
  trap - EXIT INT TERM HUP
  if [[ -n ${PROXY_PID:-} ]]; then
    kill "$PROXY_PID" 2>/dev/null || true
    wait "$PROXY_PID" 2>/dev/null || true
  fi
  ROUTER_API_KEY=
  unset ROUTER_API_KEY
  case ${TEMP_ROOT:-} in
    */router-harness-live.*)
      rm -rf -- "$TEMP_ROOT"
      ;;
    *)
      printf 'refusing to remove unexpected temporary path\n' >&2
      status=70
      ;;
  esac
  exit "$status"
}
trap cleanup EXIT INT TERM HUP

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'required harness command is not installed: %s\n' "$1" >&2
    exit 2
  }
}

for command_name in python3 perl cline cn opencode kilo codex claude gemini hermes aider jq; do
  require_command "$command_name"
done

run_quiet() {
  perl -e '
    my $seconds = shift @ARGV;
    $SIG{ALRM} = sub { exit 124 };
    alarm $seconds;
    exec @ARGV;
    exit 127;
  ' "$CASE_TIMEOUT_SECONDS" "$@" </dev/null >/dev/null 2>&1
}

start_proxy() {
  local label=$1
  local ready_file="$CASE_DIR/proxy.ready"
  EVIDENCE_FILE="$CASE_DIR/evidence.jsonl"
  (
    export APITOKEN_HARNESS_PROXY_KEY="$ROUTER_API_KEY"
    exec python3 "$HERE/router_harness_evidence_proxy.py" \
      --target-base-url "$ROUTER_BASE_URL" \
      --label "$label" \
      --ready-file "$ready_file" \
      --evidence-file "$EVIDENCE_FILE" \
      --api-key-env APITOKEN_HARNESS_PROXY_KEY
  ) </dev/null >/dev/null 2>&1 &
  PROXY_PID=$!

  local attempt
  for attempt in $(seq 1 200); do
    if [[ -s $ready_file ]]; then
      PROXY_BASE_URL="http://127.0.0.1:$(tr -d '\r\n' <"$ready_file")"
      return
    fi
    kill -0 "$PROXY_PID" 2>/dev/null || break
    sleep 0.05
  done
  printf '%s: evidence proxy did not become ready\n' "$label" >&2
  return 1
}

stop_proxy() {
  if [[ -n ${PROXY_PID:-} ]]; then
    kill "$PROXY_PID" 2>/dev/null || true
    wait "$PROXY_PID" 2>/dev/null || true
  fi
  PROXY_PID=
}

assert_evidence() {
  local label=$1
  local protocol=$2
  local path=$3
  local model=$4
  local tier=$5
  local selector=$6
  python3 - "$EVIDENCE_FILE" "$label" "$protocol" "$path" "$model" "$tier" "$selector" "$OPENCODE_CLAUDE_SMALL_MODEL" <<'PY'
import json
import sys
from pathlib import Path

evidence_path, label, protocol, path, model, tier, selector, claude_small_model = sys.argv[1:]
entries = []
for line in Path(evidence_path).read_text(encoding="utf-8").splitlines():
    if line:
        entries.append(json.loads(line))

observed = [
    entry
    for entry in entries
    if entry.get("method") == "POST"
    and entry.get("protocol") == protocol
    and entry.get("path") == path
]
if not observed:
    raise SystemExit(f"{label}: no executable {protocol} request was recorded")

allowed_statuses = {200, 400} if label.startswith("claude-code-") else {200}
unexpected_failures = [entry for entry in observed if entry.get("status") not in allowed_statuses]
if unexpected_failures:
    status = unexpected_failures[0].get("status")
    raise SystemExit(f"{label}: request returned unexpected HTTP {status}")

# Current Claude Code performs one fail-closed 400 capability probe containing a system role in
# messages before the real request. It never crossed the execution boundary. Accepted HTTP 200
# requests are the executable attempts whose tier and lifecycle must all be authoritative.
attempts = [entry for entry in observed if entry.get("status") == 200]
if not attempts:
    raise SystemExit(f"{label}: no successful executable {protocol} request was recorded")

for attempt in attempts:
    sequence = attempt.get("sequence", "?")
    if label == "opencode-claude-native":
        if attempt.get("model") not in {model, claude_small_model}:
            raise SystemExit(f"{label}: attempt {sequence} used unexpected model")
    elif attempt.get("model") != model:
        raise SystemExit(f"{label}: attempt {sequence} used unexpected model")
    request_header = attempt.get("request_fast_header")
    request_tiers = set(attempt.get("request_tiers") or [])
    response_tiers = set(attempt.get("response_service_tiers") or [])
    if tier == "standard":
        if request_header not in (None, ""):
            raise SystemExit(f"{label}: Standard attempt {sequence} sent a Fast header")
        if request_tiers.intersection({"fast", "priority"}):
            raise SystemExit(f"{label}: Standard attempt {sequence} sent a Fast body tier")
        if "priority" in response_tiers:
            raise SystemExit(f"{label}: Standard attempt {sequence} executed as priority")
        if protocol != "gemini_native" and label not in {"opencode-gemini-tools", "opencode-claude-native"} and not label.startswith("opencode-claude-effort-") and not response_tiers.intersection({"standard", "default"}):
            raise SystemExit(f"{label}: Standard attempt {sequence} lacks authoritative tier evidence")
    else:
        if selector == "header" and request_header != "fast":
            raise SystemExit(f"{label}: Fast attempt {sequence} lacks the exact Fast header")
        if selector == "body" and not request_tiers.intersection({"fast", "priority"}):
            raise SystemExit(f"{label}: Fast attempt {sequence} lacks a Fast body tier")
        if "priority" not in response_tiers:
            raise SystemExit(f"{label}: Fast attempt {sequence} lacks authoritative priority evidence")
        if label == "opencode-fast" and attempt.get("request_reasoning_effort") != "low":
            raise SystemExit(f"{label}: Fast model did not preserve its reasoning variant")

    event_types = set(attempt.get("response_event_types") or [])
    if protocol == "anthropic_messages" and not {"message_start", "message_stop"}.issubset(event_types):
        raise SystemExit(f"{label}: attempt {sequence} has an incomplete Messages SSE lifecycle")
    if protocol == "openai_responses" and not {"response.created", "response.completed"}.issubset(event_types):
        raise SystemExit(f"{label}: attempt {sequence} has an incomplete Responses SSE lifecycle")
    if label.startswith("codex-"):
        tool_types = set(attempt.get("request_tool_types") or [])
        required_tool_types = {"function", "custom", "tool_search"}
        if not required_tool_types.issubset(tool_types):
            raise SystemExit(f"{label}: attempt {sequence} lacks current Codex tool forms")
    if label.startswith("claude-code-"):
        control_fields = set(attempt.get("request_control_fields") or [])
        if not {"context_management", "output_config"}.issubset(control_fields):
            raise SystemExit(f"{label}: attempt {sequence} lacks current Claude Code controls")
        if "ephemeral" not in set(attempt.get("request_cache_control_types") or []):
            raise SystemExit(f"{label}: attempt {sequence} lacks the exact ephemeral cache marker")
        output_fields = set(attempt.get("request_output_config_fields") or [])
        if "effort" not in output_fields:
            seen = ",".join(sorted(output_fields)) or "none"
            raise SystemExit(f"{label}: attempt {sequence} lacks effort control (seen: {seen})")

if label.startswith("claude-code-"):
    structured_attempts = [attempt for attempt in attempts if attempt.get("request_has_structured_output_tool")]
    if not structured_attempts:
        seen = ";".join(
            f"{attempt.get('sequence')}:{','.join(attempt.get('request_output_config_fields') or [])}"
            for attempt in attempts
        )
        raise SystemExit(f"{label}: no executable attempt carried the structured output tool (seen: {seen})")

if label == "opencode-gemini-tools":
    if len(attempts) < 2:
        raise SystemExit(f"{label}: expected a tool-call turn and a replay turn")
    if not any(attempt.get("request_has_bash_tool") for attempt in attempts):
        raise SystemExit(f"{label}: OpenCode did not send its bash tool")
    if not any(attempt.get("request_has_schema_dialect") for attempt in attempts):
        raise SystemExit(f"{label}: raw AI SDK $schema evidence was not observed")
    if not any(attempt.get("request_has_exclusive_bounds") for attempt in attempts):
        raise SystemExit(f"{label}: raw AI SDK exclusive bound evidence was not observed")
    if not any(attempt.get("response_has_tool_call") for attempt in attempts):
        raise SystemExit(f"{label}: Gemini did not return a tool call")
    if not any(attempt.get("request_has_replayed_tool_history") for attempt in attempts):
        raise SystemExit(f"{label}: OpenCode did not replay tool history after execution")

if label == "opencode-claude-native":
    main_attempts = [attempt for attempt in attempts if attempt.get("model") == model]
    title_attempts = [attempt for attempt in attempts if attempt.get("model") == claude_small_model]
    if not main_attempts:
        raise SystemExit(f"{label}: OpenCode main-agent request was not observed")
    if not title_attempts:
        raise SystemExit(f"{label}: OpenCode title-agent request was not observed")
    if not any(attempt.get("request_reasoning_effort") == "low" for attempt in title_attempts):
        raise SystemExit(f"{label}: raw title-agent reasoning_effort=low was not observed")

if label.startswith("opencode-claude-effort-"):
    expected_effort = label.removeprefix("opencode-claude-effort-")
    if not any(attempt.get("request_reasoning_effort") == expected_effort for attempt in attempts):
        raise SystemExit(f"{label}: raw reasoning_effort={expected_effort} was not observed")

print(len(attempts))
PY
}

run_case() {
  local label=$1
  local protocol=$2
  local path=$3
  local model=$4
  local tier=$5
  local selector=$6
  local runner=$7

  CASE_DIR="$TEMP_ROOT/$label"
  mkdir -p "$CASE_DIR/work"
  start_proxy "$label"
  if ! "$runner" "$tier"; then
    printf 'FAIL %-24s client exited unsuccessfully or timed out\n' "$label" >&2
    return 1
  fi

  local attempts
  if ! attempts=$(assert_evidence "$label" "$protocol" "$path" "$model" "$tier" "$selector"); then
    printf 'FAIL %-24s protocol evidence rejected\n' "$label" >&2
    return 1
  fi
  stop_proxy
  printf 'PASS %-24s %s executable attempt(s)\n' "$label" "$attempts"
  PASSED_CASES=$((PASSED_CASES + 1))
}

run_matrix_case() {
  local label=$1
  if [[ -n $CASE_FILTER ]]; then
    case ",$CASE_FILTER," in
      *,"$label",*) ;;
      *) return ;;
    esac
  fi
  run_case "$@"
}

run_cline() {
  local tier=$1
  local config_dir="$CASE_DIR/config"
  local data_dir="$CASE_DIR/data"
  run_quiet cline auth openai-compatible \
    --apikey "$PLACEHOLDER_API_KEY" \
    --modelid "$OPENAI_MODEL" \
    --baseurl "$PROXY_BASE_URL/v1" \
    --config "$config_dir" \
    --data-dir "$data_dir"
  if [[ $tier == fast ]]; then
    local providers_file="$data_dir/settings/providers.json"
    jq '.providers["openai-compatible"].settings.headers = {"x-apitoken-service-tier":"fast"}' \
      "$providers_file" >"$providers_file.next"
    mv "$providers_file.next" "$providers_file"
  fi
  run_quiet cline --plan --json --auto-approve false \
    --cwd "$CASE_DIR/work" \
    --provider openai-compatible \
    --model "$OPENAI_MODEL" \
    --retries 0 \
    --timeout "$CASE_TIMEOUT_SECONDS" \
    --config "$config_dir" \
    --data-dir "$data_dir" \
    "$PROMPT"
}

run_continue() {
  local tier=$1
  local config_file="$CASE_DIR/continue.yaml"
  local fast_config=
  if [[ $tier == fast ]]; then
    fast_config=$'    requestOptions:\n      headers:\n        x-apitoken-service-tier: fast'
  fi
  printf '%s\n' \
    'name: API Token Router Harness' \
    'version: 1.0.0' \
    'schema: v1' \
    'models:' \
    '  - name: Router GPT' \
    "    provider: openai" \
    "    model: $OPENAI_MODEL" \
    "    apiKey: $PLACEHOLDER_API_KEY" \
    "    apiBase: $PROXY_BASE_URL/v1" \
    '    roles:' \
    '      - chat' \
    "$fast_config" >"$config_file"
  run_quiet env \
    CONTINUE_GLOBAL_DIR="$CASE_DIR/global" \
    CONTINUE_CLI_ENABLE_TELEMETRY=0 \
    cn --config "$config_file" --readonly -p --format json "$PROMPT"
}

openai_compatible_config() {
  local tier=$1
  local header_json='{}'
  if [[ $tier == fast ]]; then
    header_json='{"x-apitoken-service-tier":"fast"}'
  fi
  printf '{"provider":{"apitoken":{"npm":"@ai-sdk/openai-compatible","name":"API Token Router","options":{"baseURL":"%s/v1","apiKey":"%s","headers":%s},"models":{"%s":{"name":"Router GPT"}}}},"model":"apitoken/%s","small_model":"apitoken/%s"}' \
    "$PROXY_BASE_URL" "$PLACEHOLDER_API_KEY" "$header_json" \
    "$OPENAI_MODEL" "$OPENAI_MODEL" "$OPENAI_MODEL"
}

opencode_config() {
  local tier=$1
  local selected_model=$OPENAI_MODEL
  local model_config='{"name":"Router GPT","reasoning":true,"variants":{"low":{"reasoningEffort":"low"}}}'
  if [[ $tier == fast ]]; then
    selected_model="$OPENAI_MODEL-fast"
    model_config=$(printf '{"id":"%s","name":"Router GPT Fast","reasoning":true,"options":{"service_tier":"priority"},"variants":{"low":{"reasoningEffort":"low"}}}' "$OPENAI_MODEL")
  fi
  printf '{"provider":{"apitoken":{"npm":"@ai-sdk/openai-compatible","name":"API Token Router","options":{"baseURL":"%s/v1","apiKey":"%s"},"models":{"%s":%s}}},"model":"apitoken/%s","small_model":"apitoken/%s"}' \
    "$PROXY_BASE_URL" "$PLACEHOLDER_API_KEY" "$selected_model" "$model_config" \
    "$selected_model" "$selected_model"
}

run_opencode() {
  local tier=$1
  local config_content
  local selected_model=$OPENAI_MODEL
  if [[ $tier == fast ]]; then
    selected_model="$OPENAI_MODEL-fast"
  fi
  config_content=$(opencode_config "$tier")
  run_quiet env \
    OPENCODE_CONFIG_CONTENT="$config_content" \
    OPENCODE_CONFIG_DIR="$CASE_DIR/config" \
    OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_DISABLE_DEFAULT_PLUGINS=1 \
    OPENCODE_DISABLE_MODELS_FETCH=1 \
    OPENCODE_DISABLE_PROJECT_CONFIG=1 \
    XDG_CACHE_HOME="$TEMP_ROOT/opencode-cache" \
    XDG_CONFIG_HOME="$CASE_DIR/xdg-config" \
    XDG_DATA_HOME="$CASE_DIR/xdg-data" \
    XDG_STATE_HOME="$CASE_DIR/xdg-state" \
    opencode run --pure --format json \
      --model "apitoken/$selected_model" \
      --variant low \
      --agent plan \
      --title router-harness \
      --dir "$CASE_DIR/work" \
      "$PROMPT"
}

run_opencode_gemini_tools() {
  local _tier=$1
  local config_content
  config_content=$(printf '{"permission":"allow","provider":{"apitoken":{"npm":"@ai-sdk/openai-compatible","name":"API Token Router","options":{"baseURL":"%s/v1","apiKey":"%s"},"models":{"%s":{"name":"Router Gemini","tool_call":true}}}},"model":"apitoken/%s","small_model":"apitoken/%s"}' \
    "$PROXY_BASE_URL" "$PLACEHOLDER_API_KEY" "$OPENCODE_GEMINI_MODEL" \
    "$OPENCODE_GEMINI_MODEL" "$OPENCODE_GEMINI_MODEL")
  run_quiet env \
    OPENCODE_CONFIG_CONTENT="$config_content" \
    OPENCODE_CONFIG_DIR="$CASE_DIR/config" \
    OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_DISABLE_DEFAULT_PLUGINS=1 \
    OPENCODE_DISABLE_MODELS_FETCH=1 \
    OPENCODE_DISABLE_PROJECT_CONFIG=1 \
    XDG_CACHE_HOME="$TEMP_ROOT/opencode-gemini-cache" \
    XDG_CONFIG_HOME="$CASE_DIR/xdg-config" \
    XDG_DATA_HOME="$CASE_DIR/xdg-data" \
    XDG_STATE_HOME="$CASE_DIR/xdg-state" \
    opencode run --pure --auto --format json \
      --model "apitoken/$OPENCODE_GEMINI_MODEL" \
      --agent build \
      --dir "$CASE_DIR/work" \
      "$GEMINI_TOOL_PROMPT"
  [[ -f $CASE_DIR/work/opencode-gemini-tool-proof.txt ]] || return 1
  [[ $(<"$CASE_DIR/work/opencode-gemini-tool-proof.txt") == OPENCODE_GEMINI_TOOL_OK ]]
}

run_opencode_claude_native() {
  local _tier=$1
  local config_content
  config_content=$(printf '{"permission":"allow","provider":{"apitoken":{"npm":"@ai-sdk/openai-compatible","name":"API Token Router","options":{"baseURL":"%s/v1","apiKey":"%s"},"models":{"%s":{"name":"Router Claude","tool_call":true,"reasoning":true,"interleaved":{"field":"reasoning_content"}},"%s":{"name":"Router Claude Small","tool_call":true,"reasoning":true,"interleaved":{"field":"reasoning_content"}}}}},"model":"apitoken/%s","small_model":"apitoken/%s"}' \
    "$PROXY_BASE_URL" "$PLACEHOLDER_API_KEY" "$OPENCODE_CLAUDE_MODEL" \
    "$OPENCODE_CLAUDE_SMALL_MODEL" "$OPENCODE_CLAUDE_MODEL" "$OPENCODE_CLAUDE_SMALL_MODEL")
  run_quiet env \
    OPENCODE_CONFIG_CONTENT="$config_content" \
    OPENCODE_CONFIG_DIR="$CASE_DIR/config" \
    OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_DISABLE_DEFAULT_PLUGINS=1 \
    OPENCODE_DISABLE_MODELS_FETCH=1 \
    OPENCODE_DISABLE_PROJECT_CONFIG=1 \
    XDG_CACHE_HOME="$TEMP_ROOT/opencode-claude-cache" \
    XDG_CONFIG_HOME="$CASE_DIR/xdg-config" \
    XDG_DATA_HOME="$CASE_DIR/xdg-data" \
    XDG_STATE_HOME="$CASE_DIR/xdg-state" \
    opencode run --pure --auto --format json \
      --model "apitoken/$OPENCODE_CLAUDE_MODEL" \
      --agent plan \
      --dir "$CASE_DIR/work" \
      "$PROMPT"
}

run_opencode_claude_effort() {
  local _tier=$1
  local effort=$2
  local config_content
  config_content=$(printf '{"permission":"allow","provider":{"apitoken":{"npm":"@ai-sdk/openai-compatible","name":"API Token Router","options":{"baseURL":"%s/v1","apiKey":"%s"},"models":{"%s":{"name":"Router Claude Opus 5","reasoning":true,"interleaved":{"field":"reasoning_content"},"variants":{"xhigh":{"reasoningEffort":"xhigh"},"max":{"reasoningEffort":"max"}}}}}},"model":"apitoken/%s","small_model":"apitoken/%s"}' \
    "$PROXY_BASE_URL" "$PLACEHOLDER_API_KEY" "$OPENCODE_CLAUDE_EFFORT_MODEL" \
    "$OPENCODE_CLAUDE_EFFORT_MODEL" "$OPENCODE_CLAUDE_EFFORT_MODEL")
  run_quiet env \
    OPENCODE_CONFIG_CONTENT="$config_content" \
    OPENCODE_CONFIG_DIR="$CASE_DIR/config" \
    OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_DISABLE_DEFAULT_PLUGINS=1 \
    OPENCODE_DISABLE_MODELS_FETCH=1 \
    OPENCODE_DISABLE_PROJECT_CONFIG=1 \
    XDG_CACHE_HOME="$TEMP_ROOT/opencode-claude-effort-cache" \
    XDG_CONFIG_HOME="$CASE_DIR/xdg-config" \
    XDG_DATA_HOME="$CASE_DIR/xdg-data" \
    XDG_STATE_HOME="$CASE_DIR/xdg-state" \
    opencode run --pure --format json \
      --model "apitoken/$OPENCODE_CLAUDE_EFFORT_MODEL" \
      --variant "$effort" \
      --agent plan \
      --title router-harness \
      --dir "$CASE_DIR/work" \
      "$PROMPT"
}

run_opencode_claude_xhigh() {
  run_opencode_claude_effort "$1" xhigh
}

run_opencode_claude_max() {
  run_opencode_claude_effort "$1" max
}

run_kilo() {
  local tier=$1
  local config_content
  config_content=$(openai_compatible_config "$tier")
  run_quiet env \
    KILO_CONFIG_CONTENT="$config_content" \
    KILO_CONFIG_DIR="$CASE_DIR/config" \
    KILO_DISABLE_AUTOUPDATE=1 \
    KILO_DISABLE_DEFAULT_PLUGINS=1 \
    KILO_DISABLE_MODELS_FETCH=1 \
    KILO_DISABLE_PROJECT_CONFIG=1 \
    XDG_CACHE_HOME="$TEMP_ROOT/kilo-cache" \
    XDG_CONFIG_HOME="$CASE_DIR/xdg-config" \
    XDG_DATA_HOME="$CASE_DIR/xdg-data" \
    XDG_STATE_HOME="$CASE_DIR/xdg-state" \
    kilo run --pure --format json \
      --model "apitoken/$OPENAI_MODEL" \
      --agent plan \
      --title router-harness \
      --dir "$CASE_DIR/work" \
      "$PROMPT"
}

run_codex() {
  local tier=$1
  local tier_args=()
  if [[ $tier == fast ]]; then
    tier_args=(-c 'service_tier="fast"')
  fi
  run_quiet env OPENAI_API_KEY="$PLACEHOLDER_API_KEY" \
    codex exec \
      --ignore-user-config \
      --ignore-rules \
      --ephemeral \
      --skip-git-repo-check \
      --json \
      --color never \
      --sandbox read-only \
      --cd "$CASE_DIR/work" \
      --model "$OPENAI_MODEL" \
      -c 'model_provider="apitoken"' \
      -c 'model_providers.apitoken.name="API Token Router"' \
      -c "model_providers.apitoken.base_url=\"$PROXY_BASE_URL/v1\"" \
      -c 'model_providers.apitoken.env_key="OPENAI_API_KEY"' \
      -c 'model_providers.apitoken.wire_api="responses"' \
      -c 'web_search="disabled"' \
      "${tier_args[@]}" \
      "$PROMPT"
}

run_claude() {
  local tier=$1
  local custom_headers=
  if [[ $tier == fast ]]; then
    custom_headers='x-apitoken-service-tier: fast'
  fi
  local output_schema='{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}'
  run_quiet env \
    ANTHROPIC_API_KEY="$PLACEHOLDER_API_KEY" \
    ANTHROPIC_BASE_URL="$PROXY_BASE_URL" \
    ANTHROPIC_CUSTOM_HEADERS="$custom_headers" \
    CLAUDE_CONFIG_DIR="$CASE_DIR/claude-config" \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
    DISABLE_AUTOUPDATER=1 \
    DISABLE_ERROR_REPORTING=1 \
    DISABLE_TELEMETRY=1 \
    claude \
      --tools '' \
      --strict-mcp-config \
      --mcp-config '{"mcpServers":{}}' \
      --setting-sources '' \
      --print \
      --output-format json \
      --no-session-persistence \
      --max-turns 1 \
      --max-budget-usd 1 \
      --effort low \
      --model "$OPENAI_MODEL" \
      --json-schema "$output_schema" \
      'Return only {"ok":true}. Do not call tools.'
}

run_gemini() {
  local _tier=$1
  local gemini_home="$CASE_DIR/gemini-home"
  mkdir -p "$gemini_home/.gemini"
  printf '%s\n' '{"security":{"auth":{"selectedType":"gemini-api-key"}},"privacy":{"usageStatisticsEnabled":false}}' \
    >"$gemini_home/.gemini/settings.json"
  run_quiet env \
    GEMINI_CLI_HOME="$gemini_home" \
    GEMINI_API_KEY="$PLACEHOLDER_API_KEY" \
    GOOGLE_GEMINI_BASE_URL="$PROXY_BASE_URL" \
    gemini \
      --prompt "$PROMPT" \
      --model "$GEMINI_MODEL" \
      --approval-mode plan \
      --skip-trust \
      --output-format json
}

run_hermes() {
  local tier=$1
  local default_headers=
  local service_tier=
  if [[ $tier == fast ]]; then
    default_headers=$'  default_headers:\n    x-apitoken-service-tier: fast'
    service_tier='  service_tier: fast'
  fi
  printf '%s\n' \
    'model:' \
    "  default: $OPENAI_MODEL" \
    '  provider: custom' \
    '  api_mode: chat_completions' \
    "  api_key: $PLACEHOLDER_API_KEY" \
    "  base_url: $PROXY_BASE_URL/v1" \
    '  context_length: 131072' \
    '  max_tokens: 32' \
    "$default_headers" \
    'agent:' \
    '  max_turns: 1' \
    '  reasoning_effort: low' \
    "$service_tier" >"$CASE_DIR/config.yaml"
  run_quiet env \
    HERMES_HOME="$CASE_DIR" \
    CUSTOM_API_KEY="$PLACEHOLDER_API_KEY" \
    CUSTOM_BASE_URL="$PROXY_BASE_URL/v1" \
    HERMES_DISABLE_TELEMETRY=1 \
    hermes --ignore-rules --oneshot "$PROMPT"
}

run_aider() {
  local tier=$1
  local model_settings="$CASE_DIR/model-settings.yml"
  local extra_params=
  if [[ $tier == fast ]]; then
    extra_params=$'  extra_params:\n    service_tier: priority'
  fi
  printf '%s\n' \
    "- name: openai/$OPENAI_MODEL" \
    '  edit_format: whole' \
    '  use_repo_map: false' \
    '  use_temperature: false' \
    '  streaming: false' \
    "$extra_params" >"$model_settings"
  run_quiet env \
    OPENAI_API_KEY="$PLACEHOLDER_API_KEY" \
    OPENAI_API_BASE="$PROXY_BASE_URL/v1" \
    aider \
      --model "openai/$OPENAI_MODEL" \
      --model-settings-file "$model_settings" \
      --message "$PROMPT" \
      --no-git \
      --no-auto-commits \
      --no-dirty-commits \
      --no-gitignore \
      --no-add-gitignore-files \
      --map-tokens 0 \
      --no-check-update \
      --no-analytics \
      --no-pretty \
      --no-stream \
      --no-show-model-warnings \
      --no-check-model-accepts-settings \
      --no-restore-chat-history \
      --no-suggest-shell-commands \
      --disable-playwright \
      --yes-always \
      --input-history-file "$CASE_DIR/input.history" \
      --chat-history-file "$CASE_DIR/chat.history.md" \
      --llm-history-file "$CASE_DIR/llm.history"
}

run_matrix_case cline-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_cline
run_matrix_case cline-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast header run_cline
run_matrix_case continue-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_continue
run_matrix_case continue-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast header run_continue
run_matrix_case opencode-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_opencode
run_matrix_case opencode-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast body run_opencode
run_matrix_case opencode-claude-native openai_chat /v1/chat/completions "$OPENCODE_CLAUDE_MODEL" standard none run_opencode_claude_native
run_matrix_case opencode-claude-effort-xhigh openai_chat /v1/chat/completions "$OPENCODE_CLAUDE_EFFORT_MODEL" standard none run_opencode_claude_xhigh
run_matrix_case opencode-claude-effort-max openai_chat /v1/chat/completions "$OPENCODE_CLAUDE_EFFORT_MODEL" standard none run_opencode_claude_max
run_matrix_case opencode-gemini-tools openai_chat /v1/chat/completions "$OPENCODE_GEMINI_MODEL" standard none run_opencode_gemini_tools
run_matrix_case kilo-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_kilo
run_matrix_case kilo-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast header run_kilo
run_matrix_case codex-standard openai_responses /v1/responses "$OPENAI_MODEL" standard none run_codex
run_matrix_case codex-fast openai_responses /v1/responses "$OPENAI_MODEL" fast body run_codex
run_matrix_case claude-code-standard anthropic_messages /v1/messages "$OPENAI_MODEL" standard none run_claude
run_matrix_case claude-code-fast anthropic_messages /v1/messages "$OPENAI_MODEL" fast header run_claude
run_matrix_case gemini-cli-standard gemini_native "/v1beta/models/$GEMINI_MODEL:streamGenerateContent" "$GEMINI_MODEL" standard none run_gemini
run_matrix_case hermes-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_hermes
run_matrix_case hermes-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast header run_hermes
run_matrix_case aider-standard openai_chat /v1/chat/completions "$OPENAI_MODEL" standard none run_aider
run_matrix_case aider-fast openai_chat /v1/chat/completions "$OPENAI_MODEL" fast body run_aider

roo_version=$(code --list-extensions --show-versions 2>/dev/null | awk -F@ '$1 == "rooveterinaryinc.roo-cline" {print $2; exit}') || true
if [[ -n $roo_version ]]; then
  printf 'SKIP %-24s extension %s has compatible OpenAI base/model/tier settings but no official headless CLI\n' \
    roo-code "$roo_version"
else
  printf 'SKIP %-24s extension not installed and no official headless CLI is published\n' roo-code
fi

if [[ -n $CASE_FILTER && $PASSED_CASES -eq 0 ]]; then
  printf 'APITOKEN_HARNESS_CASES did not match a matrix case\n' >&2
  exit 2
fi

printf 'Router harness live matrix passed %s executable Standard/Fast checks; Roo Code reported separately.\n' \
  "$PASSED_CASES"

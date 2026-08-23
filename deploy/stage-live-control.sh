#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 && ${SUDO_USER:-} == stage-ctl ]] || { echo 'stage-live-control: caller rejected' >&2; exit 1; }
STATE=/var/lib/apitoken/watchdog
STAGE_STATE=/var/lib/apitoken-staging/watchdog
META=$STATE/stage-live.json
KEY=$STATE/stage-live.key
exec 8>/run/lock/apitoken-stage-live.lock; flock -n 8 || { echo 'stage-live-control: locked' >&2; exit 1; }
# shellcheck disable=SC1091
source /etc/apitoken/server.env
: "${CLAUDE_API_CONTROL_KEY:?production control key missing}"
control_curl() { curl -fsS -m 15 -K - "$@" <<EOF
header = "x-api-key: $CLAUDE_API_CONTROL_KEY"
header = "content-type: application/json"
EOF
}
case "${1:-}" in
  enable)
    [[ $# -eq 4 && $2 =~ ^[0-9]+$ && $3 =~ ^[0-9]+$ && $4 =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
    cap=$2 ttl=$3 actor=$4
    (( cap >= 100000 && cap <= 100000000 )) || { echo 'cap must be 100000..100000000 nanoUSD' >&2; exit 2; }
    (( ttl >= 300 && ttl <= 86400 )) || { echo 'ttl must be 300..86400 seconds' >&2; exit 2; }
    [[ ! -e $META && ! -e /etc/apitoken-staging/stage-live.enabled ]] || { echo 'stage-live-control: already enabled' >&2; exit 1; }
    response=$(mktemp); trap 'rm -f "$response"' EXIT
    account=$(control_curl -X POST http://127.0.0.1:8790/admin/account --data '{"handle":"stage-live","mult_bp":10000}' | jq -er '.account')
    expires=$(( $(date +%s) + ttl ))
    control_curl -X POST http://127.0.0.1:8790/admin/key --data "{\"account_id\":\"$account\",\"label\":\"stage-live-$actor\",\"spend_limit_nano\":\"$cap\",\"expires_ts\":$expires}" >"$response"
    jq -er '.key' "$response" >"$KEY"; chmod 0600 "$KEY"; key_id=$(jq -er '.key_id' "$response")
    control_curl -X POST "http://127.0.0.1:8790/admin/account/$account/credit" --data "{\"amount_nano\":\"$cap\",\"ref\":\"stage-live:$key_id\"}" >/dev/null
    install -o root -g deploy-stage -m 0640 "$KEY" /etc/apitoken-staging/stage-live.key
    install -o root -g deploy-stage -m 0640 /dev/null /etc/apitoken-staging/stage-live.enabled
    jq -cn --arg account "$account" --arg key_id "$key_id" --arg cap "$cap" --arg actor "$actor" --argjson expires "$expires" '{account:$account,key_id:$key_id,cap_nanousd:$cap,actor:$actor,expires_ts:$expires}' >"$META"; chmod 0600 "$META"
    ip netns exec apitoken-stage nft add rule inet apitoken_stage output ip daddr 10.254.32.1 tcp dport 9081 accept 2>/dev/null || true
    systemctl start apitoken-stage-live-host-proxy.service apitoken-stage-live-client.service
    printf 'stage-live-control: enabled key_id=%s cap_nanousd=%s expires_ts=%s\n' "$key_id" "$cap" "$expires"
    ;;
  probe)
    [[ $# -eq 3 && $2 =~ ^[A-Za-z0-9._:-]{1,128}$ && $3 =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
    [[ -s $META && -s /etc/apitoken-staging/stage-live.key ]] || { echo 'stage-live-control: not enabled' >&2; exit 1; }
    key_id=$(jq -er '.key_id' "$META"); [[ ! -e $STAGE_STATE/live-probe-$key_id.json ]] || { echo 'stage-live-control: probe already consumed' >&2; exit 1; }
    model=$2 actor=$3; body=$(mktemp); trap 'rm -f "$body"' EXIT
    code=$(ip netns exec apitoken-stage runuser -u deploy-stage -- curl -sS -m 45 -o "$body" -w '%{http_code}' -H 'content-type: application/json' -X POST http://10.254.32.2:9081/v1/messages --data "{\"model\":\"$model\",\"max_tokens\":1,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with one word: ok\"}]}")
    [[ $code == 200 ]] || { echo "stage-live-control: live probe HTTP $code" >&2; exit 1; }
    jq -e '.type=="message" and (.usage.input_tokens|type=="number") and (.usage.output_tokens|type=="number")' "$body" >/dev/null
    digest=$(sha256sum "$body" | cut -d' ' -f1); now=$(date +%s)
    jq -cn --arg key_id "$key_id" --arg model "$model" --arg actor "$actor" --arg digest "$digest" --argjson now "$now" '{key_id:$key_id,model:$model,actor:$actor,http_status:200,response_digest:$digest,probed_at:$now}' >"$STAGE_STATE/live-probe-$key_id.json"
    chown root:deploy-stage "$STAGE_STATE/live-probe-$key_id.json"; chmod 0640 "$STAGE_STATE/live-probe-$key_id.json"
    printf 'stage-live-control: probe GREEN key_id=%s model=%s response_digest=%s\n' "$key_id" "$model" "$digest"
    ;;
  disable)
    [[ $# -eq 2 && $2 =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
    if [[ -s $META ]]; then
      key_id=$(jq -er '.key_id' "$META")
      control_curl -X POST "http://127.0.0.1:8790/admin/key-id/$key_id/status" --data '{"status":"disabled"}' >/dev/null
    fi
    systemctl stop apitoken-stage-live-client.service apitoken-stage-live-host-proxy.service
    rm -f -- /etc/apitoken-staging/stage-live.enabled /etc/apitoken-staging/stage-live.key "$KEY" "$META"
    printf 'stage-live-control: disabled actor=%s\n' "$2"
    ;;
  *) echo 'usage: stage-live-control.sh enable CAP TTL ACTOR | probe MODEL ACTOR | disable ACTOR' >&2; exit 2 ;;
esac

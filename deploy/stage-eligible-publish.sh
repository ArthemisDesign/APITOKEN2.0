#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || exit 1
STATE=/var/lib/apitoken-staging/watchdog
POLICY=/usr/local/lib/apitoken-watchdog/stage-degradation-policy.json
[[ $# -eq 3 && $1 =~ ^[0-9a-f]{40}$ && $2 =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
sha=$1 actor=$2 reason=$3
[[ -n $reason && ${#reason} -le 512 ]] || exit 2
for marker in source candidate deployed processed; do [[ $(cat "$STATE/$marker.sha" 2>/dev/null) == "$sha" ]] || exit 1; done
repo=/opt/apitoken-staging/repo; tree=$(git -c safe.directory="$repo" -C "$repo" rev-parse "$sha^{tree}")
now=$(date +%s); policy=$(sha256sum "$POLICY" | cut -d' ' -f1)
payload=$(jq -cn --arg sha "$sha" --arg tree "$tree" --arg actor "$actor" --arg reason "$reason" --arg policy "$policy" --argjson now "$now" '{mode:"promotion",unix_user:"deploy",github_actor:$actor,commit_sha:$sha,tree_sha:$tree,artifact_digests:{},policy_digest:$policy,contour_id:"stage",issued_at:$now,expires_at:($now+86400),reason:$reason,candidate_marker:$sha}')
digest=$(printf '%s' "$payload" | sha256sum | cut -d' ' -f1)
printf '%s\n' "$payload" | jq --arg digest "$digest" '.+{record_digest:$digest}' >"$STATE/.promotion-eligible.tmp"
chown root:deploy-stage "$STATE/.promotion-eligible.tmp"; chmod 0640 "$STATE/.promotion-eligible.tmp"
mv -f "$STATE/.promotion-eligible.tmp" "$STATE/promotion-eligible.json"
printf 'stage-eligible-publish: eligible %s\n' "$sha"

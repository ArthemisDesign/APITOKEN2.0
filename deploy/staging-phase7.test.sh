#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd); T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
bash -n "$ROOT/deploy/promotion-admission.sh" "$ROOT/deploy/stage-emergency-guard.sh" "$ROOT/deploy/stage-eligible-publish.sh" "$ROOT/deploy/stage-promotion-helper.sh"
grep -Fq 'if [[ $PROCESSED_SHA != "$CANDIDATE_SHA" ]]' "$ROOT/deploy/watchdog.sh"
grep -Fq 'promotion-admission.sh "$CANDIDATE_SHA"' "$ROOT/deploy/watchdog.sh"
grep -Fq 'MemAvailable' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fq '12582912' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fq 'systemctl stop staging.slice' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fxq 'OnUnitInactiveSec=15s' "$ROOT/systemd/apitoken-stage-emergency-guard.timer"
grep -Fq 'APITOKEN_PROMOTION' "$ROOT/deploy/sudoers.d/95-apitoken-deploy"
grep -Fq 'stage-eligible-publish.sh [0-9a-f]* *' "$ROOT/deploy/sudoers.d/96-apitoken-stage"
grep -Fq 'stage-promotion-helper.sh attest' "$ROOT/deploy/sudoers.d/96-apitoken-stage"
mkdir -p "$T/prod" "$T/stage" "$T/repo"; git -C "$T/repo" init -q; git -C "$T/repo" config user.name test; git -C "$T/repo" config user.email test@example.invalid; echo x >"$T/repo/x"; git -C "$T/repo" add x; git -C "$T/repo" commit -qm base; sha=$(git -C "$T/repo" rev-parse HEAD); tree=$(git -C "$T/repo" rev-parse HEAD^{tree}); now=$(date +%s)
sed -e 's/^\[\[ \${EUID:-\$(id -u)} -eq 0 \]\].*/:/' -e "s#PROD=/var/lib/apitoken/watchdog#PROD=$T/prod#" -e "s#STAGE=/var/lib/apitoken-staging/watchdog#STAGE=$T/stage#" -e "s#REPO=/opt/apitoken/repo#REPO=$T/repo#" "$ROOT/deploy/promotion-admission.sh" >"$T/admit"; chmod +x "$T/admit"
"$T/admit" "$sha" | grep -Fq bootstrap
if "$T/admit" "$sha" >/dev/null 2>&1; then exit 1; fi
grep -Fxq "$sha" "$T/prod/admission-rejected.sha"
record() { jq -cn --arg mode "$1" --arg sha "$sha" --arg tree "$tree" --argjson now "$now" '{mode:$mode,unix_user:"deploy",commit_sha:$sha,tree_sha:$tree,issued_at:$now,expires_at:($now+3600),policy_digest:("a"*64),record_digest:("b"*64)}'; }
record promotion >"$T/stage/promotion-eligible.json"; "$T/admit" "$sha" | grep -Fq promotion-eligible
rm "$T/stage/promotion-eligible.json"; record hotfix >"$T/prod/hotfix-eligible.json"; "$T/admit" "$sha" | grep -Fq hotfix-eligible
jq '.expires_at=0' "$T/prod/hotfix-eligible.json" >"$T/x"; mv "$T/x" "$T/prod/hotfix-eligible.json"; if "$T/admit" "$sha" >/dev/null 2>&1; then exit 1; fi
printf 'staging-phase7.test: PASS\n'

#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd); T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
bash -n "$ROOT/deploy/promotion-admission.sh" "$ROOT/deploy/stage-emergency-guard.sh" "$ROOT/deploy/stage-eligible-publish.sh"
grep -Fq 'if [[ $PROCESSED_SHA != "$CANDIDATE_SHA" ]]' "$ROOT/deploy/watchdog.sh"
grep -Fq 'promotion-admission.sh "$CANDIDATE_SHA"' "$ROOT/deploy/watchdog.sh"
grep -Fq 'staging-admission.enabled' "$ROOT/deploy/watchdog.sh"
grep -Fq 'MemAvailable' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fq '12582912' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fq 'systemctl stop staging.slice' "$ROOT/deploy/stage-emergency-guard.sh"
grep -Fxq 'OnUnitInactiveSec=15s' "$ROOT/systemd/apitoken-stage-emergency-guard.timer"
grep -Fq 'APITOKEN_PROMOTION' "$ROOT/deploy/sudoers.d/95-apitoken-deploy"
grep -Fq 'stage-eligible-publish.sh [0-9a-f]* *' "$ROOT/deploy/sudoers.d/96-apitoken-stage"
# Bootstrap must admit the enabling SHA; the next unattested SHA must fail and be audited.
mkdir -p "$T/prod" "$T/stage" "$T/repo"; git -C "$T/repo" init -q; git -C "$T/repo" config user.name test; git -C "$T/repo" config user.email test@example.invalid; echo x >"$T/repo/x"; git -C "$T/repo" add x; git -C "$T/repo" commit -qm base; sha=$(git -C "$T/repo" rev-parse HEAD)
sed -e 's/^\[\[ \${EUID:-\$(id -u)} -eq 0 \]\].*/:/' -e "s#PROD=/var/lib/apitoken/watchdog#PROD=$T/prod#" -e "s#STAGE=/var/lib/apitoken-staging/watchdog#STAGE=$T/stage#" -e "s#REPO=/opt/apitoken/repo#REPO=$T/repo#" "$ROOT/deploy/promotion-admission.sh" >"$T/admit"; chmod +x "$T/admit"
"$T/admit" "$sha" | grep -Fq bootstrap
touch "$T/prod/staging-admission.enabled"
if "$T/admit" "$sha" >/dev/null 2>&1; then exit 1; fi
grep -Fxq "$sha" "$T/prod/admission-rejected.sha"
printf 'staging-phase7.test: PASS\n'

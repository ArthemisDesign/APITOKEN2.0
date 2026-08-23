#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd); T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
repo=$T/repo; state=$T/state; mkdir -p "$repo" "$state"; git -C "$repo" init -q; git -C "$repo" config user.name test; git -C "$repo" config user.email test@example.invalid; echo x >"$repo/x"; git -C "$repo" add x; git -C "$repo" commit -qm base; sha=$(git -C "$repo" rev-parse HEAD); echo "$sha" >"$state/deployed.sha"
mkdir -p "$T/usr/local/lib/apitoken-watchdog"; cp "$ROOT/deploy/stage-degradation-policy.json" "$T/usr/local/lib/apitoken-watchdog/stage-degradation-policy.json"
# The test fallback reads the repository policy when the installed absolute path is absent.
(cd "$ROOT" && STAGE_POLICY_FILE=deploy/stage-degradation-policy.json python3 deploy/stage-attestation.py --mode promotion --commit "$sha" --actor owner --reason drill --state-root "$state" --repo "$repo" --now 1000 >"$T/promotion.json")
jq -e '.mode=="promotion" and .unix_user=="deploy" and .expires_at==87400 and (.record_digest|test("^[0-9a-f]{64}$"))' "$T/promotion.json" >/dev/null
(cd "$ROOT" && STAGE_POLICY_FILE=deploy/stage-degradation-policy.json python3 deploy/stage-attestation.py --mode hotfix --commit "$sha" --actor owner --reason emergency --state-root "$state" --repo "$repo" --now 2000 >"$T/hotfix.json")
jq -e '.mode=="hotfix" and .reason=="emergency"' "$T/hotfix.json" >/dev/null
echo 0000000000000000000000000000000000000000 >"$state/deployed.sha"
if (cd "$ROOT" && STAGE_POLICY_FILE=deploy/stage-degradation-policy.json python3 deploy/stage-attestation.py --mode promotion --commit "$sha" --actor owner --reason stale --state-root "$state" --repo "$repo" >/dev/null 2>&1); then exit 1; fi
printf 'staging-phase6-drills.test: PASS\n'

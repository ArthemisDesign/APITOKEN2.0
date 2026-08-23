#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'promotion-admission: root required' >&2; exit 1; }
[[ $# -eq 1 && $1 =~ ^[0-9a-f]{40}$ ]] || exit 2
sha=$1
PROD=/var/lib/apitoken/watchdog
STAGE=/var/lib/apitoken-staging/watchdog
REPO=/opt/apitoken/repo
# The implementation delivery enables admission only after its own complete GREEN cycle.
if [[ ! -f $PROD/staging-admission.enabled ]]; then
  printf '%s\n' "$sha" >"$PROD/staging-admission.enabled"
  chmod 0644 "$PROD/staging-admission.enabled"
  echo 'promotion-admission: bootstrap cycle'
  exit 0
fi
now=$(date +%s)
tree=$(git -c safe.directory="$REPO" -C "$REPO" rev-parse "$sha^{tree}")
for file in "$STAGE/promotion-eligible.json" "$PROD/hotfix-eligible.json"; do
  [[ -f $file && ! -L $file ]] || continue
  if jq -e --arg sha "$sha" --arg tree "$tree" --argjson now "$now" '
      .commit_sha==$sha and .tree_sha==$tree and .unix_user=="deploy" and
      .issued_at <= $now and .expires_at > $now and
      .mode == (if input_filename|contains("hotfix") then "hotfix" else "promotion" end) and
      (.policy_digest|test("^[0-9a-f]{64}$")) and (.record_digest|test("^[0-9a-f]{64}$"))
    ' "$file" >/dev/null; then
    echo "promotion-admission: admitted $sha via $(basename "$file")"
    exit 0
  fi
done
printf '%s\n' "$sha" >"$PROD/admission-rejected.sha"
chmod 0644 "$PROD/admission-rejected.sha"
echo "promotion-admission: rejected unattested master SHA $sha" >&2
exit 1

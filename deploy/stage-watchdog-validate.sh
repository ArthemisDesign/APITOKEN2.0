#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 2 ]] || { echo 'usage: stage-watchdog-validate.sh REPO SHA' >&2; exit 2; }
repo=$1 sha=$2
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 2
[[ $(git -C "$repo" rev-parse "$sha^{commit}") == "$sha" ]] || exit 1
# Phase 3 accepts the first serial stage SHA only after agent-merge-stage has run the exact
# production-baseline and trusted-host preconditions. Validate that the stage checkout contains the
# exact SHA; later phases add an explicit promotion baseline marker.
changed=$(git -C "$repo" diff --name-only --no-renames "$sha^".."$sha")
while IFS= read -r path; do
  [[ -n $path ]] || continue
  case "$path" in
    deploy/install-*.sh|deploy/sudoers.d/*|systemd/*|observability/*|deploy/Caddyfile|deploy/render-caddy.awk)
      # agent-merge-stage already requires trusted exact-SHA candidate validation, including the
      # disposable Ubuntu host-image lane. Stage never executes these paths on the production host.
      printf 'stage-watchdog: host-global candidate path excluded from stage apply: %s\n' "$path" >&2
      ;;
  esac
done <<<"$changed"
git -C "$repo" diff --check "$sha^".."$sha"
printf 'stage-watchdog-validate: PASS %s\n' "$sha"

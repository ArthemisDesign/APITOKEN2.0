#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 2 ]] || { echo 'usage: stage-watchdog-validate.sh REPO SHA' >&2; exit 2; }
repo=$1 sha=$2
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 2
[[ $(git -C "$repo" rev-parse "$sha^{commit}") == "$sha" ]] || exit 1
git -C "$repo" merge-base --is-ancestor origin/master "$sha"
changed=$(git -C "$repo" diff --name-only --no-renames origin/master.."$sha")
while IFS= read -r path; do
  [[ -n $path ]] || continue
  case "$path" in
    deploy/install-*.sh|deploy/sudoers.d/*|systemd/*|observability/*|deploy/Caddyfile|deploy/render-caddy.awk)
      printf 'stage-watchdog: host-global candidate path rejected: %s\n' "$path" >&2
      exit 1
      ;;
  esac
done <<<"$changed"
git -C "$repo" diff --check origin/master.."$sha"
printf 'stage-watchdog-validate: PASS %s\n' "$sha"

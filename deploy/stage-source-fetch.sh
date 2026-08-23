#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-source-fetch: root required' >&2; exit 1; }
SOURCE=/opt/apitoken/repo
TARGET=/opt/apitoken-staging/repo
STATE=/var/lib/apitoken-staging/watchdog
[[ -d $SOURCE/.git && ! -L $SOURCE ]] || { echo 'stage-source-fetch: production source missing' >&2; exit 1; }
if ! runuser -u deploy -- git -c safe.directory="$SOURCE" -C "$SOURCE" fetch --quiet --no-tags origin '+refs/heads/stage:refs/remotes/origin/stage'; then
  # The stage branch is absent before the first serial batch. This is an idle state, not a failure.
  exit 0
fi
sha=$(runuser -u deploy -- git -c safe.directory="$SOURCE" -C "$SOURCE" rev-parse 'refs/remotes/origin/stage^{commit}')
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 1
if [[ ! -d $TARGET/.git ]]; then
  rm -rf --one-file-system "$TARGET"
  git clone --no-hardlinks --no-checkout "$SOURCE" "$TARGET"
fi
git -c safe.directory="$TARGET" -C "$TARGET" fetch --quiet --no-tags "$SOURCE" \
  "+refs/remotes/origin/stage:refs/remotes/origin/stage"
chmod -R g+rX "$TARGET/.git"
printf '%s\n' "$sha" >"$STATE/source.sha"
chmod 0640 "$STATE/source.sha"

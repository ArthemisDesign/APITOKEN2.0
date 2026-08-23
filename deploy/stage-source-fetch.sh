#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-source-fetch: root required' >&2; exit 1; }
SOURCE=/opt/apitoken/repo
TARGET=/opt/apitoken-staging/repo
STATE=/var/lib/apitoken-staging/watchdog
[[ -d $SOURCE/.git && ! -L $SOURCE ]] || { echo 'stage-source-fetch: production source missing' >&2; exit 1; }
if ! runuser -u deploy -- git -c safe.directory="$SOURCE" -C "$SOURCE" fetch --quiet --no-tags origin '+refs/heads/stage:refs/remotes/origin/stage'; then
  exit 0
fi
sha=$(runuser -u deploy -- git -c safe.directory="$SOURCE" -C "$SOURCE" rev-parse 'refs/remotes/origin/stage^{commit}')
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 1
rm -rf --one-file-system "$TARGET"
install -d -o deploy-stage -g deploy-stage -m 0750 "$TARGET"
tar -C "$SOURCE" -cf - .git | tar -C "$TARGET" -xf -
chown -R deploy-stage:deploy-stage "$TARGET/.git"
printf '%s\n' "$sha" >"$STATE/source.sha"
chmod 0640 "$STATE/source.sha"

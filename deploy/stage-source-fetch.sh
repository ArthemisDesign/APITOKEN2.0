#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-source-fetch: root required' >&2; exit 1; }
[[ ${SUDO_USER:-} == deploy-stage ]] || { echo 'stage-source-fetch: caller rejected' >&2; exit 1; }
[[ $# -eq 1 && $1 == stage ]] || { echo 'stage-source-fetch: branch rejected' >&2; exit 2; }
SOURCE=/opt/apitoken/repo
TARGET=/opt/apitoken-staging/repo
[[ -d $SOURCE/.git && ! -L $SOURCE ]] || { echo 'stage-source-fetch: production source missing' >&2; exit 1; }
git -C "$SOURCE" fetch --quiet --no-tags origin '+refs/heads/stage:refs/remotes/origin/stage'
sha=$(git -C "$SOURCE" rev-parse 'refs/remotes/origin/stage^{commit}')
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 1
if [[ ! -d $TARGET/.git ]]; then
  rm -rf --one-file-system "$TARGET"
  git clone --no-hardlinks --no-checkout "$SOURCE" "$TARGET"
fi
git -C "$TARGET" fetch --quiet --no-tags "$SOURCE" \
  "+refs/remotes/origin/stage:refs/remotes/origin/stage"
chown -R deploy-stage:deploy-stage "$TARGET"
printf '%s\n' "$sha"

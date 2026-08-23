#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'staging-image-seed: root required' >&2; exit 1; }
STAGE_USER=deploy-stage
SOCKET=/run/apitoken-staging/docker.sock
for image in \
  postgres:18-alpine@sha256:d3e1620b530c944afa6e887d22eb899824da68e19c52024bf98f5220c88a65b2 \
  redis:7.4.2-alpine@sha256:02419de7eddf55aa5bcf49efb74e88fa8d931b4d77c07eff8a6b2144472b6952; do
  archive=$(mktemp /var/lib/apitoken-staging/.image-seed.XXXXXX.tar)
  docker image inspect "$image" >/dev/null 2>&1 || docker pull "$image" >/dev/null
  source_id=$(docker image inspect --format '{{.Id}}' "$image")
  source_tag=$(docker image inspect --format '{{index .RepoTags 0}}' "$image")
  [[ $source_id == sha256:* && $source_tag == *:* ]] \
    || { echo "staging-image-seed: source image lacks stable identity" >&2; exit 1; }
  docker save -o "$archive" "$source_tag"
  chown "$STAGE_USER:$STAGE_USER" "$archive"
  runuser -u "$STAGE_USER" -- env DOCKER_HOST="unix://$SOCKET" docker load -i "$archive" >/dev/null
  if ! runuser -u "$STAGE_USER" -- env DOCKER_HOST="unix://$SOCKET" docker image inspect "$image" >/dev/null 2>&1; then
    runuser -u "$STAGE_USER" -- env DOCKER_HOST="unix://$SOCKET" docker image inspect "$source_id" >/dev/null
    runuser -u "$STAGE_USER" -- env DOCKER_HOST="unix://$SOCKET" docker tag "$source_id" "$image"
  fi
  rm -f "$archive"
done
printf 'staging-image-seed: pinned images loaded into rootless daemon\n'

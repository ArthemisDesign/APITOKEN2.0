#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-store-diagnostics: root required' >&2; exit 1; }
container=${1:-}
case "$container" in
  apitoken-postgres-stage|apitoken-redis-history-stage|apitoken-redis-affinity-stage) ;;
  *) echo 'stage-store-diagnostics: container rejected' >&2; exit 2 ;;
esac
runuser -u deploy-stage -- env DOCKER_HOST=unix:///run/apitoken-staging/docker.sock \
  docker inspect --format 'status={{.State.Status}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}} error={{.State.Error}}' "$container"
runuser -u deploy-stage -- env DOCKER_HOST=unix:///run/apitoken-staging/docker.sock \
  docker logs --tail 80 "$container" 2>&1

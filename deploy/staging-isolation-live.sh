#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'staging-isolation-live: root required' >&2; exit 1; }
ip netns list | awk '{print $1}' | grep -Fxq apitoken-stage \
  || { echo 'stage netns missing' >&2; exit 1; }
# All denied connections must fail. Run only after trusted foundation apply.
for target in 127.0.0.1:5433 127.0.0.1:6379 127.0.0.1:6380 127.0.0.1:8790 \
  127.0.0.1:8791 127.0.0.1:8792 127.0.0.1:8794 127.0.0.1:8802 \
  127.0.0.1:13306 127.0.0.1:3010 127.0.0.1:5440 127.0.0.1:3900; do
  host=${target%:*}; port=${target##*:}
  if timeout 1 ip netns exec apitoken-stage bash -c "</dev/tcp/$host/$port" 2>/dev/null; then
    echo "stage isolation reached denied endpoint $target" >&2; exit 1
  fi
done
for path in /etc/apitoken /srv/claude-api/data /var/lib/apitoken/watchdog /var/run/docker.sock; do
  if runuser -u deploy-stage -- test -r "$path"; then echo "deploy-stage reads $path" >&2; exit 1; fi
done
ss -H -lnt | awk '$4 ~ /^(127\.0\.0\.1|0\.0\.0\.0|\[::\]):/ {print $4}' | grep -E ':(5434|13000|16379|18787|18788)$' \
  && { echo 'stage listener escaped to host/public address' >&2; exit 1; } || true
printf 'staging-isolation-live: PASS\n'

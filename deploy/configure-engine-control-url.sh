#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

CONTROL_URL=${ENGINE_CONTROL_BASE_URL:-http://127.0.0.1:8790}
CONTROL_READY_URL=$CONTROL_URL/ready
API_ENV=${API_ENV_FILE:-/etc/apitoken/api.env}
WORKER_ENV=${WORKER_ENV_FILE:-/etc/apitoken/worker.env}
CHECK_ONLY=0

[[ $CONTROL_URL =~ ^http://127\.0\.0\.1:[0-9]+$ ]] \
  || { echo "ENGINE_CONTROL_BASE_URL must be a loopback HTTP origin with an explicit port" >&2; exit 2; }

case ${1:-} in
  '') ;;
  --check) CHECK_ONLY=1 ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "usage: $0 [--check]" >&2; exit 2; }

status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 3 "$CONTROL_READY_URL" 2>/dev/null) || status=000
[[ $status == 200 ]] || { echo "stable Control API is not ready at $CONTROL_READY_URL (HTTP $status)" >&2; exit 1; }

files=("$API_ENV" "$WORKER_ENV")
for file in "${files[@]}"; do
  [[ -f $file && ! -L $file ]] || { echo "required regular environment file is missing: $file" >&2; exit 1; }
  awk -F= '$1 == "ENGINE_BASE_URL" { found++ } END { exit found == 1 ? 0 : 1 }' "$file" \
    || { echo "$file must contain exactly one ENGINE_BASE_URL assignment" >&2; exit 1; }
done

if [[ $CHECK_ONLY == 1 ]]; then
  for file in "${files[@]}"; do
    awk -F= -v expected="$CONTROL_URL" '
      $1 == "ENGINE_BASE_URL" { ok = ($2 == expected) }
      END { exit ok ? 0 : 1 }
    ' "$file" || { echo "$file does not use the stable Control API origin" >&2; exit 1; }
  done
  echo "commerce API and worker use the ready stable Control API origin"
  exit 0
fi

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
backups=()
temps=()
committed=0
cleanup() {
  local index
  for index in "${!temps[@]}"; do
    [[ -z ${temps[$index]} ]] || rm -f -- "${temps[$index]}"
  done
  if [[ $committed == 0 && ${#backups[@]} -gt 0 ]]; then
    for index in "${!backups[@]}"; do
      [[ -e ${backups[$index]} ]] && cp -a "${backups[$index]}" "${files[$index]}"
    done
  fi
}
trap cleanup EXIT

for index in "${!files[@]}"; do
  file=${files[$index]}
  backup=$file.pre-control-proxy.$timestamp
  tmp=$(mktemp "$(dirname -- "$file")/.engine-control-env.XXXXXX")
  temps+=("$tmp")
  chmod 0600 "$tmp"
  cp -a "$file" "$backup"
  backups+=("$backup")
  awk -F= -v url="$CONTROL_URL" '
    $1 == "ENGINE_BASE_URL" { print "ENGINE_BASE_URL=" url; next }
    { print }
  ' "$file" >"$tmp"
  chown --reference="$file" "$tmp"
  chmod --reference="$file" "$tmp"
done

for index in "${!files[@]}"; do
  mv -f "${temps[$index]}" "${files[$index]}"
  temps[$index]=
done
committed=1

echo "configured stable Control API origin in ${files[*]}"
echo "root-only rollback copies use suffix .pre-control-proxy.$timestamp"

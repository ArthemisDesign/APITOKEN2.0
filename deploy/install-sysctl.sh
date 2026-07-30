#!/usr/bin/env bash
set -euo pipefail

SOURCE=/usr/local/lib/apitoken-watchdog/sysctl-apitoken-redis.conf
TARGET=/etc/sysctl.d/99-apitoken-redis.conf
SYSCTL=/usr/sbin/sysctl

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'run as root\n' >&2; exit 1; }
[[ -f $SOURCE && ! -L $SOURCE ]] || { printf 'fixed Redis sysctl definition is missing\n' >&2; exit 1; }
[[ $(stat -c '%u:%a' -- "$SOURCE") == 0:644 ]] \
  || { printf 'fixed Redis sysctl definition must be root-owned mode 0644\n' >&2; exit 1; }
[[ $(wc -l <"$SOURCE") -eq 1 ]] \
  && grep -Fxq 'vm.overcommit_memory = 1' "$SOURCE" \
  || { printf 'fixed Redis sysctl definition is invalid\n' >&2; exit 1; }
[[ -x $SYSCTL ]] || { printf 'sysctl is required\n' >&2; exit 1; }
[[ -d /etc/sysctl.d && ! -L /etc/sysctl.d && ! -L $TARGET ]] \
  || { printf 'Redis sysctl destination is unsafe\n' >&2; exit 1; }

install -o root -g root -m 0644 "$SOURCE" "$TARGET"
"$SYSCTL" --load "$TARGET"
[[ $("$SYSCTL" -n vm.overcommit_memory) == 1 ]] \
  || { printf 'vm.overcommit_memory did not converge to 1\n' >&2; exit 1; }

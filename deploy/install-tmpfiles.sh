#!/usr/bin/env bash
set -euo pipefail

SOURCE=/usr/local/lib/apitoken-watchdog/apitoken-tmpfiles.conf
TARGET=/etc/tmpfiles.d/apitoken.conf

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'run as root\n' >&2; exit 1; }
[[ -f $SOURCE && ! -L $SOURCE ]] || { printf 'fixed tmpfiles definition is missing\n' >&2; exit 1; }
[[ $(stat -c '%u:%a' -- "$SOURCE") == 0:644 ]] \
  || { printf 'fixed tmpfiles definition must be root-owned mode 0644\n' >&2; exit 1; }
[[ -d /etc/tmpfiles.d && ! -L /etc/tmpfiles.d && ! -L $TARGET ]] \
  || { printf 'tmpfiles destination is unsafe\n' >&2; exit 1; }

install -o root -g root -m 0644 "$SOURCE" "$TARGET"
systemd-tmpfiles --create "$TARGET"

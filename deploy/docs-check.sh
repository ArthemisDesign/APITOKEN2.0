#!/usr/bin/env bash
# docs-check.sh <base> <target> — проверяет, что diff base..target соблюдает «живой контракт»
# документации (AGENTS.md, docs/CHANGE_CHECKLISTS.md, docs/DEPENDENCIES.md).
#
# Блокирующе падает, когда diff трогает контрактные поверхности (packages/contracts, миграции,
# Control API-роуты движка, sales feed / internal-контроллеры, packages/payments,
# crates/metering) и при этом не изменён ни один markdown-документ. Для любого другого
# кодового diff без изменений документации печатает warning и проходит — напоминание, а не блок.
#
# Известное ограничение эвристики: «документация изменена» засчитывает ЛЮБОЙ *.md в diff, даже
# несвязанный. Gate — страховка от полного забвения документации, а не доказательство её
# достаточности; предметную полноту дают чеклисты из docs/CHANGE_CHECKLISTS.md.
#
# Вызывается из am_gate_static() в deploy/agent-merge.sh с уже разрешёнными полными SHA.
# Самостоятельный запуск (оба аргумента — полные 40-символьные SHA):
#   bash deploy/docs-check.sh "$(git rev-parse origin/master)" "$(git rev-parse HEAD)"
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -ne 2 ]]; then
  echo "docs-check: usage: docs-check.sh <base-sha> <target-sha>" >&2
  exit 2
fi
base=$1 target=$2
if [[ ! $base =~ ^[0-9a-f]{40}$ || ! $target =~ ^[0-9a-f]{40}$ ]]; then
  echo "docs-check: base and target must be full 40-character lowercase SHAs" >&2
  exit 2
fi

mapfile -t files < <(git -C "$ROOT" diff --name-only --no-renames \
  --diff-filter=ACDMRTUXB "$base..$target")

docs_changed=0
code_changed=0
contract_hits=()

for path in ${files[@]+"${files[@]}"}; do
  case "$path" in
    *.md)
      docs_changed=1
      ;;
  esac
  case "$path" in
    packages/contracts/*|\
    packages/payments/*|\
    crates/metering/*|\
    packages/db/migrations/*|packages/sales-db/migrations/*|\
    packages/openkeys-db/migrations/*|crates/registry/migrations_pg/*|\
    crates/server/src/http.rs|crates/server/src/admin.rs|\
    apps/api/src/sales-feed.controller.ts|apps/sales-api/src/internal.controller.ts)
      contract_hits+=("$path")
      code_changed=1
      ;;
    crates/*|apps/*|packages/*)
      code_changed=1
      ;;
  esac
done

if (( ${#contract_hits[@]} > 0 )) && (( docs_changed == 0 )); then
  {
    echo "docs-check: FAILED — diff меняет контрактные поверхности без обновления документации:"
    printf '  %s\n' ${contract_hits[@]+"${contract_hits[@]}"}
    echo "По «живому контракту» (AGENTS.md) документация обновляется В ТОМ ЖЕ коммите:"
    echo "  - пройди чеклист из docs/CHANGE_CHECKLISTS.md (Control API / sales feed / способ оплаты / миграция);"
    echo "  - обнови документ контракта (docs/engine/CONTROL_API.md, docs/sales/SALES_PORTAL.md, ...) и/или"
    echo "    строку связи в docs/DEPENDENCIES.md;"
    echo "  - если контракт по факту не изменился — всё равно отрази это в коммите и при необходимости"
    echo "    уточни документ."
  } >&2
  exit 1
fi

if (( code_changed == 1 )) && (( docs_changed == 0 )); then
  echo "docs-check: warning — кодовый diff без изменений документации; проверь по" >&2
  echo "docs/CHANGE_CHECKLISTS.md и docs/DEPENDENCIES.md, не требует ли изменение обновления доков." >&2
fi

exit 0

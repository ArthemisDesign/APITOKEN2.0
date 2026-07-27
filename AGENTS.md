# AGENTS.md — контракт для любого AI-агента в этом репозитории

Полные правила проекта — в `CLAUDE.md` (архитектура, слои, инварианты), `BRANCHES.md` (модель
веток) и `CONTRIBUTING.md` (delivery-конвейер). Прочитай их. Здесь — то, что нарушают чаще всего,
плюс проверенная карта и команды. Над репозиторием одновременно работают десятки агентов
(см. `git worktree list`), поэтому дисциплина изоляции и атрибуции — не формальность.

## Страховки нет — правила соблюдаешь сам

Хук `.claude/hooks/guard-git.sh` — это PreToolUse-hook Claude Code (см. `.claude/settings.json`).
В OpenCode и других агентах он НЕ исполняется: запрещённые git-команды ничто не блокирует,
и чужую работу можно уничтожить молча. Дисциплина ниже — на тебе.

## Ветка не изолирует — изолирует worktree

Рабочее дерево одно на каталог. `git checkout` в общем каталоге переписывает файлы под соседним
агентом и переносит его незакоммиченные правки на твою ветку. Именно так работа оказывается
приписана не тому автору.

```bash
git fetch origin
git worktree add ~/wt/<task> -b <type>/<task> origin/master
cd ~/wt/<task>          # дальше не покидаешь этот каталог
```

Проверь до первой правки: `git rev-parse --show-toplevel` — твой каталог, `git rev-parse
--abbrev-ref HEAD` — твоя ветка. Если это не так, остановись и спроси человека.

## Запрещённые команды

Без явной команды человека НИКОГДА: `git checkout <branch>`, `git switch`, `git stash`,
`git reset --hard`, `git clean -f`, `git merge`, `git rebase`, `git push` в чужую ветку или в
`master`, `git worktree remove`. Стейджи только свои пути: `git add crates/forward/...`.
`git add -A` и `git add .` запрещены.

## Что считать своей работой

Твоя работа — коммиты на твоей ветке, а не состояние дерева:

```bash
git diff --stat origin/master...HEAD    # только это идёт в отчёт
```

Увидел в `git status` чужие изменения — не откатывай, не чини, не объясняй их происхождение.
Одна строка «в дереве есть посторонние изменения», и продолжай свою задачу. Файл, прочитанный
давно, перечитай перед правкой: между чтением и записью его мог изменить другой агент. Никогда не
описывай содержимое файла по памяти из контекста.

## Карта репозитория (три bounded context'а)

- **Rust-движок** (Cargo workspace, `crates/*`): слои строго вниз
  `registry ← pool ← forward ← server` (бинарь `claude-api`). Рядом, со своими границами:
  `crates/metering` (тарификация, чистая математика, только `serde_json`) и `crates/authbot`
  (пополнение пула, стоит ВНЕ слоёв, перед реестром). В API-слоях env читается только в
  `crates/server/src/config.rs`; `registry`/`pool` — без сети и HTTP. У каждого крейта свой
  `crates/<name>/CLAUDE.md` — читай его до первой правки крейта.
- **Коммерция** (pnpm workspace): `apps/api` (NestJS), `apps/worker`, общие
  `packages/{contracts,db,engine-client,payments}`. К движку — ТОЛЬКО через HTTP Control API
  (`CONTROL_API.md`); engine PostgreSQL/SQLite коммерция не открывает. Карта и локальный запуск —
  `COMMERCIAL_BACKEND.md`.
- **Партнёрка**: `apps/sales-api`, `apps/sales-web`, `packages/sales-db` — своя БД `sales`;
  единственная граница с коммерцией — internal feed под ключом `SALES_CONTROL_KEY`. Описание —
  `SALES_PORTAL.md`.
- **`apps/web`** — фронт клиентов, деплоится на Vercel независимо от host-watchdog.
- CRM вынесена в отдельный репозиторий; роутинг `crm.apitoken.sale` в `deploy/Caddyfile` и юниты
  `systemd/apitoken-crm-*` остаются здесь — НЕ удалять (снесёт прод-роут CRM).
- Сквозной инвариант: деньги — только integer (`bigint` / nanoUSD-строки); float и JavaScript
  `number` для сумм запрещены везде.

## Проверка

```bash
cargo build                        # всегда зелёный до коммита
cargo test -p <crate>              # точечно; для metering/денег — ВСЕ тесты обязательны
cargo build && bash tests/rotation_fanout_smoke.sh   # smoke ротации без живых подписок (мок-апстрим)

pnpm build && pnpm typecheck && pnpm test            # коммерческий workspace
pnpm --filter @claude-api/<pkg> test                 # один пакет
# интеграционные тесты требуют PostgreSQL:
docker compose up -d commerce-postgres
TEST_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce pnpm test:integration
```

Полный gate перед мёржем (ровно то, что прогоняет `deploy/agent-merge.sh`):
`pnpm install --frozen-lockfile` → `pnpm build` → `pnpm typecheck` → `pnpm test` →
`cargo test --locked --workspace` → `bash -n deploy/*.sh deploy/apitoken-db-dump` →
`git diff --check`. Node 24 (`engines` уже задан, `.node-version` есть), pnpm 9.

## Миграции — только expand, двумя коммитами

- Пути: коммерция — `packages/db/migrations`, движок — `crates/registry/migrations_pg`, партнёрка —
  `packages/sales-db/migrations`. Существующую миграцию не редактировать, не переименовывать, не
  удалять (`packages/db/MIGRATIONS.md`).
- Зависимая схема идёт отдельным expand-only коммитом ПЕРВЫМ; код, который от неё зависит,
  мёржится только после зелёных `deploy/migration` и `deploy/watchdog` на миграционном SHA.
- Production-миграции и деплой выполняет только host-watchdog. Ничего не деплоить и не мигрировать
  по SSH вручную.

## Мёрж в master — одной командой

```bash
git push -u origin HEAD
./deploy/agent-merge.sh
```

Запуск — из своего worktree, без аргументов, с чистым деревом и настроенным upstream (из основного
клона скрипт откажет; `--allow-primary-tree` — только для человека). `master` — production trigger:
хост деплоит ровно один SHA за раз. Скрипт прогоняет полный gate, берёт машинный merge-lock,
ребейзит, перепроверяет gate на том самом SHA, который пушит, и держит lock до зелёного
`deploy/watchdog`. До gate и повторно под lock он сам читает именно `deploy/watchdog` через
GitHub API, переиспользуя credential из `git credential` (на macOS — Keychain), поэтому `gh` и
отдельный `GITHUB_TOKEN` не нужны. Pending/временную ошибку скрипт ждёт и перепроверяет сам. Агент
НИКОГДА не просит человека дать токен или доказать зелёный деплой: сломанный credential чинит
локально и перезапускает команду. Мёржить вслепую или вручную запрещено. Красный SHA не ретраить —
исправлять новым коммитом на новой ветке.

## Уборка после мёрджа

После того как `agent-merge.sh` завершился и `deploy/watchdog` зелёный на твоём SHA, агент обязан
удалить свой worktree и ветку:

```bash
cd <основной_каталог_репо>   # выйти из worktree перед удалением
git worktree remove ~/wt/<task>
git branch -D <type>/<task>
```

`git worktree remove` — единственное разрешённое использование этой команды, и только для своего
дерева, после подтверждённого зелёного деплоя. Чужие worktree не трогать.

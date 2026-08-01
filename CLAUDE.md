# CLAUDE.md — системные правила проекта claude-api

> Это обязательные инструкции для ЛЮБОГО агента, работающего в этом репозитории.
> Они важнее удобства и привычек: **соблюдай архитектуру и модель веток ниже всегда.**
> В каждом крейте есть свой вложенный `crates/<name>/CLAUDE.md` с локальными границами —
> Claude Code читает их автоматически, когда работает в подкаталоге.

## Язык ответа

ВСЕГДА отвечай на языке текущего запроса пользователя. Язык предыдущих сообщений не переопределяет
язык нового запроса. Если запрос смешивает несколько языков, используй преобладающий; переходи на
другой язык только по явной просьбе пользователя.

## Что это

Пул обычных Claude-подписок (Max/Pro) отдаётся по сети как **API, неотличимый от
`api.anthropic.com`**. Клиент наводит любой Anthropic-совместимый инструмент на наш сервер —
запрос уходит на квоте подписки из пула, с ротацией по лимитам. Полное описание — `README.md`,
карта модулей — `docs/engine/ARCHITECTURE.md`, модель веток — `BRANCHES.md`, production runbook —
`docs/ops/DEPLOYMENT.md`, authority/fencing Stage 2 — `docs/engine/STAGE2_POSTGRES_AUTHORITY.md`.
Обязательный workflow для contributor/AI и автоматическая доставка — `CONTRIBUTING.md`.

## CRM & Parsing — ВЫНЕСЕНО в отдельный репозиторий

Внутренняя AI-CRM и парсинг (`crm.apitoken.sale`) больше НЕ живут в этом монорепо — они
переехали в самостоятельный продукт **`github.com/Q666Q666Q/CRM-Parcing`** (пакеты `@crm/*`).
Здесь остаётся только ИНФРА-роутинг под общий прод-сервер: `crm.apitoken.sale` и общий
`managed_admin_auth` в `deploy/Caddyfile`, а также юниты `systemd/apitoken-crm-*.service`.
Human credentials и domain grants хранятся в commerce PostgreSQL и проверяются внутренним
`apps/api` auth endpoint; CRM ingest по-прежнему обходит human auth и проверяет свой ingest key.
Этот роутинг держим тут, потому что Caddy и watchdog на сервере централизованы в этом репозитории;
НЕ удалять (снесёт прод-роут CRM).
Код/доки/парсеры CRM правим в новом репозитории, деплой CRM — ручной (его `deploy/DEPLOY.md`),
вне watchdog монорепо. Аккаунт движка `crm-parsing` и ключ «CRM & Parsing» — общие (виден в панели).

## Commercial workspace (TypeScript, отдельный bounded context)

В этом же репозитории находится коммерческий pnpm-workspace: `apps/api`, `apps/worker` и общие
`packages/*`. Он отвечает за будущих пользователей, платежи, вебхуки и связь user→engine account.
Он **не входит** в Rust-цепочку `registry ← pool ← forward ← server` и не импортирует Rust-крейты.

- Коммерческий код не открывает engine PostgreSQL/SQLite и не пишет баланс напрямую.
- Единственная граница коммерция→движок — HTTP Control API из `docs/engine/CONTROL_API.md`.
- `apps/api` и `apps/worker` независимо деплоятся; общую логику кладём в `packages/*`.
- Коммерческая PostgreSQL хранит людей/платежи/доставку событий, но НЕ авторитетный live-баланс.
- CONTROL_KEY существует только в server-side env; браузеру, ответам и логам его не отдавать.
- Суммы провайдера и движка — только integer (`bigint`/decimal string), без JavaScript `number`.
- Пополнение — произвольное целое число USD, введённое пользователем строкой цифр. Каталога
  фиксированных продуктов нет; точки, дроби, float, знаки и ведущие нули запрещены.
- Browser API никогда не принимает `user_id` как доказательство личности. Владельца берём только
  из проверенной server-side сессии; приватные SQL-запросы дополнительно фильтруют по `user_id`.
- Полный клиентский `sk-pool-…` возвращается браузеру только при выпуске и не хранится в commerce
  PostgreSQL; листинг/отзыв используют маску и не-секретный engine `key_id`.
- Password users may receive a session without email verification only while
  `EMAIL_VERIFICATION_REQUIRED=false`; they never receive the welcome bonus. Google/GitHub require a
  provider-verified email, key identities on provider subject, and are the only bonus-eligible methods.
- Auth tokens are stored hashed. The email outbox may contain only AES-GCM-encrypted raw tokens;
  neither tokens nor verification/reset URLs may be logged.
- Публичный production API коммерческого слоя: `https://backend.apitoken.sale`; клиентский домен:
  `https://apitoken.sale`.
- B2C pricing derives only from idempotently consumed engine charge-ledger rows. Tier/month state and
  B2B invite/manual pricing live in commerce PostgreSQL; engine multiplier changes use durable jobs.

Локальная карта и запуск — `docs/commerce/COMMERCIAL_BACKEND.md`. Проверка: `pnpm build && pnpm typecheck && pnpm test`.

## Архитектура — слои (НЕ нарушать направление зависимостей)

```
registry  ←  pool  ←  forward  ←  server(bin)
```

| Крейт | Отвечает за | МОЖЕТ зависеть от | НЕ ДЕЛАЕТ |
|---|---|---|---|
| `crates/registry` | engine PostgreSQL authority + SQLite importer | postgres, rusqlite, anyhow | HTTP, env, логика пула |
| `crates/pool` | пул + ротация (in-memory) | registry | сеть, HTTP, БД, env |
| `crates/forward` | прозрачный форвардинг /v1/*, поллер лимитов | pool, registry, axum, reqwest | чтение env, CLI, управляющие роуты |
| `crates/server` | КОМПОЗИЦИЯ: env-конфиг, CLI, роутер, фоновые циклы | forward, pool, registry | бизнес-логику форвардинга (она в forward) |

**Пополнение пула — `crates/authbot` (ВНЕ слоёв API).** Отдельный Rust-компонент-ПРОИЗВОДИТЕЛЬ
доступа: Telegram-бот покупает Claude/ChatGPT/Gemini, пишет Claude-токены через
`registry::authority`, Codex-профили публикует отдельными `CODEX_HOME`, а проверенные Gemini
Code Assist OAuth subscriptions — AEAD-конвертами в атомарном roster. Он не участвует в слоях
`registry←…←server` и не импортирует `pool`/`forward`/`server`. Владелец-ветка `comp/authbot`,
локальные правила — `crates/authbot/CLAUDE.md`.

**Инварианты (проверяй перед коммитом):**
1. **Прозрачность.** Для клиента протокол = чистый Anthropic API (тело/ответ/стрим/ошибки).
   Единственное, что делаем под капотом — инжект Claude Code identity + oauth-заголовки
   (иначе токен подписки не пускают). Не ломать эту прозрачность.
2. **Направление зависимостей** строго по таблице. registry/pool — без сети и без HTTP.
3. **env читается ТОЛЬКО** в `crates/server/src/config.rs`. Ниже по стеку — принимают готовый
   конфиг (`forward::ProxyConfig`), а не лезут в окружение.
4. **Секреты не коммитим:** токены, `subscriptions.db`, прокси с паролями, `*.env`, `target/`
   (см. `.gitignore`). В коде и логах не печатать токены.
5. **Куда класть новый код** — по зоне ответственности из таблицы. Меняешь работу с БД →
   `registry`; выбор/ротацию → `pool`; транспорт форвардинга → `forward`; проводку/CLI/env →
   `server`. Если тянет добавить сеть в pool или env в forward — это сигнал, что слой выбран не тот.

## Модель веток (trunk + ветка-владелец на компонент)

Подробно — `BRANCHES.md`. Кратко:

- **`master`** — интеграция, ВСЕГДА собирается (`cargo build` зелёный). Прямые коммиты — только
  для кросс-компонентной проводки/доков/релиза.
- **`comp/registry`, `comp/pool`, `comp/forward`, `comp/server`** — долгоживущие ветки-владельцы.
  Работа над компонентом идёт в его ветке и трогает преимущественно свой крейт. На каждой — свой
  `BRANCH.md` (назначение, границы, как тестировать).
- Правило: **изменение компонента делай на его `comp/*`-ветке**, затем merge в `master`.
  Кросс-компонентную задачу разбивай по владельцам или веди на `master` с явным обоснованием.
- Перед началом: `git branch` — пойми, где ты. Ветка сама себя объясняет через `BRANCH.md`.

## Как собрать/проверить

```bash
cargo build                      # весь workspace (должен быть зелёным до коммита)
cargo build -p forward           # отдельный крейт
cargo run -p claude-api -- serve # поднять сервер (env см. crates/server/src/config.rs)
cargo run -p claude-api -- sub list
```

Smoke без живых подписок — мок-апстрим (`CLAUDE_API_UPSTREAM=http://127.0.0.1:PORT`): проверяет
форвардинг, инжект identity, ротацию при 429, стрим. Пример — в истории/`README.md`.

## Жизненный цикл агента: изоляция → работа → мёрж

Над репозиторием одновременно работает несколько агентов и людей. **Ветка не изолирует.**
Рабочее дерево одно на каталог: `git checkout` в общем каталоге переносит чужие незакоммиченные
правки на твою ветку и переписывает файлы под соседом. Изолирует только отдельный worktree.
Правила ниже продублированы в `AGENTS.md` и принудительно проверяются хуком
`.claude/hooks/guard-git.sh`.

### 1. Старт — свой worktree

Первым делом убедись, что ты в собственном каталоге и на собственной ветке:

```bash
git rev-parse --show-toplevel      # твой каталог, НЕ основной клон
git rev-parse --abbrev-ref HEAD    # твоя ветка, НЕ master
```

Если worktree ещё не создан — создай и больше не покидай его:

```bash
git fetch origin
git worktree add ~/wt/<task> -b <type>/<task> origin/master
cd ~/wt/<task>
```

### 2. Работа — запрещённые команды

Без явной команды человека НИКОГДА: `git checkout <branch>`, `git switch`, `git stash`,
`git reset --hard`, `git clean -f`, `git merge`, `git rebase`, `git push` в чужую ветку или в
`master`, `git worktree remove`. Кажется, что нужна одна из них — остановись и спроси.
Стейджим только свои пути (`git add crates/forward/...`); `git add -A` и `git add .` запрещены —
они утаскивают файлы соседа в твой коммит.

### 3. Атрибуция — что считать своей работой

Твоя работа — это коммиты на твоей ветке, а НЕ состояние дерева:

```bash
git diff --stat origin/master...HEAD    # только это идёт в отчёт
```

Увидел в `git status` изменения, которых не делал: не откатывай, не чини, не объясняй их
происхождение. Одна строка «в дереве есть посторонние изменения» — и продолжай свою задачу.
Файл, прочитанный давно, перечитай перед правкой; не описывай содержимое по памяти из контекста.

### 4. Проверка и документация

Меняй код в границах крейта, держи `cargo build` зелёным, коммить по одному логическому
изменению. Обнови документацию, если поменялись границы/поведение (`crates/<x>/CLAUDE.md`,
`docs/engine/ARCHITECTURE.md`). Если нужна commerce-миграция — сначала отдельный expand-only коммит, и только
после зелёных `deploy/migration` и `deploy/watchdog` зависимый application-код. Существующую
миграцию не менять и не удалять (`packages/db/MIGRATIONS.md`).

### 5. Комментарий к каждому коммиту — обязателен

КАЖДЫЙ созданный агентом коммит должен иметь содержательное сообщение как на странице коммита в
GitHub: короткий Conventional Commit-заголовок (`type(scope): result`), пустую строку и подробный
body. Однострочный `git commit -m "..."` без body запрещён.

Body должен объяснять:

- какую проблему или ручную работу устраняет изменение и почему оно понадобилось;
- что именно теперь делает код или документация, включая важные ограничения и страховки;
- какие проверки выполнены. Нельзя заявлять о проверках, которые не запускались.

Сообщение описывает изменение и его последствия, а не инструмент, модель или агента, который его
сделал. Не добавляй AI/модель в заголовок, body, `Co-Authored-By` или другие trailers.

### 6. Мёрж в master — одной командой

```bash
git push -u origin HEAD
./deploy/agent-merge.sh          # человек в обычном клоне: --allow-primary-tree
```

Скрипт сначала отклоняет красный `master`, ребейзит ветку на его последний закоммиченный SHA и
повторно под lock проверяет именно `deploy/watchdog`, переиспользуя credential, которым `git` уже
пушит (на macOS — Keychain); отдельные `gh` и `GITHUB_TOKEN` не нужны. Pending target разрешает
спекулятивно начать проверки, но push ждёт его зелёного verdict под lock; временно недоступный status
скрипт перепроверяет сам каждые пять секунд. Агент НИКОГДА не просит человека дать токен или
доказать зелёный деплой: чинит локальный credential helper и перезапускает команду. Скрипт
параллельно прогоняет fail-closed локальный path-aware gate и trusted validation точного
ребейзнутого и запушенного SHA на production-хосте, затем берёт машинный merge-lock. Shell и
whitespace проверки выполняются всегда; TypeScript, Rust и deployment lanes выбираются по diff,
а неизвестный путь или изменение механизма gate включает их все. TypeScript-проверка обычного diff
ограничена изменёнными workspace-пакетами, их потребителями и внутренними зависимостями; shared
inputs и удаления проверяют весь workspace, а `.next/cache` переиспользуется между worktree и
host-кандидатами. Полные runtime-артефакты каждого TypeScript-контекста также переиспользуются по
content-addressed ключу точных tracked inputs, platform/toolchain и build environment; повреждение
кэша означает обычный rebuild. Миграции трёх disposable TypeScript-БД идут параллельно в своих
test lanes, а package `pretest` не пересобирает уже проверенные candidate-артефакты. Оба результата переиспользуются
только если SHA не изменился; иначе feature-ветка обновляется и оба gate повторяются для нового
SHA. После push хост переиспользует тот же замороженный кандидат без
повторного теста; commerce lane заранее собирает и хэширует минимальный production-bundle с одним
pnpm virtual store и standalone Content Studio, поэтому release promotion reflink-копирует только
его, а не весь candidate; no-change GitHub-статусы публикуются параллельно, а финальные production-smoke
выбираются только для реально затронутых surfaces и независимые проверки выполняются одновременно.
Root-установка operational-изменений тоже ограничена точным составным scope: controller, Caddy,
systemd и monitoring не запускают несвязанный полный bootstrap; только systemd требует следующего
пятисекундного poll для нового sandbox.
Скрипт держит lock до
зелёного `deploy/watchdog`. Мёржить в `master` вслепую или вручную запрещено: `master` — production
trigger, и watchdog деплоит ровно один SHA за раз.

Красный SHA не ретраить — исправлять новым коммитом на новой ветке. Не запускай deploy или
production-миграцию через SSH. Полный workflow — `CONTRIBUTING.md`.

# CLAUDE.md — системные правила проекта claude-api

> Это обязательные инструкции для ЛЮБОГО агента, работающего в этом репозитории.
> Они важнее удобства и привычек: **соблюдай архитектуру и модель веток ниже всегда.**
> У основных крейтов (`registry`, `pool`, `forward`, `server`, `metering`, `authbot`, `router`)
> есть свой вложенный `crates/<name>/CLAUDE.md` с локальными границами —
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
2. **Направление зависимостей** строго по таблице. pool — без сети и без HTTP.
   registry — без HTTP и внешней сети, но владеет PostgreSQL-подключениями движка
   (authority Stage 2): DB-I/O внутри registry — норма, а не нарушение.
3. **env читается ТОЛЬКО** в `crates/server/src/config.rs`. Ниже по стеку — принимают готовый
   конфиг (`forward::ProxyConfig`), а не лезут в окружение.
4. **Секреты не коммитим:** токены, `subscriptions.db`, прокси с паролями, `*.env`, `target/`
   (см. `.gitignore`). В коде и логах не печатать токены.
5. **Куда класть новый код** — по зоне ответственности из таблицы. Меняешь работу с БД →
   `registry`; выбор/ротацию → `pool`; транспорт форвардинга → `forward`; проводку/CLI/env →
   `server`. Если тянет добавить сеть в pool или env в forward — это сигнал, что слой выбран не тот.

## Модель веток (trunk + ветка-владелец на компонент)

Подробно — `BRANCHES.md`. Кратко:

- **`master`** — интеграция и production trigger, ВСЕГДА собирается (`cargo build` зелёный).
  Изменения попадают в него только через `deploy/agent-merge.sh`; прямые коммиты запрещены.
- **`comp/registry`, `comp/pool`, `comp/forward`, `comp/server`, `comp/authbot`** — долгоживущие
  ветки-владельцы. На каждой — свой `BRANCH.md` (назначение, границы, как тестировать).
- Правило: повседневная работа агента — **task-ветка от `origin/master`** в отдельном worktree
  и мёрж через `deploy/agent-merge.sh` (канон процесса — `AGENTS.md`). `comp/*` остаются
  ветками-владельцами для накопительной работы; их синхронизация с `master` — отдельная
  операция вне типового цикла агента. Кросс-компонентную задачу разбивай по владельцам
  последовательными мёржами или веди одной task-веткой с явным обоснованием в коммите.
- Перед началом: `git branch` — пойми, где ты. Ветка сама себя объясняет через `BRANCH.md`.

## Как собрать/проверить

```bash
cargo build                      # весь workspace (должен быть зелёным до коммита)
cargo build -p forward           # отдельный крейт
cargo run -p claude-api -- serve # поднять сервер (env см. crates/server/src/config.rs)
cargo run -p claude-api -- sub list
```

Smoke без живых подписок — мок-апстрим (`CLAUDE_API_UPSTREAM=http://127.0.0.1:PORT`): проверяет
форвардинг, инжект identity, ротацию при 429, стрим. Готовые сценарии —
`tests/rotation_fanout_smoke.sh` и `tests/universal_chat_smoke.sh`.

## Жизненный цикл агента: изоляция → работа → мёрж

Канон процесса — корневой `AGENTS.md`: worktree-изоляция, запрещённые команды, атрибуция,
обязательные сообщения коммитов, «живой контракт» документации и чеклисты кросс-функциональных
изменений (`docs/CHANGE_CHECKLISTS.md`), карта связей (`docs/DEPENDENCIES.md`), expand-only
миграции и контракты, мёрж одной командой, синхронизация master и уборка. Он обязателен
полностью и здесь не дублируется — две версии процесса неизбежно разъезжаются. Краткая суть:
создавай worktree только через `deploy/agent-worktree.sh create`, работай в нём от
`origin/master`, держи `cargo build` зелёным и обновляй документацию в том же коммите; мёрж —
только `git push -u origin HEAD` + `./deploy/agent-merge.sh`; после зелёного `deploy/watchdog` —
`deploy/agent-worktree.sh finish` для своего дерева. `doctor` и dry-run `gc` диагностируют хвосты,
а глобальный `gc --apply` остаётся операторской maintenance-командой. Дисциплину частично страхует
хук `.claude/hooks/guard-git.sh` (только в Claude Code). Внутреннее устройство gate, lifecycle и
кэшей — `deploy/README.md`; workflow контрибьютора — `CONTRIBUTING.md`.

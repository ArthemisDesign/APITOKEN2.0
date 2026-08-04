# Архитектура claude-api

Пул Claude-подписок как **прозрачный `/v1` API**. Один Cargo workspace, слоёная структура —
каждый слой знает только о нижних. Правила для агентов — `CLAUDE.md` (корневой и в каждом крейте).

## Поток запроса

```
Клиент (Anthropic SDK / curl)  ──POST /v1/messages (наш api-key)──►  claude-api
                                                                        │
  server::http (роутер) ── авторизует клиента, отдаёт fallback ──►  forward::forward
                                                                        │
  forward: автоматически выводит cache-lineage (native session header   │
          или канонические префиксы истории), L1→Redis affinity;        │
          pool::route_affinity → пин / placement / wait / spill; ретраи →│
          pool::pick (наименее загруженная),                            │
          Bearer подписки + её прокси                                   ▼
                                                            api.anthropic.com
                                                                        │
  ответ (в т.ч. SSE) ◄──────────── байт-в-байт ──────────────────────  │
  при 429/5xx: pool::mark_cooling + следующая подписка (до начала стрима)

  metered POST /v1/messages: только после terminal всей local pre-byte
  ротации/smooth-wait ── один default-off attempt ──► ClaudeStore API3
  (без OAuth/persona headers; тот же reserve + exact usage settlement)

  GPT /v1/responses|chat|skin ──► local Codex home rotation/retry ──► ChatGPT backend
                                  │ terminal до model output, gpt-5.5/5.4 only
                                  └─ один separate-key default-off /v1/responses
                                                           ──► ClaudeStore API3
```

## Слои (направление зависимостей — только вниз)

```
┌────────────────────────────────────────────────────────────┐
│ server (bin claude-api)  — КОМПОЗИЦИЯ                       │
│   config(env→ProxyConfig) · http(роутер) · poller · main    │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ forward  — Claude + Codex adapter + native Gemini gateway   │
│   AffinityStore · Clients · Codex native pool · Gemini pool │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ pool  — пул + ротация (in-memory)                          │
│   Pool · Live · route · pick · place_best · mark_* · …     │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ registry  — engine-owned PostgreSQL authority              │
│   reservations/outbox · capacity leases · epochs · CRUD    │
└────────────────────────────────────────────────────────────┘
```

## Зоны ответственности (куда класть код)

| Меняешь… | Крейт | Ветка-владелец |
|---|---|---|
| хранение/чтение подписок, схему БД | `registry` | `comp/registry` |
| выбор подписки, ротацию, cooling, состояние лимитов | `pool` | `comp/pool` |
| форвардинг, инжект identity, поллер, стрим | `forward` | `comp/forward` |
| env-конфиг, CLI, роутер, фоновые циклы, проводку | `server` | `comp/server` |
| покупку подписок и пополнение пула (Telegram-бот) | `crates/authbot` | `comp/authbot` |

**Пополнение пула (вне слоёв API).** `crates/authbot` — Rust Telegram-бот: покупает Claude,
ChatGPT и Gemini-доступ, записывает Claude-токены через `registry::authority`, завершённые Codex
device flows атомарно публикует как отдельные `CODEX_HOME`, а проверенные платные Antigravity OAuth
subscriptions — как AEAD-encrypted profiles. Стоит ПЕРЕД `registry` как производитель и не импортирует
`pool`, `forward` или `server`.

## Ключевые решения

- **Claude: форвардинг, а не CLI.** Прокси шлёт сырой HTTP на api.anthropic.com на OAuth-токене
  подписки — поэтому Claude-ответ идёт байт-в-байт, в отличие от CLI-обёртки.
- **Codex: отдельная строгая граница.** Опциональные `/v1/responses`, `/v1/chat/completions` и
  OpenAI model-discovery на `openai.api.apitoken.sale` обслуживает native HTTPS-пул sealed
  ChatGPT OAuth-профилей (как у Gemini); это совместимый текстовый subset, а не прозрачный
  OpenAI Platform forwarding.
  `api.apitoken.sale` остаётся исключительно Claude-плоскостью: auth-заголовки провайдера не
  выбирают. Anthropic работает в blue-green `claude-api-anthropic@8787/8788`, OpenAI — в
  `claude-api-openai@8793/8797`, а native Gemini — в active/passive
  `claude-api-gemini@8795/8799` через `gemini.api.apitoken.sale`. Backend-only KIMI plane —
  четвёртая fixed-плоскость: active/passive слоты `claude-api-kimi@8804/8805` за стабильным
  loopback origin `127.0.0.1:8803` (singleton `claude-api-kimi` на 8804 — только rollback/anchor),
  без публичного vhost, router namespace и каталога; плоскость включена argv-пином
  `CLAUDE_API_KIMI_ENABLED=1` в reviewed юнитах (выключение — обратное reviewed изменение). Все используют один fenced
  PostgreSQL billing authority, но не общий
  HTTP process, router, credential pool или health state. Gemini profiles — отдельные encrypted
  Google OAuth identities с Cloud Code project, собственным proxy/refresh/cooling; private
  wrapper и identity никогда не выходят на публичную границу. Codex-патч удаляет локальные
  instructions/tools/context, оставляя только явный
  клиентский контекст. Transport не читает auth store, требует ChatGPT account type, attests binary
  SHA/version и не меняет Claude path. Один pre-provisioned process-wide lock под root-owned
  `/run/apitoken` ограждает весь набор homes: два процесса не могут разделить пул между собой, а
  rename/замена отдельного `CODEX_HOME` не создаёт второй lock inode.
- **Identity-инжект** — цена работы на подписочном токене; вынесен в конфиг, тюнится без пересборки.
- **Ротация до стрима** — статус ответа проверяется до отдачи тела, поэтому переключение подписок
  при 429/5xx не рвёт клиентский стрим.
- **ClaudeStore — не новый provider plane.** Это два compile-pinned default-off аварийных transport
  с разными ключами. Claude transport выполняет один metered `/v1/messages` после terminal local
  rotation/smooth-wait и не отправляет local OAuth, identity/billing block, persona, proxy или
  subscription identity. GPT transport аналогично допускает один `/v1/responses` после normal Codex
  rotation/retry, только для `gpt-5.5`/`gpt-5.4`; публичный id заменяет private local slug, а
  `chatgpt-account-id`, originator, OAuth, proxy и calibration identity наружу не выходят. Оба
  используют исходный customer reserve и authoritative terminal usage, не меняя local pool
  spend/quota/calibration/affinity. GPT требует отдельный key на ClaudeStore Codex tier и остаётся
  blocked до authenticated live gate. Полный контракт —
  [`CLAUDESTORE_FALLBACK.md`](CLAUDESTORE_FALLBACK.md).
- **Client dispatch без concurrency wait/reject.** Claude, Codex и Gemini принимают любой fan-out и
  сразу запускают независимые upstream attempts: process/per-account/per-profile request semaphore
  отсутствует. In-flight — только routing/observability signal и durable lifecycle accounting, не
  admission cap. Реальный provider quota/cooling остаётся отдельным честным `429 + Retry-After`.
- **env только в server** — нижние слои чисто-функциональны и тестируемы без окружения.
- **Redis — только shared cache-affinity.** Никакого client-supplied session ID: native harness ID
  используется автоматически, обычный API связывается rolling-хэшами канонических префиксов истории.
  Большой/явно cache-controlled общий system/tools root может подсказать тёплый дом новой conversation,
  после чего она сразу получает отдельный lineage и не связывает rebind разных диалогов.
  Ключи и значения — keyed BLAKE3 digests (без prompt/API key/account/subscription ID). Local L1
  остаётся всегда; таймаут/отказ/eviction Redis fail-open и влияет только на prompt-cache hit rate.
  Affinity живёт в СВОЁМ Redis-инстансе (`CLAUDE_API_AFFINITY_REDIS_URL`, 6380), отдельно от Codex
  response history (`CLAUDE_API_REDIS_URL`, 6379). `maxmemory` и `maxmemory-policy` в Redis задаются
  на инстанс, поэтому общий инстанс не давал им независимых бюджетов: крупные разговоры вытесняли
  affinity, а churn affinity удалял оплаченные разговоры. Потеря affinity безопасна по построению,
  потеря history — нет, поэтому переехал именно affinity.
- **PostgreSQL — durable authority.** Generated request IDs own exact reservation rows. Settlement
  first lands in a durable outbox, then atomically closes that exact reservation, updates the account,
  and inserts a charge unique on `(kind, request_id)`. SQLite is retained only as the guarded import
  source and rollback-era audit snapshot.
- **Fencing, not distributed hope.** Every engine process holds a monotonic PostgreSQL owner epoch;
  stale epochs cannot reserve money, persist pool state, or acquire capacity. Subscription admission
  is one transaction (cooldown/utilization validation + durable lease/inflight increment); tracked
  in-flight не ограничивает параллельность. Polling uses one
  PostgreSQL lease-epoch leader; there is no Redlock path.
- **Proven overlap gate.** Real-PostgreSQL fault injection and a two-owner end-to-end test gate the
  blue/green path. PostgreSQL mode may overlap two engine slots because money, delivery, capacity,
  pool writes, and poller leadership are fenced. SQLite fallback still takes the OS singleton lock.

Полная схема request lifecycle, fencing, cutover и операционные инварианты описаны в
[`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`](STAGE2_POSTGRES_AUTHORITY.md). Production runbook —
[`docs/ops/DEPLOYMENT.md`](../ops/DEPLOYMENT.md).

Граница совместимости, sealed roster, refresh-дисциплина, авторизация и rollback Codex-провайдера
описаны отдельно в [`docs/engine/CODEX_PROVIDER.md`](CODEX_PROVIDER.md).

Детали конфигурации — `config.env.example` / `server.env.example`. Деплой — единый provider cohort:
`systemd/claude-api-anthropic@.service`, `systemd/claude-api-openai@.service`,
`systemd/claude-api-gemini@.service` и `deploy/engine-bluegreen.sh` (legacy singleton units остаются
только для rollback на выпуски до соответствующего blue-green marker).

## Коммерческий контур (отдельно от движка)

```text
future Next.js web → apps/api → whole-USD checkout_sessions → commerce PostgreSQL
                           └── Control API → Rust claude-api
payment provider → apps/api (verified webhook) → engine_credits outbox → apps/worker → Control API
engine charge ledger → apps/worker cursor → funding/referral attribution ───────────┘
```

`apps/api` владеет будущей browser-facing API-границей и приёмом подписанных вебхуков.
Пользователь вводит произвольное целое число USD строкой; каталог продуктов отсутствует.
Browser identity определяется только opaque server-side сессией; email/Google identities и
сессии живут в commerce PostgreSQL, подробности — `docs/commerce/AUTHENTICATION.md`.
`apps/worker` забирает durable credit jobs из PostgreSQL через `FOR UPDATE SKIP LOCKED` и
идемпотентно вызывает `/admin/account/{id}/credit`. Общие схемы/репозитории/клиент движка находятся
в `packages/contracts`, `packages/db`, `packages/engine-client`. Коммерческие приложения не получают
engine DSN и не имеют прямого DB-кода; они общаются с движком только через Control API. Полная карта —
`docs/commerce/COMMERCIAL_BACKEND.md`.
Dashboard routes read authoritative balances, ledger rows and per-key spend through the Control API.
Key creation returns the usable secret once; later revocation uses a stable non-secret engine `key_id`.
B2C/B2B pricing state lives in commerce PostgreSQL; the worker synchronizes immutable policy and
release data through durable jobs. Target B2C is global 50% with provider/model overrides; tiers and
retention are removed. Full rules are in `docs/commerce/PRICING.md`.

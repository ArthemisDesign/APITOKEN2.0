# DEPENDENCIES.md — карта связей проекта

Единая карта всех связей между bounded context'ами и компонентами: кто производит, по какому
контракту, кто потребляет. Когда меняешь что-то на границе — сначала найди здесь строку связи,
потом действуй по `docs/CHANGE_CHECKLISTS.md` и протоколу контрактов из корневого `AGENTS.md`.

**Правило поддержки (часть «живого контракта»):** новая кросс-контекстная связь, новый
потребитель существующей связи или новый домен/сервис = новая строка в этом файле В ТОМ ЖЕ
коммите + строка в индексе `docs/README.md` для новых документов контрактов. Изменил контракт —
обновил и строку карты, и документ контракта. Строка, не соответствующая коду, — дефект уровня
бага; если связь исчезла — строку удаляют, а не оставляют «для истории».

## 1. Контракты между контекстами

Формат: производитель → контракт/канал → потребители. Документ контракта — место, где связь
описана предметно.

### Engine Control API (движок → коммерция, OpenKeys)

| Производитель | Контракт / канал | Потребители | Документ контракта |
|---|---|---|---|
| `crates/server` (`src/http.rs`, `src/admin.rs`) | HTTP `/admin/*` под `x-api-key: CLAUDE_API_CONTROL_KEY`; роуты только в режимах Combined/Anthropic | `packages/engine-client` — единственный клиент; прямые обращения к `/admin/*` вне него запрещены | `docs/engine/CONTROL_API.md` |
| `packages/engine-client` | TS-клиент `EngineClient`, zod-валидация из `@claude-api/contracts`, деньги — `json-bigint` строками; pricing v2 cursor/prepare/readback и canonical pure OpenKeys policy builder | `apps/api`, `apps/worker`, `apps/openkeys` и `packages/db` Stage 5 v2 materializer (env `ENGINE_BASE_URL` + `ENGINE_CONTROL_KEY` только у runtime-потребителей) | `docs/engine/CONTROL_API.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE7.md` |
| `claude-api db stage8-evidence` (`crates/registry`, `crates/server`) | protected schema-v2 JSON artifact with signed-i64 nanoUSD and canonical Rust `sha256:v2` evidence digest; exact target/recovery, full engine inventory/funding/shadow/runtime floor | `packages/db` Stage 8 consumer reads the explicit file path, verifies shape/digest/age and combines it with commerce/service plus two exhaustive OpenKeys scans; connected only after GREEN producer SHA | `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI-DISCOUNT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md` |
| `crates/server` operator-роуты | read-only `/overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health` (→ 8790), `/codex-subs` (→ 8792), `/gemini-subs` (→ 8794) через Caddy `admin.apitoken.sale`, ключ подставляет прокси (`ADMIN_CONTROL_KEY`); Claude `/capacity`, `/overview` supply и Prometheus используют одну exact turn+quota authority, не pool prior/EMA | `apps/admin` (без engine-client и без своих секретов); `/metrics` также скрейпит Prometheus напрямую по loopback, минуя Caddy (`observability/prometheus/prometheus.yml`) | `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md` |

Группы эндпоинтов Control API: аккаунты, credit/ledger (идемпотентный credit по
provider-qualified `ref`, cursor-протокол `ledger` + `ledger/ack`), usage, ключи, versioned pricing
(catalog/switches/policy), и PostgreSQL-only release-v2 prepare/read под `/admin/pricing/v2/*`.
Release-v2 producer публикует immutable policy/release/recovery prepare, полный engine inventory,
nullable head и account-local funding normalization plan/apply; activation mutation намеренно пока
отсутствует. Funding apply сериализуется с money writers и не требует global drain. После зелёного
exact producer SHA `packages/contracts` валидирует strict release/funding wire shape, а
`packages/engine-client` является единственным typed transport consumer. `apps/worker` через
`packages/db/src/funding-normalization-jobs.ts` реализует отдельный bounded/resumable Stage 6
application consumer: exhaustive cursor scans, exact service exclusion, fresh GET перед каждым
account-local POST, full-coverage parent confirmation, одинаковое target/recovery funding evidence
и prepare+readback обоих releases/recovery link. Job staging/status привязан к exact Stage 5 plan
digest и доступен через DB package entrypoint только будущему защищённому production control-plane;
CLI не является разрешением на ручной SSH. Наличие transport-методов или runner без явно staged
job не запускает backfill, не создаёт Stage 8 evidence и не активирует release.

### Sales feed (коммерция ↔ партнёрка)

Двусторонний контур под одним ключом `SALES_CONTROL_KEY` (заголовок `x-api-key`).

| Производитель | Контракт / канал | Потребители | Документ контракта |
|---|---|---|---|
| `apps/api` (`src/sales-feed.controller.ts`, `/v1/internal/sales/*`) | GET-фиды `attributions` / `usage-events` / `topups` (курсоры `after_id`); target usage schema v2 несёт exact referred-B2C `paid_funded_nano` независимо от pricing mode; schema v1/`referral-discount` живут только на producer-first переходе | `apps/sales-api` (`sync.service.ts`, `commerce.service.ts`; `COMMERCE_BASE_URL`) | `docs/sales/SALES_PORTAL.md` |
| `apps/sales-api` (`src/internal.controller.ts`, `/v1/internal/*`) | POST `promo/redeem` сохраняется для credit/attribution; `partners/referral-discount` и discount-поля — legacy compatibility до удаления tier-linked персональной цены | `apps/api` (`promo.service.ts`, `auth.service.ts`; `SALES_API_URL`) | `docs/sales/SALES_PORTAL.md` |

Типы фида продублированы локальными zod-схемами на обеих сторонах; в `packages/contracts`
не вынесены. Любое изменение фида правит обе стороны — см. протокол контрактов в `AGENTS.md`.

### Прочие связи между контекстами

| Производитель | Контракт / канал | Потребители | Документ контракта |
|---|---|---|---|
| `packages/contracts` | zod-схемы engine/pricing/auth/checkout-контрактов, canonical models и catalog pins; target pricing — global B2C/provider/model rules, B2B, OpenKeys 1:1, service `meter_only`, pricing releases | `apps/api`, `apps/worker`, `apps/openkeys`, `packages/db`, `packages/engine-client`. НЕ импортируют: `apps/web`, `apps/sales-*`, `apps/admin` | `docs/commerce/MULTI-DISCOUNT.md` |
| `apps/api` (публичный API) | HTTPS `backend.apitoken.sale/v1/*`, cookie-сессия | `apps/web` (`src/lib/api.ts`, `NEXT_PUBLIC_BACKEND_URL`) | `docs/commerce/COMMERCIAL_BACKEND.md` |
| `apps/api` (админ API) | `/v1/admin/*` через Caddy-rewrite `admin.apitoken.sale/admin/*`, заголовок `x-admin-key`; включает per-service CAS producer `/service-account-inventory/*`, который сверяет полный engine inventory через typed `packages/engine-client`; тот же канал и ключ на `content-studio.apitoken.sale/v1/*` | `apps/admin`; `packages/db` Stage 5 v2 читает durable `service_account_inventory_v2` в одном commerce snapshot; `apps/content-studio` (`/v1/admin/content/*`) | `docs/product/ADMIN_PANEL.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md` |
| `apps/openkeys` (админ API) | `/api/internal/admin/*` через Caddy `admin.apitoken.sale/openkeys-admin/*`, заголовок `X-OpenKeys-Control-Key` | `apps/admin` | `docs/product/OPENKEYS.md` |
| `apps/openkeys` (pricing inventory producer) | loopback/internal GET `/api/internal/pricing/v2/inventory`, bounded cursor + full `sha256:v2` manifest под `X-OpenKeys-Control-Key`; без secrets/live money | `packages/db` Stage 5 materializer и Stage 8 combined-evidence consumer каждый исчерпывают cursor дважды и требуют один неизменный full-manifest digest; оба подключены только после GREEN producer SHA | `docs/product/OPENKEYS.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/ops/DEPLOYMENT.md` |
| `apps/sales-api` (публичный + админ API) | `partners.apitoken.sale/v1/*`; `/v1/admin` через Caddy `admin.apitoken.sale/partner-admin/*`, заголовок `x-sales-admin-key` | `apps/sales-web`; `apps/admin` | `docs/sales/SALES_PORTAL.md` |
| `packages/payments` | адаптеры провайдеров: Platega (дефолт) и Cryptomus — боевые; DigiSeller — зарегистрирован, но отключён для клиентов (нет точки входа, статус в документе); вебхуки `POST /v1/payments/{platega,cryptomus}/webhook` в `apps/api`; reconcile-поллинг в `apps/worker` | `apps/api`, `apps/worker` (единственные потребители) | `docs/commerce/PLATEGA_INTEGRATION.md`, `docs/commerce/CRYPTOMUS_INTEGRATION.md`, `docs/commerce/DIGISELLER_INTEGRATION.md` |

### Devbot (`apps/devbot`)

Dev-бот Telegram — потребитель сигналов observability и deploy-контура; ни в одну БД не ходит,
Control API движка использует только на чтение. Своя release-lane `deploy/devbot`
(`/opt/apitoken/devbot-releases`), loopback-порт `127.0.0.1:3800`.

| Производитель | Контракт / канал | Потребители | Документ контракта |
|---|---|---|---|
| Alertmanager (`observability/alertmanager/alertmanager.yml.template`) | webhook `POST http://127.0.0.1:3800/alerts/{DEVBOT_AM_SECRET}` — receiver `devbot-telegram`, route с `continue: true` рядом с email-деревом (expand-only); блок рендерится только при provisioned `DEVBOT_AM_SECRET` из `/etc/apitoken/devbot.env` | `apps/devbot` | `docs/ops/DEVBOT.md` |
| GitHub API | commit statuses `deploy/*`, deployments `production-*` (read-only PAT) | `apps/devbot` (поллер, 30–60 с) | `docs/ops/DEVBOT.md` |
| `crates/server` Control API | readonly/control GET (`/pool`, `/codex-subs`, `/gemini-subs`, `/settlement-health`, слоты `/ready`) | `apps/devbot` (команды бота) | `docs/engine/CONTROL_API.md` |
| journald | чтение журнала юнитов deploy-контура (префиксы `[watchdog]`, `[admin-deploy]` и т.п.) | `apps/devbot` (этап 3) | `docs/ops/DEVBOT.md` |
| `apps/devbot` | node-exporter textfile `devbot_heartbeat_timestamp_seconds` (`/var/lib/apitoken/monitoring/textfile/devbot.prom`, атомарно каждые 60 с) | Prometheus → алерт `DevBotHeartbeatMissing` | `docs/ops/MONITORING.md#devbotheartbeatmissing` |

## 2. Внутри движка (кратко)

Слои и инварианты — `CLAUDE.md` (таблица слоёв) и `docs/engine/ARCHITECTURE.md`. Здесь только
то, что нужно для обхода связей при изменениях:

- **`crates/metering` — authority цен движка.** Захардкоженные effective-dated таблицы в
  nanoUSD: `src/lib.rs` (Anthropic), `src/codex.rs` (OpenAI), `src/gemini.rs` (Gemini).
  Изменение цены/модели — ревьюимый коммит сюда. Потребители: `crates/forward` (основной),
  `crates/server` (типы/тарифные идентификаторы).
- `crates/registry/src/pricing/` — НЕ прайс-лист, а durable-идентичности multi-discount:
  каталоги/свитчи/политики, admission-снапшоты (`docs/commerce/MULTI-DISCOUNT.md`). Fixed provider
  IDs actual/shadow contract — `anthropic|openai|google`; `gemini` не является durable authority.
- `crates/forward/src/pricing*` — pricing-resolver и shadow-evaluation конвейер. Живой:
  resolver вызывается в admission-пути strict-политики (`proxy.rs`) и в Codex-биллинге
  (`codex/billing.rs`); atomic legacy snapshot producers находятся в Anthropic, Codex и Gemini
  billing paths, а shadow-runtime для всех трёх fixed planes стартует в проде (`crates/server`).
  НЕ читает БД и не считает стоимость токенов.
- **Provider data-plane (`crates/forward` → `crates/router`).** Плоскости производят
  native и universal HTTP-поверхности, router потребляет их только через stable loopback
  origins. В частности, `/v1/messages/count_tokens` доступен на всех трёх плоскостях и
  выбирается по `model`: Anthropic native, локальный reserve-grade подсчёт Codex или
  quota-free Gemini `:countTokens`. Router сохраняет universal body, поэтому плоскость снимает
  собственный namespaced-префикс до admission; canonical GPT Fast aliases нормализует Codex
  plane. На compatibility boundary router дополнительно принимает camelCase
  `serviceTier:"fast"|"priority"` только для исполняемой GPT-цепочки Chat/Responses, удаляет
  alias и передаёт плоскости canonical `service_tier:"priority"`; конфликтующие значения и
  non-GPT/surface misuse отклоняются до вызова плоскости.
  Codex Messages skin также принимает и снимает только bounded no-op `context_management`
  текущего Claude Code (`edits:[]` или exact `clear_thinking_20251015` + `keep:"all"`), а
  stateful/неизвестные формы оставляет fail-closed; exact ephemeral cache markers клиента
  снимает в пользу automatic Codex caching, не принимая extended retention; его GA
  `output_config.effort` и bounded
  json_schema `format` переводит в эквивалентные Responses controls, включая structured
  title request текущего Claude Code. Для
  harness без arbitrary body fields router принимает `x-apitoken-service-tier: fast|priority`
  только на исполняемой GPT-цепочке, превращает его в body `service_tier:"priority"` и снимает
  сам заголовок до вызова плоскости; Codex plane остаётся authority reserve/settlement/effective tier.
  Агрегированный каталог остаётся OpenAI-shaped, кроме аутентифицированного Codex-native
  `{models:[]}` overlay по harness identity. Контракт — `docs/engine/UNIFIED_ROUTER.md`.
- **Контракт `x-apitoken-execution-state` (плоскости → router, этап 6.1).** Производители —
  `crates/forward` (`proxy.rs`, `anthropic.rs`, `anthropic_responses.rs`, `codex/api.rs`,
  `codex/skin.rs`, `gemini/api.rs`, `gemini/chat.rs`, `gemini/responses.rs`,
  `gemini/skin.rs`): заголовок
  `not_started` на не-2xx отказах до границы started при гарантии refund/cancel reserve;
  universal adapters сохраняют только точный сигнал плоскости и снимают его с ошибок после
  2xx. Потребитель —
  `crates/router`: всегда снимает заголовок со всех публичных ответов и при явной
  off-by-default `models`-цепочке использует точный сигнал для следующей serial attempt;
  401/402/клиентские 4xx (кроме signed 429) не ретраятся. Второе разрешённое доказательство —
  TCP `ConnectionRefused`; timeout/generic connect/unsigned 5xx fail closed. Документ
  контракта — `docs/engine/ROUTING_FENCING.md` §3.
- **Контракт execution group (router → provider planes → registry, этап 6.3).** Производитель
  trusted identity — `crates/router`: одна CSPRNG UUIDv4 и attempts `1..N` только для explicit
  fallback chain. `deploy/Caddyfile` удаляет клиентские копии на всех provider/router vhost'ах;
  router повторно удаляет их перед собственным инжектом. `crates/forward` валидирует пару до
  reserve и передаёт её через `AsyncBilling`; `crates/registry` сохраняет identity и атомарно
  выбирает один nonzero settlement winner в общей PostgreSQL authority (SQLite parity для
  rollback/tests). Потребители winner-результата — money/funding settlement и
  `crates/server` `/metrics`; публичные API group identity не возвращают. Контракт —
  `docs/engine/ROUTING_FENCING.md` §4.
- **Контракт policy preflight (provider planes → router, фаза 6.4a).** Производитель — одинаковый
  `crates/server::router_policy` на каждом fixed runtime: authenticated loopback-only
  `POST /internal/router/policy/preflight` читает customer key и один coherent pricing bundle через
  `AsyncBilling`, применяет engine-owned resolver и возвращает только bounded ordered allow-list.
  Потребитель — `crates/router` (6.4b реализован): после preset/catalog/preferences, до attempt 1,
  с exact ordered-subset validation, sequential mixed-version origin failover и без кэша
  credential/policy и без импорта `forward`/`registry`. Публичные provider Caddy-vhost'ы не включают
  `/internal/*` в allowlist; stable origins 8790/8792/8794 доступны router'у по loopback. Контракт и
  mixed-version failure semantics — `docs/engine/ROUTING_FENCING.md` §5.1.
- **Контракт early auth preflight (provider planes → router).** Производитель — одинаковый
  `crates/server::router_auth` на каждой fixed runtime: loopback-only bodyless
  `POST /internal/router/auth/preflight` проверяет forwarding-admin/customer credential через тот
  же `authed`/`AsyncBilling` resolver, что live admission, и возвращает только закрытый success
  marker либо 401/503 без reserve, pricing/policy read и identity. Потребитель — `crates/router`:
  до materialization 32 MiB universal body последовательно перебирает fixed origins, принимает
  только exact schema-v1 success, считает 401 терминальным, а mixed-version/transport/5xx fail
  closed; fail-fast 64 MiB budget с шагом 1 MiB ограничивает raw-body residency без
  execution-очереди и сохраняет concurrency малых запросов.
  Контракт —
  `docs/engine/UNIFIED_ROUTER.md` §«Ранний auth и граница памяти request body».
- **Контракт catalog pricing (provider planes → router).** Производитель — одинаковый
  `crates/server::router_pricing` на каждой fixed runtime: authenticated loopback-only
  `POST /internal/router/catalog/pricing` разрешает customer/admin credential, читает только один
  coherent pricing bundle для strict account и проецирует audited `crates/metering` rates через
  effective payable multiplier в integer nanoUSD-per-million strings. Ответ не содержит key,
  account, balance, policy или rule identity и ничего не резервирует/списывает. Потребитель —
  `crates/router`: после отдельного producer-first GREEN SHA он валидирует version/unit/canonical
  integer strings и ordered subset, фильтрует недоступные модели, добавляет
  `data[].apitoken.pricing`, помечает ответ `private, no-store` и fail-closed возвращает 401/503.
  Credential-specific overlay не кэшируется и никогда не попадает в общий catalog TTL-cache.
  Публичные provider vhost'ы `/internal/*` не обслуживают.
- **Fallback telemetry (router/provider planes → Prometheus, фаза 6.4c).** `crates/router`
  производит unauthenticated loopback `/metrics` на 8798 с ровно 18
  `claude_router_fallback_total{from_namespace,to_namespace,reason}` series; публичный Caddy
  allowlist этот путь не пропускает. Каждая fixed `crates/server` plane через существующий
  authenticated `/metrics` производит три bounded
  `claude_api_execution_not_started_total{plane}` series, считая только exact response реально
  возвращённой plane. Потребитель — `observability/prometheus/prometheus.yml` и recording/alert
  rules; Alertmanager/operator используют runbooks `RouterMetricsDown`, `RouterFallbackRateHigh`,
  `RouterConnectionRefusedFallback`, а money-regression detectors остаются отдельными. Model,
  credential, account, group и request identity через эту связь не проходят. Контракт —
  `docs/engine/ROUTING_FENCING.md` §§5.3–6 и `docs/ops/MONITORING.md`.
- `crates/authbot` — производитель доступа вне слоёв; OAuth-callback на `127.0.0.1:8796`.

## 3. Модели и цены — где ещё зеркалятся

Authority — `crates/metering` (выше). Всё нижеописанное — зеркала, которые надо трогать
вместе с ним (полный обход — чеклист «Новая модель» / «Изменение цены» в
`docs/CHANGE_CHECKLISTS.md`):

- `packages/contracts` — `CURRENT_*_CANONICAL_MODELS`, catalog generations и pricing release
  schemas. Frozen dormant capability generation 3 сохраняет исходный main
  Anthropic/OpenAI/Gemini set. Immutable generation 4 исторически добавила
  `gemini-3-flash-preview` (`google` — internal engine provider id), но gate старого public wire дал
  404, поэтому generation 4 остаётся rejected/dormant и не может быть материализована или
  активирована; её digest не переписывается. Новая dormant private-wire implementation не меняет
  этот исторический контракт: публикация после exact-SHA live gate потребует следующую additive
  capability generation. OpenKeys target set остаётся явным Anthropic/OpenAI subset.
  `B2C_PRICING_TIERS` — cleanup target, не authority нового pricing.
- `apps/web/src/lib/models.ts` — захардкоженный SEO-каталог моделей с официальными ценами;
  шапка файла требует синхронизации с `crates/metering/src/{codex,gemini}.rs`.
- `apps/web/src/lib/pricing-tiers.ts` — legacy cleanup target; витрина должна читать/показывать
  global 50% и effective provider/model discount без tier ladder.
- `packages/engine-client/src/openkeys-policy.ts` — canonical OpenKeys policy identity/digest и
  сверка каталога с точной reviewed identity поколения 1 или 2
  (`CURRENT_PRODUCT_CATALOG_ENTRIES` / `MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES`);
  `apps/openkeys` и Stage 5 planner используют один builder (fail closed при расхождении).
- `apps/admin/src/app/sales/calculator/calculation.ts` — захардкоженный `PRODUCT_CATALOG`
  подписочных продуктов (nanoUSD, bigint).
- Политика включения моделей и расчёт скидки: `docs/commerce/MULTI-DISCOUNT.md` §§2–4;
  клиентский прайсинг — `docs/commerce/PRICING.md`.

## 4. Границы баз данных

| БД | Пакет | Открывают |
|---|---|---|
| engine PostgreSQL/SQLite | `crates/registry` | только движок; из TS — никто (только Control API) |
| commerce PostgreSQL (`DATABASE_URL`) | `packages/db` | только `apps/api`, `apps/worker` |
| sales (`SALES_DATABASE_URL`) | `packages/sales-db` | только `apps/sales-api` |
| OpenKeys | `packages/openkeys-db` | только `apps/openkeys` |

## 5. Инфраструктурные связи

### Caddy (`deploy/Caddyfile`) — домен → upstream

| Домен | Upstream |
|---|---|
| `api.apitoken.sale` | engine `127.0.0.1:8790` (blue-green слоты 8787/8788) |
| `openai.api.apitoken.sale` | OpenAI-runtime `:8792` (слоты 8793/8797) |
| `gemini.api.apitoken.sale` | Gemini-runtime `:8794` (слоты 8795/8799); `/oauth/callback` → authbot `:8796` |
| `router.apitoken.sale` | claude-router `:8798` |
| `backend.apitoken.sale` | commerce `apps/api` `:8791` (слоты 3000/3001) |
| `admin.apitoken.sale` | managed auth; data-роуты → engine 8790/8792/8794, `/admin/*` → commerce 8791, `/openkeys-admin/*` → 3410, `/partner-admin/*` → sales 3100; остальное → `apps/admin` `:3700` |
| `partners.apitoken.sale` | `/v1/*` → sales-api `:3100`; остальное → sales-web `:3200` |
| `openkeys.apitoken.sale` | `apps/openkeys` `:3410` |
| `content-studio.apitoken.sale` | `/v1/*` → commerce 8791; остальное → `apps/content-studio` `:3500` |
| `crm.apitoken.sale` | `/v1/ingest/*`, `/r/*` → crm-api `:3400` (без admin-auth); `/v1/*` и остальное → managed auth → crm-api 3400 / crm-web `:3300`. CRM живёт в отдельном репозитории — роутинг НЕ удалять |
| `monitoring.apitoken.sale` | Grafana `:3600`; `support.apitoken.sale` → Chatwoot `:3010` |
| `mail.apitoken.sale` (+`autodiscover.`, `autoconfig.`) | почтовый сервис `127.0.0.1:8080` |
| `sales.apitoken.sale` | 301-редирект на `partners.apitoken.sale` |
| `admin.partners.apitoken.sale` | managed auth; `/v1/*` → sales-api `:3100`; остальное → sales-web `:3200` |

### systemd (`systemd/`) — сервис → приложение

`claude-api-anthropic@` → Anthropic-слоты 8787/8788 (текущий юнит; `claude-api@` — legacy) ·
`claude-api-openai@` → 8793/8797 · `claude-api-gemini@` → 8795/8799 · `claude-router` → 8798 ·
`claude-authbot` → authbot ·
`apitoken-api[@]` → `apps/api` 3000/3001 · `apitoken-worker` → `apps/worker` ·
`apitoken-admin` → 3700 · `apitoken-content-studio` → 3500 · `apitoken-openkeys` → 3410 ·
`apitoken-devbot` → `apps/devbot` 3800 · `apitoken-sales-api` → 3100 · `apitoken-sales-web` → 3200 · `apitoken-crm-{api,web}` → внешняя
CRM (3400/3300, НЕ удалять) · плюс infra-юниты: postgres, affinity-redis, deploy-watchdog,
monitoring-collector, candidate-validator, backup, fingerprint · host-bootstrap:
`apitoken-{sudoers,sysctl,tmpfiles}-install`.

### Мониторинг — петля «метрика → алерт → runbook»

`observability/prometheus/rules/{application,operations}.yml` (~64 алерта) — у каждого
аннотация `runbook: 'docs/ops/MONITORING.md#<alert>'`, и секция `## <Alert>` обязана
существовать в `docs/ops/MONITORING.md`. Согласованность механически проверяет
`deploy/monitoring-config.test.sh`, который host прогоняет при валидации каждого
merge-кандидата (`deploy/watchdog.sh`), — проверка покрывает ВСЕ алерты обоих rules-файлов,
а не только закреплённые поимённо. Это образцовый пример
замкнутой связи «код ↔ документация» — новые связи оформлять по тому же принципу.

### Доставка

`deploy/agent-merge.sh` — единственный путь в `master`; path-aware gate (классификаторы в
`deploy/watchdog-lib.sh`), машинный merge-lock, зелёный `deploy/watchdog` на production-хосте.
Полное описание — `deploy/README.md`, `CONTRIBUTING.md`.

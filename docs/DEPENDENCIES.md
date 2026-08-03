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
| `crates/server` + `crates/forward` + `crates/registry` locked-OpenKeys producer | `POST /admin/pricing/policy/{account_id}/locked-openkeys-transition`: strict exact request, atomic immutable successor insert + binding CAS, only managed provider-level 1:1 rules, fixed `shadow + legacy_single + verified` target; generic replacement lock remains intact | after GREEN exact producer SHA: `packages/contracts` → `packages/engine-client` → protected commerce rollout worker; no direct caller and no consumer in the producer commit | `docs/engine/CONTROL_API.md`, `docs/commerce/MULTI-DISCOUNT.md` |
| `packages/engine-client` | TS-клиент `EngineClient`, strict zod-валидация из `@claude-api/contracts`, деньги — `json-bigint` строками; pricing v2 provisioning-context/cursor/prepare/readback и единый canonical Stage 5 policy/assignment digest builder | `apps/api`, `apps/worker`, `apps/openkeys`; `packages/db` Stage 5 materializer, Stage 8 collector и pre-delivery activation authority (env `ENGINE_BASE_URL` + `ENGINE_CONTROL_KEY` только у runtime-потребителей) | `docs/engine/CONTROL_API.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE7.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md`, `docs/product/OPENKEYS.md` |
| `claude-api db stage8-evidence` (`crates/registry`, `crates/server`) | protected schema-v2 JSON artifact with signed-i64 nanoUSD and canonical Rust `sha256:v2` evidence digest; exact target/recovery, full engine inventory/funding/shadow/runtime floor | parity/diagnostic non-production input for `packages/db`; production no longer uses an SSH/file handoff | `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI-DISCOUNT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md` |
| `crates/server` + `crates/forward` Stage 8 capture producer | protected `POST /admin/pricing/v2/stage8-evidence/capture`; strict explicit inputs, server-owned compile-fixed manifest, unwrapped schema-v2 report including `passed=false`; PostgreSQL bounded reader only | after GREEN exact producer SHA: strict `packages/contracts` → raw-text/`json-bigint` `packages/engine-client` → `apps/worker`; exact raw engine bytes are durable before `packages/db` combines commerce/service and two exhaustive OpenKeys scans | `docs/engine/CONTROL_API.md`, `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI-DISCOUNT_STAGE9.md` |
| `crates/server` operator-роуты | read-only `/overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health` (→ 8790), `/codex-subs` (→ 8792), `/gemini-subs` (→ 8794) через Caddy `admin.apitoken.sale`, ключ подставляет прокси (`ADMIN_CONTROL_KEY`); Claude `/capacity`, `/overview` supply и Prometheus используют одну exact turn+quota authority, не pool prior/EMA; last-known per-window display-state routable-idle/quota-cooling подписки сохраняется только до exact provider reset и не делает stale remaining продаваемым | `apps/admin` (без engine-client и без своих секретов); `/metrics` также скрейпит Prometheus напрямую по loopback, минуя Caddy (`observability/prometheus/prometheus.yml`) | `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md` |

Группы эндпоинтов Control API: аккаунты, credit/ledger (идемпотентный credit по
provider-qualified `ref`, cursor-протокол `ledger` + `ledger/ack`), usage, ключи, versioned pricing
(catalog/switches/policy, включая узкий atomic locked-OpenKeys transition), и PostgreSQL-only release-v2 prepare/read/activation под
`/admin/pricing/v2/*`.
Release-v2 producer публикует immutable policy/release/recovery prepare, полный engine inventory,
nullable head, account-local funding normalization plan/apply и append-only assignment extension
для exact active/recovery pair аккаунта, созданного после cutover. Read-only Stage 8 capture
возвращает тот же blocker-preserving report, что CLI, через bounded PostgreSQL reader и не stage'ит
collection/activation work. Read-only
`GET /admin/pricing/v2/provisioning-context` одним snapshot публикует exact head/audit/evidence,
active release lineage и только evidence-selected recovery; до cutover возвращает `null`, а при
расхождении authority fail closed. Это producer для независимого provisioning OpenKeys/service и
не требует доступа этих контекстов к commerce-local activation tables. Единственный activation producer
принимает fresh combined evidence, повторно проверяет engine inventory/funding/runtime owner epochs
и атомарно пишет audit + singleton head; cutover/recovery не обновляют accounts или money rows.
Funding apply сериализуется с money writers и не требует global drain. После зелёного
exact producer SHA `packages/contracts` валидирует strict release/funding wire shape, а
`packages/engine-client` является единственным typed transport consumer. `apps/worker` через
`packages/db/src/funding-normalization-jobs.ts` реализует отдельный bounded/resumable Stage 6
application consumer: exhaustive cursor scans, exact service exclusion, fresh GET перед каждым
account-local POST, exact full welcome revocation → paid-only current aggregate, fail-closed
partial/mismatched revocation, exact paid-only adoption активных legacy reservations без изменения
их pricing snapshot, fail-closed ambiguous welcome reserve, full-coverage parent confirmation, одинаковое target/recovery
funding evidence и prepare+readback обоих releases/recovery link. Job staging/status привязан к exact Stage 5 plan
digest. Production producer — AdminGuard-protected `apps/api` endpoints для Stage 5 dry-run /
materialize и Stage 6 status / stage: они требуют verified `x-admin-actor`, exact plan digest,
meaningful mutation reason, strict `packages/contracts` response и attributed transactional audit.
DB package CLI остаётся non-production diagnostic и не является разрешением на ручной SSH. Наличие
transport-методов или runner без явно staged job не запускает backfill, не создаёт Stage 8 evidence
и не активирует release.
После зелёного assignment-extension и provisioning-context producer SHA consumers используют только
цепочку strict `packages/contracts` → typed/canonical `packages/engine-client`. Commerce key issuance
идёт дальше через `packages/db/src/pricing-provisioning-v2.ts`; OpenKeys issuance и service-account
admin CAS используют общий external-owner builder напрямую. При ненулевом context balance writers
завершают funding/policy/active+recovery extension, service writer — rule-free `meter_only`
policy/extension, и все требуют exact readback плюс свежий context до usable результата. При null
context release-v2 path dormant.
Managed Stage 8 capture подключён цепочкой strict `packages/contracts` → raw-text
`packages/engine-client` → `packages/db/src/pricing-stage8-capture-jobs-v2.ts` → `apps/worker`.
Единственный job producer — AdminGuard-protected
`POST /v1/admin/pricing-stage8-capture-v2/stage` в `apps/api` с UUID idempotency key, verified actor,
reason и exact capture bounds; paired GET возвращает bounded local job/artifact snapshot. Worker
сохраняет exact engine bytes до combined collector и атомарно завершает combined artifact/job;
GET раскрывает только freshness и sanitized blocker source/code/count с hashed subjects.
Engine subjects сохраняют canonical `sha256:v1`, commerce authority subjects — canonical
`sha256:v2`; combined/status schema принимает обе opaque версии, не расширяя версии evidence
identity. OpenKeys first-delivery authority берётся из prepared target 1:1 policy, а не из
pre-cutover legacy source/engine scalar.
Startup, migration, polling и activation request не создают capture job; capture не создаёт
activation job и не двигает head. После GREEN commerce producer SHA `apps/admin` подключён отдельным
consumer checkpoint: `/pricing` показывает bounded queue/artifact snapshot и stage'ит новый job
только после explicit exact-bounds form, confirmation phrase и fresh browser preflight.
Activation подключён только через цепочку strict `packages/contracts` → единственный transport
`packages/engine-client` → `packages/db/src/pricing-release-activation-jobs.ts` → `apps/worker`.
DB consumer строит request из persisted passed evidence и engine release digests, хранит body до
сети, перед первой delivery повторяет double-scan engine/OpenKeys и commerce/service ownership
authority, а после возможной доставки восстанавливает lost ACK только exact replay и сохраняет
полный validated receipt. Persisted service-inventory digest обязателен и должен совпасть с fresh
capture; старые evidence rows с `NULL` не stage'ятся. Raw identities в blocker/error artifact не
выходят. Recovery expectation берётся только из durable cutover receipt. Startup, migration,
Stage 8 collector и worker polling не stage'ят activation job. Единственный producer — защищённый
`POST /v1/admin/pricing-release-activation-v2/stage` в `apps/api`; paired GET отдаёт bounded local
snapshot и отдельно timestamped engine head. `apps/admin` подключён к этому expand-only
контракту отдельным consumer-коммитом после GREEN producer SHA: `/pricing` показывает bounded
snapshot и fail-closed stage'ит только explicit cutover/recovery после fresh browser preflight.

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
| `packages/contracts` | zod-схемы engine/pricing/auth/checkout-контрактов, canonical models и catalog pins; target pricing — global B2C/provider/model rules, B2B, OpenKeys 1:1, service `meter_only`, pricing releases; strict Stage 5/6 admin request/status summaries | `apps/api`, `apps/worker`, `apps/openkeys`, `packages/db`, `packages/engine-client`. НЕ импортируют: `apps/web`, `apps/sales-*`, `apps/admin` | `docs/commerce/MULTI-DISCOUNT.md` |
| `apps/api` (публичный API) | HTTPS `backend.apitoken.sale/v1/*`, cookie-сессия | `apps/web` (`src/lib/api.ts`, `NEXT_PUBLIC_BACKEND_URL`) | `docs/commerce/COMMERCIAL_BACKEND.md` |
| `apps/api` (админ API) | `/v1/admin/*` через Caddy-rewrite `admin.apitoken.sale/admin/*`, заголовок `x-admin-key`; protected Stage 5 dry-run/materialize и Stage 6 status/stage требуют verified actor, exact plan digest и audit mutation reason; exact pre-cutover `/pricing-policy-delivery-repairs` supersede'ит только доказанный dead `strict + legacy_single` job и создаёт новую audited shadow delivery; per-service CAS `/service-account-inventory/*` сверяет полный engine inventory и после cutover завершает exact `meter_only` release-v2 policy/extension до durable registration; тот же канал и ключ на `content-studio.apitoken.sale/v1/*` | будущий отдельный `apps/admin` Stage 5/6 UI consumer подключается только после GREEN producer; `packages/db` materializer/orchestration; engine Control API — через typed `packages/engine-client`; `apps/content-studio` (`/v1/admin/content/*`) | `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE6.md`, `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md` |
| `apps/openkeys` (админ API) | `/api/internal/admin/*` через Caddy `admin.apitoken.sale/openkeys-admin/*`, заголовок `X-OpenKeys-Control-Key` | `apps/admin` | `docs/product/OPENKEYS.md` |
| `apps/openkeys` (pricing inventory producer) | loopback/internal GET `/api/internal/pricing/v2/inventory`, bounded cursor + full `sha256:v2` manifest под `X-OpenKeys-Control-Key`; без secrets/live money | `packages/db` Stage 5/Stage 8 consumers и activation first-delivery preflight исчерпывают cursor дважды и требуют один неизменный full-manifest digest; подключены только после GREEN producer SHA | `docs/product/OPENKEYS.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md`, `docs/ops/DEPLOYMENT.md` |
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
| `apitoken-affinity-redis.service` | два инстанса под одним юнитом: history `127.0.0.1:6379` (legacy Compose service identity `affinity-redis`, Codex response history, `allkeys-lru`, 512 MiB) и affinity `127.0.0.1:6380` (service `cache-affinity-redis`, cache-lineage L2 + advisory cooling hints `claude-api:cool:v1`, `allkeys-lru`, 128 MiB); installer выполняет additive Compose reconcile до two-target monitoring gate и не останавливает history; экспортеры `9121`/`9122` (job `redis`, лейбл `instance_role`); пароль читается из `CLAUDE_API_REDIS_PASSWORD` в `server.env` и публикуется как JSON-секрет `observability/secrets/affinity_redis_password` | Prometheus → алерты `AffinityRedisDown`, `AffinityRedisEvictingKeys`, `AffinityRedisMemoryHigh` | `docs/ops/MONITORING.md#affinityredisevictingkeys` |
| `crates/forward` billing writer + `crates/server` `/metrics` | гистограмма `claude_api_billing_pg_command_duration_seconds{op="reserve\|settle\|acquire_capacity"}` (латентность вокруг retry-обёртки, 10 бакетов 1 ms–1 s) и gauge `claude_api_billing_write_queue_depth` (занятые слоты 4096-слотового writer-канала); обе операционные (без money-лейблов), видимы readonly-ключу; PostgreSQL-only, SQLite fallback гистограмму не публикует | Prometheus → алерты `BillingPGCommandLatencyHigh`, `BillingWriteQueueBacklog`; Grafana `production-overview` row «Billing writer (PostgreSQL hot path)» | `docs/ops/MONITORING.md#billingpgcommandlatencyhigh` |

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
  После dispatch плоскости одинаково fail-closed валидируют optional execution controls:
  missing/null остаются absence, а malformed non-null boolean/output limit получает локальный
  400 до reserve/upstream; output alias precedence и точный OpenAI `error.param` являются частью
  produced universal contract. Translated SSE также fail-closed на границе плоскости: Anthropic
  требует полный Messages lifecycle до `message_stop`, Gemini — `finishReason` либо
  `promptFeedback.blockReason` до EOF; malformed/premature stream превращается в lane-shaped
  terminal error, а не в ложный success. Router не нормализует и не исправляет эти поля или
  события, а сохраняет request body и streaming response.
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
- **ClaudeStore emergency transport (`crates/server` → `crates/forward` → ClaudeStore API3).**
  `crates/server/src/config.rs` единолично читает strict enable switch и secret, а compile-fixed
  `https://api3.claudestore.store` нельзя заменить env-URL. `crates/forward/src/proxy.rs` потребляет
  конфиг только для metered Anthropic `POST /v1/messages`: после terminal всей локальной pre-byte
  ротации выполняет максимум один очищенный внешний attempt, сохраняет исходный request/reservation
  identity и customer exact settlement, но не пишет local subscription spend/quota/calibration/
  affinity. Prometheus потребляет fixed-cardinality attempts/successes/failures; failure alert и
  rollback описаны в `docs/ops/MONITORING.md#claudestorefallbackfailing`. Контракт и evidence —
  `docs/engine/CLAUDESTORE_FALLBACK.md`.
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
  до materialization 32 MiB universal body конкурентно запускает три bounded probe, принимает
  первый exact schema-v1 success либо terminal 401, а mixed-version/transport/5xx считает
  inconclusive. Fail-fast 64 MiB budget с шагом 1 MiB динамически растёт по фактическим chunked
  байтам, имеет 15-секундный idle и 5-минутный абсолютный body deadline и не создаёт
  execution-очередь.
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
  Каталоги больше 256 candidates режутся на детерминированные чанки; failed chunk закрывает весь
  overlay. Credential-specific overlay не кэшируется и никогда не попадает в общий catalog TTL-cache.
  Публичные provider vhost'ы `/internal/*` не обслуживают.
- **Контракт catalog runtime metadata (provider planes → router).** Anthropic производит native
  `max_input_tokens`/`max_tokens`/`capabilities`; owned OpenAI и Gemini model resources производят
  expand-only `apitoken.limits`/`apitoken.capabilities`, включая modalities, tool calling,
  structured outputs и streaming; OpenAI также может публиковать provider-authored `name`. Codex
  context/name — last-good authenticated provider evidence, агрегированное консервативно между
  serving profiles; output/efforts/Fast/adapter capabilities и Gemini model-specific metadata
  принадлежат reviewed runtime contract. Потребитель —
  `crates/router`: после отдельного producer-first GREEN SHA он строго валидирует и нормализует
  metadata в unified `apitoken`, сохраняет top-level capability mirrors, а malformed metadata
  переводит плоскость на last-good/degraded. Он также снимает глобально конфликтующие aliases,
  сохраняя исполнимость namespaced IDs и приватный native ID для rewrite/preflight. Pricing overlay
  дополняет, а не заменяет runtime metadata. Pricing rates, account identity и credential в эту
  связь не входят; неизвестные значения не выводятся из model id или pricing таблиц. Контракт —
  `docs/engine/UNIFIED_ROUTER.md` §«Модели и каталог».
- **Контракт unified catalog (router → OpenCode integration).** `crates/router` производит
  аутентифицированный key-scoped `/v1/models`: authoritative runtime metadata дополняется
  персональной pricing projection без изменения исходных model IDs. Потребитель — канонический
  `packages/opencode-router-plugin`: live-ответ переводится в model/variant/Fast schema OpenCode,
  а локальный schema-v2 last-good cache содержит только зашифрованные capability records без
  `pricing` и
  `cost`, привязан к exact credential/base URL и ограничен schema/TTL/max-stale guards. Cached
  fallback всегда явно stale и без стоимости. OpenCode transport не потребляет Gemini
  `inlineData`, поэтому plugin не рекламирует generated-image output; нативный Gemini API остаётся
  поддерживаемой image surface. Router-owned preset публикует live member IDs, conservative
  guarantees и marker переменной цены, но plugin намеренно не превращает его в OpenCode model.
  Других потребителей cache-файла нет. Контракт —
  `docs/engine/UNIFIED_ROUTER.md` §§«Совместимость с harness-агентами», «Модели и каталог».
- **Fallback telemetry (router/provider planes → Prometheus, фаза 6.4c).** `crates/router`
  производит unauthenticated loopback `/metrics`; Caddy stable origin 8802 направляет scrape в тот
  же active router slot 8800/8801, что и публичный vhost, с ровно 18
  `claude_router_fallback_total{from_namespace,to_namespace,reason}` series плюс fixed-cardinality
  admission/auth/catalog/pricing/policy/header-timeout/balance telemetry; публичный Caddy allowlist
  этот путь не пропускает. Каждая fixed `crates/server` plane через существующий
  authenticated `/metrics` производит три bounded
  `claude_api_execution_not_started_total{plane}` series, считая только exact response реально
  возвращённой plane. Потребитель — `observability/prometheus/prometheus.yml` и recording/alert
  rules; Alertmanager/operator используют runbooks `RouterMetricsDown`, `RouterFallbackRateHigh`,
  `RouterConnectionRefusedFallback`, `RouterAdmissionFailures`, `RouterAuthorityFailures` и
  `RouterResponseHeaderTimeout`, а money-regression detectors остаются отдельными. Model,
  credential, account, group и request identity через эту связь не проходят. Контракт —
  `docs/engine/ROUTING_FENCING.md` §§5.3–6 и `docs/ops/MONITORING.md`.
- `crates/authbot` — производитель доступа вне слоёв; OAuth-callback на `127.0.0.1:8796`.

## 3. Модели и цены — где ещё зеркалятся

Authority — `crates/metering` (выше). Всё нижеописанное — зеркала, которые надо трогать
вместе с ним (полный обход — чеклист «Новая модель» / «Изменение цены» в
`docs/CHANGE_CHECKLISTS.md`):

- `packages/contracts` — `CURRENT_*_CANONICAL_MODELS`, catalog generations и pricing release
  schemas. Frozen capability generation 3 сохраняет исходный main Anthropic/OpenAI/Gemini set.
  Immutable generation 4 исторически добавила
  `gemini-3-flash-preview` (`google` — internal engine provider id), но gate старого public wire дал
  404, поэтому generation 4 остаётся rejected/dormant и не может быть материализована или
  активирована; её digest не переписывается. После complete fresh exact-implementation Pro+Ultra
  gate admitted generation 5 повторяет reviewed set под новым digest; Stage 5 main catalog включает
  Preview. OpenKeys generation 5 остаётся явным Anthropic/OpenAI subset.
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
| `router.apitoken.sale` | atomic `router_backend` → claude-router slot `:8800` или `:8801`; stable loopback `:8802` использует тот же backend |
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

Stable provider origins 8790/8792/8794 синтезируют внутренний
`X-Apitoken-Execution-State: not_started` только на Caddy `no healthy upstream`; обычный runtime
503 его не получает. Публичные provider vhost'ы снимают этот header, а loopback router использует
его как безопасное fencing-доказательство до следующей explicit fallback attempt.

### systemd (`systemd/`) — сервис → приложение

`claude-api-anthropic@` → Anthropic-слоты 8787/8788 (текущий юнит; `claude-api@` — legacy) ·
`claude-api-openai@` → 8793/8797 · `claude-api-gemini@` → 8795/8799 · `claude-router@` → 8800/8801 (`claude-router` → 8798 только legacy handoff) ·
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

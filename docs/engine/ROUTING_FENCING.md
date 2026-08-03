# ROUTING_FENCING.md — детальный дизайн этапа 6 UNIFIED_ROUTER (routing + attempt fencing)

Статус: фазы 6.1–6.3, policy/presets consumer 6.4b и telemetry/mock-load часть 6.4c
реализованы; контракт фазы 6.4 зафиксирован 2026-08-02. Serial fallback остаётся выключенным по
умолчанию: впереди post-deploy live canary на точном GREEN SHA и отдельный production unit-флаг.
Реализация следует этому документу; отклонение требует его пересмотра.

Дата фактбазы: 2026-08-02 (повторный аудит production после фаз 6.1–6.3).
Ссылки вида `proxy.rs:1880` — `crates/forward/src/proxy.rs`, если не указано иное.

## 1. Scope этапа 6

По `UNIFIED_ROUTER.md` п. 6: OpenRouter-grade routing — provider preferences, явные
fallback-списки моделей, attempt fencing (execution group / единственный billable winner),
per-account policy, телеметрия, presets. НЕ входит: кворумные/параллельные попытки (race
нескольких моделей), кросс-провайдерное хранилище ответов, изменение universal-словарей
этапов 3–5.

## 2. Фактбаза (аудит 2026-08-01)

### 2.1. Двойного биллинга внутри плоскости НЕТ — конструктивно

Один `engine_request_id` (UUIDv4, CSPRNG) создаётся ДО первой попытки ротации и делится всеми
in-plane ретраями: Anthropic `proxy.rs:955`, Codex `codex/billing.rs:98`, Gemini
`gemini/api.rs:2180`. Exactly-once денег: `UNIQUE INDEX ledger_request_once ON
ledger(kind, request_id)` + outbox PK + reservation PK (`registry/src/lib.rs:1517-1520`);
повторный settle с другим actual — hard error, не тихий дубль (`pg.rs:1907`).

### 2.2. Дыра — строго на границе плоскостей/моделей

Любой retry НАД плоскостью (router fallback на другую модель/плоскость) создаёт НОВЫЙ
request_id и НОВЫЙ reserve: если первая плоскость реально исполнила запрос (а router увидел
timeout/5xx/обрыв), обе попытки billable. Router сегодня stateless и не МОЖЕТ безопасно
ретраить: извне плоскости недоступно никакое состояние попытки (`router/src/proxy.rs:66-114`
— один send, ноль ретраев, connect-timeout 2 с, 5xx плоскости транзитом). Единственный
безопасный сигнал без нового контракта — TCP connect-refused (запрос физически не ушёл).

### 2.3. Граница «started» сегодня разная на четырёх ветках

| Ветка | Durable «started» | Что видит router при отказе ДО started |
|---|---|---|
| Anthropic | upstream 2xx headers → `mark_delivering` ДО первого байта клиенту (`proxy.rs:1880`) | не-2xx (плоскость отдала внутренний 5xx/429 после исчерпания ротаций) |
| Codex stream | флаг `emitted` — первая дельта в клиентский канал (`runner.rs:360-363`); `mark_delivering` стоит ДО попытки (`api.rs:302`), refund pre-delta через `HoldGuard` settle(hold,0) | не-2xx, либо 200 с error-событием внутри стрима |
| Codex non-stream | после полного turn (`api.rs:321`) | не-2xx |
| Gemini stream | первый переведённый публичный event (`api.rs:1728`, bounded-прелюдия :2305-2365) | не-2xx |
| Gemini non-stream | после успеха (`api.rs:2579`) | не-2xx |

Вывод: не-2xx от плоскости СЕГОДНЯ почти всегда означает «не доставлено, деньги возвращены» —
но это наблюдаемое поведение, а не контракт: ни одна ветка не гарантирует его явно, и для
Codex stream даже 200 не означает billable. Без явного сигнала router не может отличить
«не начала» от «начала и упала» (fact #4: plane-level 5xx до доставки = refund,
200 + mid-stream `event: error` = billable delivering — различие только внутри плоскости).

### 2.4. Существующие субстраты (расширяем, не строим заново)

- Состояние резерва per-request: `reserved → delivering → [settlement_pending] →
  settled/canceled` + lease + reconcile; `reserved` reconcile отменяет без списания —
  де-факто сегодняшний «not_started» (`lib.rs:4824-4835`).
- «delivering ⇒ billable» покрыто crash-recovery: истёкший lease в `delivering` → charge
  полного hold, только при доказуемо мёртвом owner epoch (`pg.rs:2162-2191`).
- Прецедент fail-closed fencing: неудачный durable `mark_delivering` → нет «бесплатного»
  usage (полный hold с reference `delivery-marker-failed`, клиенту 503, `proxy.rs:1880-1905`).
- Зачаток (group, attempt)-идентичности в capacity plane: `capacity_lease_id =
  "{request_id}:{attempt}"` (`proxy.rs:1632`), PG `capacity_leases` с exact-replay
  семантикой (`pg.rs:2201-2233`).
- Стабильные per-turn ID, переживающие ротацию: Codex `cal_*` (`runner.rs:272-274`), Gemini
  `upstream_request_id` (`api.rs:2133-2136`), Anthropic `engine_request_id`.
- Телеметрия: `apitoken_balance_divergence_nano` (прямой детектор лишних списаний),
  `apitoken_engine_settlement_pending` + алерты `EngineSettlementBacklog`,
  `EngineExpiredLeasePresent`, per-plane счётчики `upstream_{429,auth,5xx}`,
  `gemini_stream_start_failures_total` (pre-byte фейлы).

## 3. Контракт `execution_state=not_started` (MVP fallback, фаза 6.1)

### 3.1. Семантика

Плоскость выставляет HTTP-заголовок `x-apitoken-execution-state: not_started` на ответе,
когда выполнены ВСЕ условия:

1. Ни один байт публичного ответа не ушёл клиенту (тот же критерий, что in-plane retry
   boundary: Anthropic — до upstream 2xx headers; Codex — до `emitted`; Gemini — до первого
   переведённого public event).
2. Reserve по этому request_id НЕ БУДЕТ тарифицирован: durable зафиксирован refund
   (settle(hold, 0) / cancel reserve), либо reserve гарантированно отменится reconcile как
   `reserved` без charge.
3. Ответ не-2xx. На 2xx заголовок НЕ выставляется никогда: 2xx — всегда конец обсуждения
   (mid-stream error event — ambiguous, решение клиента, `UNIFIED_ROUTER.md` «Семантика
   fallback»: автоматического повтора на другой модели нет).

Заголовок — внутренний контракт router↔plane: router ОБЯЗАН снимать его перед отдачей ответа
клиенту (клиенты не должны зависеть от внутреннего состояния движка).

### 3.2. Обязанности плоскостей (per-plane точки выставления)

- **Anthropic** (`proxy.rs`): исчерпание бюджетов ротации → итоговые не-2xx ответы
  (429/5xx/503 exhausted, network-fail исходы) — все они до `mark_delivering`, reserve ещё в
  `reserved`: header ставится при условии, что settle этой ветки — refund/cancel. Ответы
  ПОСЛЕ `mark_delivering` (включая SseErrorTail внутри 200) — без заголовка.
- **Codex** (`api.rs` + `runner.rs`): pre-delta отказы с `HoldGuard` refund (stream) и
  не-успех non-stream до turn end — header; любой ответ после `emitted` — без.
- **Gemini** (`api.rs`): отказы в bounded-прелюдии (provider_error до первого public event),
  non-stream не-успех — header; после первого public event — без.
- Единый unit-контракт на плоскость: «ответ с header ⇒ ledger не содержит и не будет
  содержать charge по request_id» (проверяется на уровне settle-итогов в тестах ветки).
- Universal Chat/Responses-адаптеры (`anthropic.rs`/`anthropic_responses.rs`,
  `gemini/chat.rs`/`gemini/responses.rs`) покрыты с 2026-08-02: локальные pre-request
  отказы получают `not_started`, пересборка не-2xx сохраняет только точный авторитетный
  сигнал плоскости, а ошибки разбора/сборки после 2xx явно снимают его, потому что charge
  уже возможен. Gemini Messages skin (`gemini/skin.rs`) следует тому же правилу для своей
  поверхности. Отсутствующий либо неизвестный сигнал остаётся fail-closed: retry запрещён
  (§3.3).
- **Stable Caddy origins** (8790/8792/8794) синтезируют тот же exact `not_started` только когда
  сам reverse-proxy handler возвращает `503 no healthy upstream`: ни один health-gated runtime не
  принял запрос. Runtime-produced HTTP 503 не входит в `handle_errors` и не получает сигнал.
  Внешние provider-vhost'ы снимают header на outer hop; router видит его только по loopback.

### 3.3. Обязанности router (фаза 6.2)

Retry на следующую модель fallback-списка разрешён РОВНО в двух случаях:

1. Ответ плоскости не-2xx С заголовком `x-apitoken-execution-state: not_started` (header
   снимается, клиенту уходит ответ последней попытки). Router логирует только bounded
   metadata попытки; headers и тела запросов/ответов в лог не попадают.
2. TCP connect-refused к плоскости (запрос физически не ушёл).

Запрещено: retry на timeout, на 5xx БЕЗ заголовка, на обрыв после заголовков, на 402
(баланс аккаунта — повтор на другой модели той же учётки бессмысленен), на 4xx клиента.
Исключение внутри диапазона 4xx — `429` с точным `not_started`: это capacity-отказ
плоскости, а не исправимая клиентом ошибка. Exact означает одно значение `not_started`;
другой регистр, несколько значений и неизвестное значение fail closed.

## 4. Execution group / attempt identity (зрелая версия, фаза 6.3)

MVP-контракт §3 закрывает гонку «вторая попытка стартовала, пока первая billable» только при
исправном сигнале. Durable-гарантия против бага/рассинхрона — group identity:

- **Router генерирует** `group_id` (UUIDv4) на логический запрос с fallback-списком и шлёт
  плоскости `x-apitoken-execution-group: <group_id>` + `x-apitoken-attempt: <N>` (N = 1,2,…
  по порядку обхода списка). Без fallback-списка заголовки не выставляются — плоскость
  работает как сегодня (group = request_id).
- **Граница доверия:** Caddy удаляет оба заголовка на публичных provider-vhost'ах и на
  `router.apitoken.sale`. Router дополнительно удаляет клиентские копии перед каждой попыткой и
  только затем инжектирует собственную CSPRNG UUIDv4/порядковый attempt. Плоскость принимает либо
  полностью отсутствующую пару, либо ровно по одному каноническому значению; partial, duplicate,
  не-lowercase/non-v4 UUID и неканонический positive decimal fail closed до reserve.
- **Registry (expand-only миграция):** `reservations` получает nullable `group_id TEXT` и
  `attempt INTEGER NOT NULL DEFAULT 1`. PostgreSQL default не может ссылаться на другую колонку,
  поэтому `group_id IS NULL` — явное совместимое представление старой/прямой попытки, а effective
  group в runtime определяется как `COALESCE(group_id, request_id)`. Новая таблица
  `execution_group_winner(group_id TEXT PRIMARY KEY, winner_request_id TEXT NOT NULL,
  decided_at BIGINT NOT NULL)` хранит одну insert-first-wins строку на effective group.
- **Settle path:** nonzero settle (charge > 0) атомарно (в той же БД-транзакции) делает
  `INSERT INTO execution_group_winner … ON CONFLICT DO NOTHING` и читает победителя:
  - winner == мой request_id → обычный settle;
  - winner != мой request_id → двойное исполнение обнаружено durable: charge принудительно
    0 (refund), фатальный structured event `execution_group_double_winner` + метрика
    (должна быть 0 всегда; >0 = баг контракта §3, инцидент).
  Refund-settle (charge == 0) winner не назначает.
- **Strict-policy loser:** исходный outbox payload (`actual`, usage, disposition) остаётся
  неизменным для аудита exact replay, но money-обработка выполняется как внутренний `cancel` с
  effective actual 0 и без usage/charge rows. Reservation и funding allocations фиксируют нулевой
  charge и полный release. Exact replay выводит effective actual из durable winner row.
- **Retention:** winner удаляется только после bounded terminal-prune последней reservation с тем
  же effective group. Пока существует хотя бы одна reservation/replay-свидетельство группы,
  winner сохраняется, даже если reservation победителя уже стала eligible для удаления.
- **Инвариант exactly-once не ослабляется:** существующий `UNIQUE ledger(kind, request_id)`
  остаётся per-attempt; winner-правило добавляет «ровно один nonzero winner на группу».
- Миграции — expand-only, двумя коммитами по `AGENTS.md`: сначала совместимая со старым writer
  схема (nullable group identity, attempt с default, новая таблица), код — после зелёных
  `deploy/migration` + `deploy/watchdog`.

## 5. Routing-интерфейс router (фаза 6.2)

- Новое необязательное поле запроса `models: [<catalog id>, …]` (OpenRouter-совместимое
  соглашение; expand-only контракта universal endpoint — старые клиенты не затронуты).
  `model` остаётся обязательным и трактуется как первый элемент цепочки; `models` задаёт
  продолжение. Пустой список/дубликаты/неизвестные id → `400` в конверте lane входного пути.
  Флаг `CLAUDE_ROUTER_FALLBACK_ENABLED` строгий (`0|1|false|true`) и по умолчанию false;
  при выключенном флаге само присутствие `models` даёт `400` до любого network вызова.
- Запрос без `models` сохраняет прежний контракт: исходные body bytes не меняются,
  namespaced ID выбирает плоскость напрямую даже при недоступном каталоге, alias использует
  кэшированный aggregate catalog. Явная fallback-цепочка целиком валидируется по одному
  aggregate snapshot ДО первой попытки; alias и namespaced ID одной catalog entry считаются
  дубликатом. Затем `models` удаляется, а `model` заменяется для каждой попытки.
- Router буферизует только тело запроса (как сегодня, 32 MiB), выбирает плоскость каждой
  попытки независимо (namespace/alias — существующий `catalog::namespace_lane`); retry —
  только по правилам §3.3; ответ клиенту — последней попытки (успех или её ошибка),
  in-flight ответ НЕ буферизуется (инвариант byte-passthrough не затрагивается: retry
  возможен только до первого байта).
- `provider` preferences-объект (order/allow/sort по цене-латентности) — НЕ в этой фазе;
  отдельный пакет после живой телеметрии fallback.
- Per-account policy: существующий substrate `crates/registry/src/pricing.rs` (provider
  switches, account policy) будет фильтровать fallback-цепочку ДО первой попытки в фазе
  6.4; фаза 6.2 policy не читает.

### 5.1. Policy preflight (контракт 6.4a)

Policy остаётся собственностью engine и не переносится в stateless router. Каждая fixed
provider-плоскость добавляет одинаковый внутренний `POST /internal/router/policy/preflight`:

```json
{
  "schema_version": 1,
  "candidates": [
    {
      "id": "anthropic/claude-sonnet-5",
      "provider_id": "anthropic",
      "canonical_model_id": "claude-sonnet-5"
    }
  ]
}
```

Ответ содержит только версию, режим `unrestricted|strict` и ordered subset входных `id` в
поле `allowed`; account ID, policy/rule/digest, цены и причины запрета наружу не выходят.
Тело ограничено 64 KiB, список — 32 уникальными кандидатами, идентификаторы — 256 байтами;
неизвестные поля и неизвестные provider ID отклоняются. Credential передаётся теми же auth-заголовками, что и
исполняемый запрос. Env-admin получает `unrestricted`; невалидный credential — `401`, ошибка
authority — `503`.

Для активного strict binding handler читает `KeyAuth` и один когерентный
`PricingReadBundle`, строит runtime manifest только через
`RuntimePricingManifest::from_evidence` и вызывает существующий `resolve_pricing` для каждого
кандидата. `Resolved` допускается, любой typed rejection запрещает модель. Google-кандидаты
для strict account запрещены, пока Gemini plane сама fail-closed отклоняет strict admission;
preflight не имеет права обещать исполнимость, которой нет на денежном пути. Unbound,
legacy-scalar и shadow bindings остаются `unrestricted`: их live admission не меняется.

Router делает ровно один preflight на логическую цепочку после catalog/preset/preferences
валидации, но до attempt 1. Он пробует stable origins последовательно без привязки authority к
одному провайдеру; `404/405`, transport/`5xx` и malformed response позволяют попробовать
следующую плоскость, но отсутствие хотя бы одного валидного ответа заканчивается lane-shaped
`503` без исполнения. `401` терминален. Решения не кэшируются и не индексируются по ключу:
policy mutable, а credential не должен попадать в память дольше запроса. Ответ обязан быть
точным subset исходного списка без дубликатов; иное — producer-contract failure и `503`.
Пустой strict subset → lane-shaped `403 policy_restricted` до первой попытки.

Producer endpoint реализован 2026-08-02 в `crates/server/src/router_policy.rs` и зарегистрирован
на всех runtime modes до provider-specific route composition. Публичные Caddy allowlists не
пропускают `/internal/*`; router обращается к нему только через stable loopback origins. Endpoint
выкатывается и проходит `deploy/watchdog` раньше consumer router. Такой порядок делает expand-only
rollout безопасным; consumer всё равно понимает mixed-version окно и fail-closed перебирает
плоскости вместо зависимости от Anthropic origin.

Consumer реализован в `crates/router/src/policy.rs`: после построения эффективной цепочки router
сначала пробует origin первой candidate lane, затем остальные candidate/fixed origins без
повторов; запрос и ответ ограничены 64 KiB. Все значения `x-api-key`, `x-goog-api-key` и
`authorization` сохраняются (OR-семантика engine auth), но прочие headers не копируются. Для
`unrestricted` принимается только полный исходный список, для `strict` — точный ordered subset;
unknown/duplicate/out-of-order ID, неизвестное поле/режим/версия или oversized body считаются
producer-contract failure. Интеграционная TCP-матрица покрывает `404`, `5xx`, malformed и
transport failover, terminal `401`, strict filter до attempt 1 и пустой `403` без исполнения.

### 5.2. Provider preferences и presets (контракт 6.4b)

Реализовано 2026-08-02 в `crates/router/src/routing.rs`, `policy.rs`, `presets.rs` и compiled
`crates/router/routing-presets.json`; rollout остаётся default-off до 6.4c.

Universal body принимает optional OpenRouter-shaped объект `provider` только с полями:

- `order`, `only`, `ignore`: массивы уникальных namespace `anthropic|openai|google`;
- `allow_fallbacks`: boolean; `false` сохраняет только первый разрешённый кандидат после
  фильтров/сортировки;
- `sort`: `price|latency`; deterministic rank берётся из version-controlled router routing
  manifest, а не из непроверенного client input или плавающей telemetry.

Неизвестное поле/значение, дубликат, пересечение `only` и `ignore`, отсутствующий rank для
`sort` или пустая цепочка после фильтра → lane-shaped `400`. Порядок преобразований строгий:
expand preset → один aggregate catalog snapshot → canonical dedup → `only`/`ignore` →
explicit `order` (неперечисленные namespaces сохраняют относительный порядок следом) →
`sort` как primary rank с request order как stable tie-break → `allow_fallbacks` → policy
preflight. Поле `provider`, как и `models`, удаляется перед отправкой в плоскость.

Reserved catalog IDs `preset/auto`, `preset/quality`, `preset/fast`, `preset/hermes` описаны
reviewed manifest'ом рядом с `crates/router`; manifest содержит ordered model IDs и integer
price/latency ranks. Preset разворачивается до policy preflight и никогда не доезжает до
плоскости. Недоступный member пропускается; preset публикуется в `/v1/models` только если
aggregate snapshot содержит хотя бы один его member, а пустое раскрытие → `503
catalog_unavailable`. `preset/hermes` содержит только явно проверенные модели с контекстом не
меньше 64K. Изменение модели/rank — обычное reviewed изменение manifest + документации и
пересборка router, поэтому устаревшая модель не зашивается в недоступный host config.

Текущие reviewed цепочки (первый live member — primary):

| Preset | Ordered members |
|---|---|
| `preset/auto` | `anthropic/claude-sonnet-5` → `openai/gpt-5.6-terra` → `google/gemini-3.6-flash` |
| `preset/quality` | `anthropic/claude-opus-5` → `openai/gpt-5.6-sol` → `google/gemini-3.1-pro-preview` |
| `preset/fast` | `openai/gpt-5.6-luna` → `google/gemini-3.1-flash-lite` → `anthropic/claude-haiku-4-5-20251001` |
| `preset/hermes` | `anthropic/claude-sonnet-5` → `openai/gpt-5.6-terra` → `google/gemini-3.6-flash` |

Manifest содержит positive integer `price_rank`/`latency_rank` и проверенный
`context_tokens` для всех 22 опубликованных на дату реализации catalog ID. Меньший rank
предпочтительнее; это reviewed ordinal, а не вычисление цены конкретного запроса и не live
telemetry. Поэтому новая catalog model без явного rank продолжает работать в обычном порядке,
но `provider.sort` с ней fail closed получает `400` до policy/attempt. Startup-валидация требует
ровно четыре reserved preset, уникальные ranked members и context ≥64K у каждого Hermes member.

Любое присутствие `models`, `provider` или `preset/*` подчиняется одному rollout-флагу. Пока
`CLAUDE_ROUTER_FALLBACK_ENABLED=false`, запрос отклоняется до catalog/policy/network work;
single-model запросы без этих полей сохраняют byte-identical поведение фаз 1–5.

### 5.3. GA rollout (контракт 6.4c)

Telemetry и reproducible harness реализованы default-off: router/plane counters, loopback scrape,
recording/alert rules с runbooks, concurrent mock-load и stdin-only live-canary runner. Сам live
canary выполняется только после выката этого пакета; его результат и production flag не
предвосхищаются документацией.

Router экспортирует `/metrics` без авторизации на loopback. Fallback continuation увеличивает
`claude_router_fallback_total{from_namespace,to_namespace,reason}` ровно один раз перед
следующим attempt; множества labels compile-fixed (3×3 namespaces, два reason). Плоскость
увеличивает `claude_api_execution_not_started_total{plane}` ровно для фактически возвращённого
наружу exact `not_started` на non-2xx. Те же fixed dimensions покрывают active body units,
overload/read timeout, auth outcomes/latency, catalog refresh/degradation, pricing/policy failure,
response-header timeout и read-only `/balance` failover. Credential/model/group/request IDs в
metrics запрещены; `RouterAdmissionFailures`, `RouterAuthorityFailures` и
`RouterResponseHeaderTimeout` замыкаются на одноимённые runbook-секции.

Порядок включения: producer 6.4a → consumer 6.4b при default-off → telemetry/Prometheus 6.4c
при default-off → mock-load и live canary отдельным router-процессом → unit-флаг в production.
Canary обязан доказать policy filtering до attempt 1, serial continuation, отсутствие retry на
ambiguous outcome, нулевой рост double-winner и balance divergence, приемлемый settlement
backlog. Production-флаг включается только последним reviewed коммитом; rollback — возврат
флага в false новым коммитом, без удаления expand-only contract.

## 6. Телеметрия и верификация

- Фаза 6.2 пишет bounded attempt-log: surface, порядковый номер/размер цепочки, публичный
  canonical catalog ID, lane, HTTP status и reason (`not_started`/`connect_refused`/none).
  URL/query, auth headers, credentials и request/response bodies запрещены.
- Счётчики фазы 6.4: `claude_router_fallback_total{from_namespace,to_namespace,reason}`
  (reason: `not_started`/`connect_refused`), `claude_api_execution_not_started_total{plane}`,
  fixed-cardinality admission/auth/catalog/pricing/policy/balance-header-timeout ряды, а фаза 6.3
  уже экспортирует `claude_api_execution_group_double_winner_total`. Критический
  `ExecutionGroupDoubleWinner` срабатывает на любом росте за 5 минут; runbook —
  `docs/ops/MONITORING.md#executiongroupdoublewinner`. `RouterMetricsDown` закрывает потерю
  отдельного scrape, `RouterFallbackRateHigh` — устойчивую скорость >1 continuation/s, а
  `RouterConnectionRefusedFallback` — любой transport-proof за 5 минут. Recording rules
  `claude_router:fallback_rate5m` и `claude_api:execution_not_started_rate5m` оставляют только
  bounded namespace/plane dimensions.
- Групповые лейблы на существующих денежных рядах НЕ добавляются (кардинальность); после
  фазы 6.3 group_id допустим только в structured-логах попыток, не в metric labels.
- Детекторы регрессий: `apitoken_balance_divergence_nano` (существующий),
  `EngineSettlementBacklog`, `EngineExpiredLeasePresent` — проходят нагрузочный период с
  включённым fallback до GA.
- Флаг rollout: fallback выключен по умолчанию (`CLAUDE_ROUTER_FALLBACK_ENABLED=false`), включается
  config-флагом на canary; `deploy`-чеклист — измерение доли ambiguous-исходов (timeouts без
  заголовка) до и после включения.
- Верификация 6.2: TCP integration router с двумя mock-плоскостями доказывает serial
  not_started/ConnectionRefused retry и fail-closed ambiguous outcomes; per-plane тесты 6.1
  доказывают refund сигнальной попытки. Фаза 6.3 покрыта SQLite и real-PostgreSQL matrix:
  reverse settlement order, zero-settlement, exact loser replay, strict funding refund и ровно
  один charge на group; forward-тесты отдельно проверяют durable group/attempt для всех плоскостей.
- Верификация 6.4c: `tests/router_fallback_smoke.sh` даёт concurrent exact-signal load, strict и
  provider filtering до execution, unsigned-terminal и cached-catalog ConnectionRefused cases с
  точными counter deltas. `tests/router_fallback_live_canary.sh` запускает ровно deployed router
  binary отдельным процессом, использует только существующий stdin-delivered key и повторяет matrix
  на реальных secondary attempts; GA-флаг запрещён до чистых double-winner/divergence/backlog
  доказательств.

## 7. Фазировка (каждая фаза — отдельный пакет через merge-конвейер)

1. **6.1 — контракт `not_started` в плоскостях — РЕАЛИЗОВАН 2026-08-01** (header-strip в
   router для транзитных ответов даже без fallback, unit/contract-тесты веток с реальным
   reserve, документация `crates/forward/CLAUDE.md` + `crates/router/CLAUDE.md`). Выкат
   при выключенном fallback безопасен: клиент заголовок не видит. Gemini Messages skin
   и четыре universal Chat/Responses adapter-поверхности покрыты правилами §3.2; signal-
   propagation зазор закрыт до включения fallback в 6.2.
2. **6.2 — router fallback engine — РЕАЛИЗОВАН 2026-08-02:** поле `models`, единый
   preflight/rewrite engine для Chat/Responses/Messages/count_tokens, retry matrix §3.3,
   безопасное логирование attempts, feature-flag off-by-default; TCP mock-тесты двух
   плоскостей покрывают exact signal, 429, unsigned 5xx, 400/402, ConnectionRefused,
   timeout, malformed/duplicate/unknown models и снятие внутреннего header.
3. **6.3 — group identity в registry/billing — РЕАЛИЗОВАН 2026-08-02:** migration-first
   schema 0021, trusted router headers, group-aware scalar/legacy/strict reserve, transactional
   insert-first-wins settle в SQLite/PostgreSQL, safe retention, fault-matrix и always-zero alert.
4. **6.4 — policies/presets + telemetry GA — 6.4a–6.4b И TELEMETRY/MOCK-LOAD 6.4c
   РЕАЛИЗОВАНЫ 2026-08-02:**
   producer-first policy preflight одинаково доступен на всех fixed planes и покрыт bounded
   validation, auth-lattice и real-SQLite strict-policy тестами; router consumer применяет
   preferences/presets и точный policy subset до attempt 1. Counters, Prometheus alerts/runbooks,
   mock-load и credential-safe live runner готовы. Остаются post-deploy live canary и отдельное
   включение production-флага; до них fallback остаётся default-off.

## 8. Отвергнутые варианты

- **Retry на timeout/5xx без сигнала плоскости** — прямой путь к двойному списанию
  (`UNIFIED_ROUTER.md`: «молчаливый retry на timeout — путь к двойному списанию»).
- **Буферизация ответа в router для самостоятельного определения started** — нарушает
  инвариант byte-passthrough и раздувает router до второго движка (решение 1).
- **Единый request_id сквозь плоскости (одна попытка перезаписывает резерв другой)** —
  ломает exactly-once ledger и аудит попыток; group/attempt модель строго надстройка.
- **Кворум/hedged requests** — вне scope (§1): расходует capacity и баланс на каждый запрос.

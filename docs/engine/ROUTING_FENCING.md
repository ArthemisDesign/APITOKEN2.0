# ROUTING_FENCING.md — детальный дизайн этапа 6 UNIFIED_ROUTER (routing + attempt fencing)

Статус: design, первый пакет этапа 6 (по решению 7 `docs/engine/UNIFIED_ROUTER.md` —
«детальный дизайн routing'а — первым пакетом после зелёных этапов 3–5, на живой телеметрии
universal lanes»). Реализация следует этому документу; отклонение требует его пересмотра.

Дата фактбазы: 2026-08-01 (аудит кода на master после этапа 4.3; этап 5.1 в конвейере).
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

### 3.3. Обязанности router (фаза 6.2)

Retry на следующую модель fallback-списка разрешён РОВНО в двух случаях:

1. Ответ плоскости не-2xx С заголовком `x-apitoken-execution-state: not_started` (header
   снимается, тело ошибки этой попытки логируется, клиенту уходит ответ последней попытки).
2. TCP connect-refused к плоскости (запрос физически не ушёл).

Запрещено: retry на timeout, на 5xx БЕЗ заголовка, на обрыв после заголовков, на 402
(баланс аккаунта — повтор на другой модели той же учётки бессмысленен), на 4xx клиента.

## 4. Execution group / attempt identity (зрелая версия, фаза 6.3)

MVP-контракт §3 закрывает гонку «вторая попытка стартовала, пока первая billable» только при
исправном сигнале. Durable-гарантия против бага/рассинхрона — group identity:

- **Router генерирует** `group_id` (UUIDv4) на логический запрос с fallback-списком и шлёт
  плоскости `x-apitoken-execution-group: <group_id>` + `x-apitoken-attempt: <N>` (N = 1,2,…
  по порядку обхода списка). Без fallback-списка заголовки не выставляются — плоскость
  работает как сегодня (group = request_id).
- **Registry (expand-only миграция):** `reservations` получает колонки `group_id TEXT NOT
  NULL DEFAULT request_id`, `attempt INTEGER NOT NULL DEFAULT 1`; новая таблица
  `execution_group_winner(group_id TEXT PRIMARY KEY, winner_request_id TEXT NOT NULL,
  decided_at INTEGER NOT NULL)` — одна строка на группу, insert-first-wins.
- **Settle path:** nonzero settle (charge > 0) атомарно (в той же БД-транзакции) делает
  `INSERT INTO execution_group_winner … ON CONFLICT DO NOTHING` и читает победителя:
  - winner == мой request_id → обычный settle;
  - winner != мой request_id → двойное исполнение обнаружено durable: charge принудительно
    0 (refund), фатальный structured event `execution_group_double_winner` + метрика
    (должна быть 0 всегда; >0 = баг контракта §3, инцидент).
  Refund-settle (charge == 0) winner не назначает.
- **Инвариант exactly-once не ослабляется:** существующий `UNIQUE ledger(kind, request_id)`
  остаётся per-attempt; winner-правило добавляет «ровно один nonzero winner на группу».
- Миграции — expand-only, двумя коммитами по `AGENTS.md`: сначала схема (колонки с default,
  новая таблица), код — после зелёных `deploy/migration` + `deploy/watchdog`.

## 5. Routing-интерфейс router (фаза 6.2)

- Новое необязательное поле запроса `models: [<catalog id>, …]` (OpenRouter-совместимое
  соглашение; expand-only контракта universal endpoint — старые клиенты не затронуты).
  `model` остаётся обязательным и трактуется как первый элемент цепочки; `models` задаёт
  продолжение. Пустой список/дубликаты/неизвестные id → `400` в конверте lane входного пути.
- Router буферизует только тело запроса (как сегодня, 32 MiB), выбирает плоскость каждой
  попытки независимо (namespace/alias — существующий `catalog::namespace_lane`); retry —
  только по правилам §3.3; ответ клиенту — последней попытки (успех или её ошибка),
  in-flight ответ НЕ буферизуется (инвариант byte-passthrough не затрагивается: retry
  возможен только до первого байта).
- `provider` preferences-объект (order/allow/sort по цене-латентности) — НЕ в этой фазе;
  отдельный пакет после живой телеметрии fallback.
- Per-account policy: существующий substrate `crates/registry/src/pricing.rs` (provider
  switches, account policy) — fallback-цепочка фильтруется policy аккаунта ДО первой
  попытки; пустая после фильтрации → `400`/`403` с объяснением.

## 6. Телеметрия и верификация

- Новые счётчики: `claude_router_fallback_total{from_namespace,to_namespace,reason}`
  (reason: `not_started`/`connect_refused`), `claude_api_execution_not_started_total{plane}`,
  `claude_api_execution_group_double_winner_total` (always-zero алерт).
- Групповые лейблы на существующих денежных рядах НЕ добавляются (кардинальность); group_id —
  только в structured-логах попыток.
- Детекторы регрессий: `apitoken_balance_divergence_nano` (существующий),
  `EngineSettlementBacklog`, `EngineExpiredLeasePresent` — проходят нагрузочный период с
  включённым fallback до GA.
- Флаг rollout: fallback выключен по умолчанию (`ROUTER_FALLBACK_ENABLED=false`), включается
  config-флагом на canary; `deploy`-чеклист — измерение доли ambiguous-исходов (timeouts без
  заголовка) до и после включения.
- Верификация fencing: fault-injection тесты плоскостей (симулированный баг сигнала → winner
  conflict → charge 0 + алерт), интеграционный тест router: две плоскости, первая отвечает
  not_started → вторая billable, ledger содержит ровно один charge на group.

## 7. Фазировка (каждая фаза — отдельный пакет через merge-конвейер)

1. **6.1 — контракт `not_started` в плоскостях** (+ header-strip в router для транзитных
   ответов даже без fallback, unit/contract-тесты веток, документация). Безопасно выкатить
   при выключенном fallback: клиент заголовок не видит.
2. **6.2 — router fallback engine:** поле `models`, retry matrix §3.3, логирование попыток,
   feature-flag off-by-default, интеграционные тесты с двумя mock-плоскостями.
3. **6.3 — group identity в registry/billing:** expand-only миграции, winner-правило в
   settle, fault-injection тесты, always-zero алерт.
4. **6.4 — policies/presets + telemetry GA:** фильтрация цепочек policy, presets в каталоге,
   включение флага по расписанию, нагрузочный прогон, финализация документации.

## 8. Отвергнутые варианты

- **Retry на timeout/5xx без сигнала плоскости** — прямой путь к двойному списанию
  (`UNIFIED_ROUTER.md`: «молчаливый retry на timeout — путь к двойному списанию»).
- **Буферизация ответа в router для самостоятельного определения started** — нарушает
  инвариант byte-passthrough и раздувает router до второго движка (решение 1).
- **Единый request_id сквозь плоскости (одна попытка перезаписывает резерв другой)** —
  ломает exactly-once ledger и аудит попыток; group/attempt модель строго надстройка.
- **Кворум/hedged requests** — вне scope (§1): расходует capacity и баланс на каждый запрос.

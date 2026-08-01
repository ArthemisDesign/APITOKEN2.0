# crates/forward — CLAUDE.md

**Роль:** прозрачный форвардинг Claude `/v1/*` на api.anthropic.com (Шаг B) + поллер лимитов;
отдельно — optional strict OpenAI-compatible text adapter поверх encrypted ChatGPT OAuth roster
(native HTTPS к Codex backend) и native Gemini surface поверх encrypted Code Assist OAuth pool.
Три provider path не смешивать.

**Владелец-ветка:** `comp/forward`.

**Границы (жёстко):**
- Зависит от `pool`, `registry`, `metering`, `axum`, `wreq`, `redis`, `serde_json`, `futures-util`,
  `bytes`, `tokio`[sync,rt] + `anyhow` (для DB-актора биллинга).
- НЕ читает env и НЕ содержит CLI/управляющих роутов (`/health`, `/pool`, `/balance`) — это `server`.
- Конфиг получает готовым: [`ProxyConfig`] наполняет `server::config`; биллинг — async DB-актор `Option<Arc<AsyncBilling>>` в `AppState` (1 writer + N readers).

**Три класса авторизации (разделение секретов, `proxy.rs`):** `authed` (forwarding-admin: `api_keys`
/loopback) ⊂ `control_authed` (+`control_keys` — для `/admin/*` коммерции) ⊂ `readonly_authed`
(+`panel_keys` — read-only дашборды). Все — constant-time (`ct_eq`, fold без short-circuit). Control-
ключ НЕ форвардит `/v1` (не админ, не метерный → 401). `AsyncBilling` расширен control-командами
(`create_account`/`issue_key`/`account_status`/`key_status_by_id`) через ТОТ ЖЕ single-writer (без гонок).
Pricing sync uses the same actors: multiplier writes go through the writer and cursor ledger reads
through a reader; HTTP code never opens the authority directly.
Stage 3C versioned pricing control follows the same ownership: catalog/switch/policy prepare and
activate commands share the single writer, while immutable-version/head/bundle reads use the normal
bounded reader pool. SQLite and PostgreSQL dispatch the same registry typed outcomes; no HTTP
handler opens a second connection or assembles a policy bundle from separate reads.
Credential в `x-api-key`, `x-goog-api-key` и `Authorization: Bearer` имеют OR-семантику без
приоритета заголовка: достаточно любого валидного. Это критично для Claude Code,
который может одновременно прислать stale `ANTHROPIC_API_KEY` и актуальный `ANTHROPIC_AUTH_TOKEN`.

**Multi-provider pricing Stage 3B1b/3B1c (`pricing.rs`, `pricing/shadow.rs`,
`pricing/runtime.rs`):** pure
fail-closed resolver consumes
one transactionally materialized `registry::pricing::PricingReadBundle` (including the live legacy
scalar, exact policy dependencies and current admission heads), provider-fixed identities and a
runtime-owned manifest of exact `(schema, capability generation, digest)` tuples. Policy and
admission catalog/switch gates are independent: current heads need not equal policy pins, and a
mixed `C2/S1/P1` rollout must keep the old common model available while the policy lineage still
blocks a new C2-only model. The S1 catalog pin is accepted beside C2 only while it matches the
policy's C1 catalog; malformed `C2/S1/P2` fails closed. Exact model rule replaces provider rule.
Resolved output preserves both lineage pairs plus manifest identity;
malformed/missing/schema/capability/model/switch failures use stable typed reasons. A separate pure
work-item pins only the validated actual snapshot reference, full registry-canonical manifest
evidence and explicit enqueue timestamp. Its builder derives request/manifest identity internally,
resolves exactly one coherent bundle, verifies manifest/provider/model identity and converts all
resolved/rejected/read-error variants into a validated immutable registry input. It rejects early
timestamps and actual holds above the checked scalar quote before enqueue. A lower actual is an
exact funding ceiling shared by scalar and policy candidates and remains eligible. A bundle for
another outer account is an integrity error, not a durable rejection carrying that account's
scalar. The modules have no
HTTP/env and never feed shadow output into admission, reserve, settlement or `/ready`. The
default-off runtime producer is called only after successful atomic Anthropic/OpenAI snapshot
reserve, applies deterministic sampling, byte limits and an integer token bucket, then performs
exactly one `try_send`. Its bounded PostgreSQL-only workers use separate read actors, the existing
billing writer for immutable insert, per-operation PostgreSQL timeouts, queue expiry `<24h` and
fixed-cardinality metrics. Full/closed/rate-limited/oversized work drops fail-open;
read/write/timeout/replay/conflict outcomes never change customer response or money. SQLite keeps
API/test parity but cannot start live shadow readers.

**Биллинг (async, `billing.rs` + tee-метеринг `meter.rs`):** авторизация (`authorize`, async):
env-админ проверяется ПЕРВЫМ в памяти; иначе клиентский ключ → `key_account` (JOIN ключ→аккаунт)
→ баланс АККАУНТА (≤0 → 402). Баланс/резерв/наценка — на аккаунте (общий на все ключи юзера).
Все DB-операции идут через `AsyncBilling` (DB-акторы: 1 writer + N readers; sync PostgreSQL/legacy
SQLite живут на выделенных потоках, НЕ на async-воркерах). Generated request ID создаётся до reserve;
успешная доставка помечается durable до передачи стрима; finalize кладёт idempotent settlement в
outbox, а writer retry-ит до commit. RAII cancel закрывает именно этот request ID. Резерв под баланс
с урезанием `max_tokens` (`cap_to_balance`)
→ клиент не получит ни токена/цента сверх баланса. 4xx/ошибки/ротация НЕ тарифицируются.
Для policy-ключей cap берёт минимум из баланса аккаунта и оставшегося lifetime-лимита. Такие ключи
обходят auth TTL cache; срок и лимит повторно проверяются в атомарной транзакции reserve.
Pricing shadow adds a separately sized PostgreSQL read-actor pool; evaluation inserts remain on the
same single writer and deliberately do not use the normal five-second money-operation retry loop.

**Stage 3B1c.2 atomic legacy snapshot bridge — live caller, default-off:** отдельный
`ReserveWithLegacySnapshot`/`reserve_request_with_legacy_snapshot` передаёт writer'у готовый owned
typed snapshot как единственный источник request/account/hold и вызывает guarded registry commit.
Его guard может отменить только `PENDING → CANCELED` до commit gate. После
`COMMIT_DECIDED` компенсационный `CancelReserve` запрещён: lost reply оставляет active reservation
для exact replay либо штатного lease recovery, без terminal reservation/outbox. PostgreSQL
повторяет transient operation только до commit decision; неоднозначная commit-ошибка возвращается
как ошибка и разрешается последующим exact replay. Существующий scalar `Reserve` и его прежняя
RAII-компенсация не изменены. Bridge preflight использует validated config
(`disabled/0` или `sampled/1..=10000 bp`), SHA-256 v1 sampler по trusted fixed provider и внутреннему
canonical lowercase UUIDv4 request ID, stable typed decisions/reasons. Sampler не читает clock/DB,
а provider-owned builders рядом с текущими
legacy quote implementations сами выводят canonical/tariff/modifier identity через `metering` и
строят validated snapshot из одного frozen timestamp. Anthropic builder вызывает неизменённый
`cap_to_balance`, OpenAI pricing builder — неизменённый `reserve_cost`; provider/canonical/tariff и
hold caller не задаёт.

Live metered Anthropic/OpenAI admission теперь применяет sampler до денег. Disabled/not-sampled и
typed pre-money fallback идут в byte-equivalent scalar reserve без snapshot; selected request
атомарно сохраняет reservation+actual snapshot. После выбора atomic path invariant/DB/handoff или
idempotency conflict fail closed без второго scalar reserve. Успешный hold продолжает прежний
mark-delivering/cancel/settlement lifecycle. Default config остаётся `false/0`; включение требует
явного bounded sample. Метрики имеют только фиксированные provider/reason labels и fixed-bucket
atomic reserve latency histogram. Gemini, policy read/resolver, shadow queue, readiness и
settlement pricing этим caller не затронуты.

**Что внутри:** `ProxyConfig`, `AppState`, `Clients` (кэш http-клиентов по прокси),
`limits_from_headers`/`Limits` (unified-ratelimit из ответа), `poll_sub` (активный опрос idle),
`detect_plan` (тариф из /api/oauth/profile), `forward` (axum-хендлер), `authed`;
`codex/` содержит native HTTPS transport (`transport.rs`), profile pool (`mod.rs`),
Responses/Chat adapters, tenant-bound history, Codex admission/settlement и reconstruction SSE
events; `gemini/` — native route allowlist, encrypted OAuth pool, Code Assist translation и
settlement. Env для обоих читает только `server::config`.

**Cache-first роутинг без client opt-in (`affinity.rs`):** tenant = metered `account_id` (все ключи
аккаунта разделяют кэш) или отдельный admin scope. `AffinityStore::infer` считается ДО identity-инжекта.
Strong session IDs принимаются из Claude Code/generic session-conversation-thread headers и одноимённых
top-level/`metadata` body-полей; они нормализуются в keyed digest и являются ЖЁСТКОЙ границей: новый ID
никогда не наследует transcript/root другой сессии. Без ID строятся rolling keyed-хэши каждого
канонического message-prefix вместе с cache-shape (`model/system/tools/thinking/context_management`),
поэтому растущий classic API history находит самый глубокий известный префикс без разбора ответа.
Большой/явно cache-controlled общий `system+tools` хранится ОТДЕЛЬНО как soft cache-root → множество
тёплых homes (5m/1h TTL из cache_control), записывается только после upstream 2xx и никогда не разрешает
conversation. Pool сначала прогревает два конкурентных home, затем выбирает тёплый, пока его свободная
ёмкость не хуже 70% лучшей; так общий system/tools не сваливает независимые сессии на один аккаунт.
Local L1 всегда включён; optional Redis L2 делит TTL-привязки и ZSET тёплых homes между slots. Redis
хранит только keyed digests tenant/native/transcript/subscription и fail-open: сеть/timeout/eviction не
участвуют в auth, money или capacity. Первая попытка = `pool.route_affinity`
(place/pin/brief wait/spill/rebind), ретраи = `pool.pick`. PostgreSQL capacity lease ниже остаётся
авторитетным. SSE по-прежнему byte-for-byte.
In-flight держится всю жизнь стрима: успех → `mark_healthy`, `end_stream` из tee-метеринга (`meter.rs`)
снимает слот на завершении/обрыве; 4xx → `mark_ok`.

**Ротация/лимиты (устойчивость пула):**
- **Пассивный сбор:** на КАЖДОМ ответе апстрима вытаскиваем unified-ratelimit (`limits_from_headers`)
  → `pool.set_util`. Так util/reset всегда свежи из боевого трафика; активный `poll_sub` (server)
  добивает лишь простаивающие подписки (обновлённый `polled_ts` сам это гейтит). Экономит квоту.
- **Классификация вины (не студить подписку за чужое):**
  - `429` → квота подписки → `mark_cooling(cool_secs_429)`: `Retry-After` → окно-виновник
    (`util7d≥0.95` → `reset7d`, иначе `reset5h`) → burst-дефолт. Не студим на 5h, если выбит 7d.
  - `401/403` → мёртвый/битый токен, НЕ транзиент → `mark_cooling(AUTH_QUARANTINE=900s)` + лог
    «нужен refresh». Иначе долбили бы забаненный аккаунт раз в 10с (ban-signal) и жгли слот попытки.
  - `5xx/408/409/425/сетевой` → вина АПСТРИМА → `mark_done` (слот −1 БЕЗ cooling: подписка здорова)
    + `breaker.record_fail`. Битый прокси (build err) → короткий `mark_cooling(10)` (локально).
  - `2xx/4xx` → апстрим здоров → `breaker.record_ok` (сброс окна фейлов).
- **Circuit breaker (`breaker.rs`):** размыкается, когда в окне зафейлило ≥ N РАЗНЫХ подписок
  (distinct email, а не сырой счёт) — это признак аутейджа api.anthropic.com, тогда как одна
  флаки-прокси/poison даёт фейлы одного email и breaker НЕ трогает. Пока разомкнут — вход отбивает
  `503 + Retry-After` (не веерим по пулу). `record_fail(now, email)`; на 2xx/4xx `record_ok` сбрасывает.
- **Бюджет ротации (ошибка подписки клиенту НЕ идёт):** 429/401/403 — вина конкретного аккаунта
  (бан/лимит), бюджет `max_tries` НЕ тратят → крутимся по всему флоту (пул сам исключает cooling), пока
  не найдём здоровую. Бюджет тратят только BACKEND-фейлы (5xx/сеть — аутейдж). Верхний предел итераций
  = «весь флот + запас».
- **Исход при провале всех попыток:** упёрлись в backend-бюджет → отдаём последнюю upstream-ошибку
  (аутейдж; breaker вот-вот разомкнётся); все подписки за лимитом → `429 + Retry-After = soonest_ready`
  (клиент откатится сам — именно это, а не ошибка отдельной забаненной подписки); пул пуст → `503`.
  Каждый `mark_used` парен с `InflightGuard`/`end_stream`/`mark_done`.

**Антифингерпринт флота (`persona_ua`):** флот из 100 байт-в-байт одинаковых UA — сам по себе
отпечаток. `persona_ua(cfg, email)` даёт **стабильный во времени** для подписки, но **различный между
подписками** UA: пул задан списком (`user_agents` len>1) → пиним по hash(email); иначе варьируем
patch-версию базового UA на `ua_spread`. Клиентский `user-agent` НЕ пробрасываем (в `skip_req_header`)
— отпечаток наш. Тот же UA идёт и в `poll_sub`/`detect_plan` (здоровье персоны = тот же отпечаток,
что и бой). Identity/beta/anthropic-version НЕ варьируем — они корректностные (нет ground-truth на
правдоподобные альтернативы). Env: `CLAUDE_API_UA` (один или список), `CLAUDE_API_UA_SPREAD`.

**Fair-share (`keylimiter.rs`):** `KeyLimiter` кап-ирует ОДНОВРЕМЕННЫЕ запросы на метерный ключ
(`max_inflight_per_key`, деф 20; 0=выкл) — баланс ограничивает суммарный расход, но не одновременность,
и без этого один «кит» бёрстом залил бы флот, оставив остальных на 429. Слот держит `KeyGuard` всю
обработку запроса (освобождает на любом исходе/отмене). Превышение → `429 slow down` + метрика
`key_throttled`. Админ (env-ключ/localhost) — без лимита.

**RAII-гарды на отмену запроса (критично):** клиент рвёт соединение → future хендлера дропается на
`await`; без гардов `mark_used(+1)` и `reserve(hold)` НЕ откатывались бы (утечка ёмкости персоны +
денег клиента навсегда). `InflightGuard` (Drop → `mark_done`) закрывает слот на любом не-стриминговом
исходе И при отмене; разоружается на успехе (слот держит стрим → `end_stream`). `HoldGuard` (Drop →
`settle(hold,0)`) возвращает резерв на любом не-успешном исходе И при отмене; разоружается на успехе
(hold закрывает tee-метеринг фактикой). Поэтому `mark_cooling`/`mark_healthy` in-flight НЕ трогают —
владелец слота один. Breaker кормится максимум раз на запрос (анти-DoS от poison-запроса).

**Инварианты прозрачности (критично — не ломать):**
1. Ответ апстрима отдаётся клиенту **байт-в-байт** (включая SSE-стрим). Не буферизировать,
   не переписывать тело.
2. Под капотом: инжект Claude Code identity ПЕРВЫМ system-блоком + `anthropic-beta: oauth-…` +
   `Bearer` подписки. Клиентский `system` сохраняется вторым блоком. Без identity Anthropic не
   пускает OAuth-токены подписок — но клиент об этом знать не должен.
3. **Оборванный SSE закрываем кадром `event: error`** (`SseErrorTail`), а не молчаливым усечением:
   для SDK остановившийся поток неотличим от завершённого, и многие клиенты на нём висят. Кадр
   входит в протокол Anthropic, поэтому это НЕ отход от байт-в-байт прозрачности, а её
   восстановление — настоящий апстрим прислал бы ровно его. Обёртка самая внешняя, чтобы метеринг
   не видел синтетических байт (хвост отказа — не usage), и текст обезличен, как в local_err.
4. **Ротация только ДО начала стрима:** решение по статусу (429/401/403/5xx → cooling + следующая
   подписка) принимается до отдачи тела. Как только начали стримить — не переключаемся.
4. Клиентские ошибки запроса (400/404/422 …) пробрасываются как есть, БЕЗ ротации.
5. Заголовки авторизации клиента (`x-api-key`/`authorization`) НЕ уходят апстриму — заменяются
   на Bearer подписки. Токены не логировать.
6. **Санитайзер синтетических ошибок (`LocalErr`/`local_err` в `proxy.rs`) — ЕДИНСТВЕННАЯ точка,
   где рождаются НАШИ ответы клиенту.** Клиент считает, что говорит с api.anthropic.com, поэтому в
   `error.type`/`message` НЕ должно быть наших внутренностей (`subscription/pool/upstream/authority/
   cooling/persona/fleet/oauth`). Внутренняя причина живёт только в метриках и `eprintln`-логе, НЕ в
   теле. Публичные триплеты — аутентичные Anthropic: `overloaded_error`=**529**, `api_error`=500,
   `rate_limit_error`=429, `authentication_error`=401, `not_found_error`=404, `request_too_large`=413.
   Нехватка ёмкости/пул пуст/breaker/authority/сбой апстрим-соединения → обезличенный retryable
   `Overloaded`/`RateLimited`. Законные ошибки состояния аккаунта клиент ЗНАТЬ должен и они остаются:
   `InvalidKey` (401), `LowBalance` (**402**, контракт docs-portal). Новую ошибку добавляй ТОЛЬКО как
   вариант `LocalErr` (не сырой `err_response`); regression-тест `local_err_never_leaks_*` это гейтит.

**Инварианты native Codex gateway (та же планка, что у Gemini):**
0. **Пул профилей = sealed roster, без дочерних процессов.** Каждый home — AEAD-конверт
   (`codex-credential`, XChaCha20Poly1305, profile id как AAD) с OAuth-материалом ChatGPT
   (access/refresh token, account_id, план, прокси). Roster — `profiles.json` +
   `credentials/<id>.json`; symlink/другой path/duplicate id запрещены. Нативный HTTPS к
   `chatgpt.com/backend-api/codex` через per-profile wreq-клиент со своим прокси: никаких
   supervised child, pinned binary, ownership locks и ownership transition'ов — blue-green
   поколения свободно пересекаются, потому что состояние живёт в roster, а не в процессах.
   Service floor — один рабочий профиль: одна подписка не становится 503 из-за отсутствия
   запасной. **Параллелизм на home НЕ ограничен** (как у Claude-флота): атомарный счётчик
   in-flight (`TurnSlot` RAII) — лишь сигнал загрузки для выбора, не потолок.
   **Выбор — cache-first (как `affinity.rs`):** preferred home разговора → warm-прогрев общего
   cache-root на двух homes → наименее загруженный; равные кандидаты чередуются атомарным
   cursor. После успеха home пишется обратно в affinity. Selection: свежесть quota-снапшота →
   in-flight → remaining window (bucketed steering ≥50%) → cursor; hard-exclusion ТОЛЬКО по
   явному вердикту провайдера (`limit_reached` / `allowed:false` / 429) — проверено живьём:
   `usedPercent=100` при `allowed:true` обслуживает; отвод от почти-полных окон — задача
   reserve-кепок. Все homes за лимитом → один OpenAI-shaped 429+Retry-After до ближайшего reset.
   **Мягкий запас окон (как `pool::Reserve`):** не роутим выше `1−base` окна (5h деф 10%,
   weekly деф 3%) с детерминированным джиттером по profile id; под пиком фильтр fail-open
   ослабляется до провайдерской стены. Кэш-липкость: tenant-scoped affinity выводит стабильные
   opaque `prompt_cache_key` и session/thread/window UUID, поэтому разговор выглядит одной
   непрерывной сессией и не раскрывает raw customer key.
1. **Только AEAD envelopes и pinned official client identity.** `originator: codex_cli_rs`,
   UA `codex_cli_rs/<CODEX_CLI_VERSION> (…)`, `version`, `ChatGPT-Account-ID` из envelope;
   turn также несёт first-party-shaped session/thread/window/turn metadata в headers и body.
   Tokens/account_id/proxy и полный email дешифруются только в память и не попадают в
   log/metric/response. Control-authenticated `/codex-subs` может получить только bounded email hint
   (первые четыре символа local-part без домена) и reviewed paid-plan identity для операторского
   сопоставления/агрегации; homes по-прежнему
   адресуются opaque id (в логах/метриках нет путей и identity). Версия клиента движется только
   reviewed-коммитом после live probe
   (`research/CODEX_NATIVE_WIRE.md`).
2. **Refresh — single-flight с durable ротацией (критично, отличие от Gemini).** OpenAI вращает
   refresh_token на КАЖДОМ refresh со strict reuse detection: credential mutex сериализует
   проверку expiry и refresh (401-бёрст переиспользует победителя), ротированный envelope
   атомарно перезапечатывается (tmp+rename) ДО отпускания блокировки. При `invalid_grant` —
   ровно один reload envelope с диска (blue-green peer мог ротировать раньше) и один retry.
   Первый 401 на turn → один force-refresh+retry того же home до первого байта; повторный 401 →
   auth quarantine по health-политике.
3. **Model-visible context = explicit client base/developer instructions + replayed Responses
   items + client tools.** Body собирается конструкцией (`build_responses_body`): личности,
   environment/project/plugin/skill/permission контекста и built-in tools просто не существует
   — граница патча app-server теперь структурная. `store:false`, stateless полный input на
   turn; tenant continuity — `prompt_cache_key` digest плюс выведенные из него opaque
   session/thread/window UUID (никогда raw customer key).
   Custom tool выполняет клиент: gateway возвращает raw call item и никогда не исполняет его.
4. **Провайдерские окна — из `/wham/usage` и live-заголовков/SSE `codex.rate_limits`.**
   Снапшот принимается только с реальными duration+reset; stale не отклоняет и не выигрывает
   тай-брейк, никогда не приходивший равен свежему. Схема `/wham/usage` и имена заголовков
   зафиксированы по живому probe (research/CODEX_NATIVE_WIRE.md, 2026-07-31). Свип селективен:
   занятые homes кормятся
   живым трафиком, здоровые idle — на медленном floor-каденсе, stale/suspect/unprobed — каждый
   тик, всё с bounded concurrency (sweep не должен сам стать upstream-нагрузкой на флоте).
   Провалившийся turn будит свип немедленно (`probe_poke`, как `request_probe` у Claude).
   Калибровку питают ТОЛЬКО wire-события (probe/turn headers): чтения не пишут, роутинг не
   стоит DB-работы.
5. **Ретрай только ДО первого байта:** `emitted`-флаг — как только delta ушла клиенту, вторая
   попытка запрещена. Классификация вины: 429/usage-limit → вина АККАУНТА (cooling до reset,
   бюджет ретраев не тратят), 401/403 → auth (refresh+retry один раз, потом quarantine 900s),
   timeout/5xx/EOF → вина ТРАНСПОРТА (streak → degraded → wedged → rebuild клиента),
   400/context → вина КЛИЕНТА (не студим, не ретраим). Все homes за лимитом → один
   OpenAI-shaped 429 с ближайшим reset. Health — чистая политика в `health.rs` по двум осям
   (account healthy→suspect→dead; transport responsive→degraded→wedged), durable account axis в
   authority.
6. **Калибровка ёмкости окна — fixed-point workload blend по фактическим данным.** Decimal
   `used_percent` из `/wham/usage`/headers парсится без `f64` в `10^-8` fraction units; каждый
   успешный turn durable-кредитует home exact official-price cost в integer nanoUSD. Estimator v8
   считает `cap=100_000_000*ΣΔspend_nano/ΣΔused_fraction_units`: это API-USD-equivalent реально
   обслуженного mix моделей/context/reasoning/tools, а не выдуманный номинал подписки. Per-interval
   ±1-unit envelope даёт low/high, confidence = maturity × workload stability × quantisation.
   Нет prior/EMA/WLS/float-money. Cold snapshot сам по себе остаётся только anchor, но первое
   подтверждённое движение с положительным settlement сразу считается complete interval с
   quantisation envelope; если settlement запаздывает, движение ждёт его catch-up. Raw
   observations также позволяют распознать rolling weekly reset по совместному сигналу
   material forward reset-at shift + utilisation rollback, даже если shift меньше пол-окна;
   bounded reset-at jitter сам по себе окно не форкает. Exact cumulative legs и CAS-state в
   engine authority переживают restart/blue-green/reset и позволяют replay при смене estimator;
   каждое provider-reported duration калибруется независимо.
7. **Цены — только из `metering::codex`** (audited, effective-dated). Для успешного ChatGPT-auth
   turn effective tier определяется принятым запросом: `priority` = Fast, отсутствие tier =
   Standard. Completed `response.service_tier` хранится только как provider-reported диагностика:
   официальный backend обычно возвращает `default` и на измеримо ускоренном Fast. Reserve держит
   консервативный Fast-резерв; settle/ledger/capacity/публичный ответ используют effective tier.
   Public
   synthetic errors только
   OpenAI-shaped и без pool/profile/upstream internals — гейтит
   `api::tests::public_errors_never_leak_internal_architecture`.
8. **Shutdown:** detached streaming tasks входят в shutdown-barrier до history+settlement;
   `TurnEvents` Drop abort-ит upstream read, settle последнего snapshot — до освобождения
   background permit. Полный контракт/provisioning/runbook — `docs/engine/CODEX_PROVIDER.md`.

**Инварианты native Gemini gateway:**
1. Только AEAD envelopes проверенных paid Code Assist OAuth identities. Roster содержит opaque id и
   строго `<roster>/credentials/<id>.json`; symlink/другой path/duplicate Google subject запрещены.
   Runtime повторно проверяет official OAuth client/token endpoint, exact plan↔tier-label mapping,
   paid-plan allowlist и canonical proxy uniqueness (включая equivalent percent encoding).
   Tokens/email/project/tier/proxy дешифруются только в память и не попадают в log/metric/response.
2. `GeminiGateway` обслуживается только startup-fixed `ProviderMode::Gemini`. Native allowlist:
   models get/list, generateContent, streamGenerateContent, countTokens. Клиентский `x-goog-api-key`
   (как и x-api-key/Bearer) авторизует наш ключ, но никогда не уходит Google; query `key`/`api_key`,
   включая percent encoding, запрещён.
3. Production HTTPS принадлежит persistent per-profile Node helper: exact pinned
   `/usr/bin/node` v24.18.0 Linux/x64 + SHA-256, native OpenSSL, HTTP/1.1 и authenticated CONNECT.
   Новые profiles используют Antigravity 2.2.1 UA, `Go-http-client/2.0` refresh и reviewed bounded
   Antigravity `Client-Metadata`/`x-goog-api-client`; caller values вырезаются. Старые Gemini CLI
   credentials сохраняют прежний wire до миграции.
   OAuth userinfo использует отдельный global-fetch/Undici профиль того же SHA-pinned Node. Никакой
   approximate BoringSSL impersonation или ambient proxy/env.
   Antigravity text сохраняет live-verified configured endpoint; image generation всегда идёт на
   production `cloudcode-pa.googleapis.com`, как официальный LS: sandbox публикует image quota row,
   но отвечает 503 на генерацию. Literal loopback mocks не перенаправляются.
   Helper получает proxy secret только первым IPC frame, multiplexes bounded NDJSON, reaps process
   group и может restart-нуться только до upstream headers. Outbound frames, inbound NDJSON/base64
   staging, OAuth response collections и short-lived header/form strings zeroized. Loopback mocks
   остаются на `wreq`.
4. Профиль владеет отдельным transport/proxy/inflight/cooling/auth и single-flight token refresh.
   Первый 401 → один refresh+retry того же profile; повторный 401/403 → auth quarantine. 429 →
   model-specific profile cooling по Retry-After/RetryInfo/quota reset и ротация без
   transport-бюджета; health probe не стирает generation cooling. Antigravity
   `fetchAvailableModels` публикует sanitized model catalogue: explicit zero блокирует модель до
   reset, stale/missing bucket fail-open. Legacy profiles продолжают `retrieveUserQuota`.
   Network/token refresh/408/409/425 → короткий global-profile cooling. Generation 5xx/malformed
   response → exponential model-specific cooling и bounded retry, чтобы одна модель не выключала
   остальные модели профиля; остальные 4xx не вращаются.
   Если были quota failures — итог 429; только auth/transport failures — 503; уже cooling pool — 429.
5. Code Assist request wrapper строится сервером; caller не может inject project/session identity.
   Для Antigravity text generation `request.sessionId` — UUID из keyed tenant-scoped affinity
   lineage, а top-level `requestId=agent-<uuid>` создаётся один раз до rotation; wrapper также
   фиксирует `userAgent=antigravity` и `requestType=agent`. Image generation сохраняет только
   public affinity, но private wire обязан быть stateless: без `request.sessionId`, с
   `requestType=image_gen`, `requestId=image_gen/<unix-ms>/<uuid>/12`, `candidateCount=1` и
   `responseModalities=[TEXT,IMAGE]`. Resolution allowlist private subscription surface — только
   live-verified `1K`/`2K`/`4K`; Developer API-only `0.5K` fail-closed до отдельного live evidence.
   Legacy profiles сохраняют `request.session_id` и
   `user_prompt_id=<session UUID>########<human-turn ordinal>`.
   Public Gemini разрешает пустой/пропущенный `contents[].role`; для строгого private Antigravity
   wire wrapper выводит только такие роли чередованием `user`/`model`, не переписывая явные значения.
   Публичный model ceiling 65,536 сохраняется, но Antigravity wire `maxOutputTokens` ограничен 65,535.
   Canonical Gemini 3 model id отдельно от private effort/quota id: 3.6 Flash выбирает
   `gemini-3.6-flash-{low,medium,high}`, 3.1 Pro Preview — `gemini-3.1-pro-low`/`gemini-pro-agent`.
   Thinking level выбирается до admission; quota/cooling ключуются private bucket, а affinity,
   billing и клиентский каталог — canonical public id. Response/SSE переписывает private
   `modelVersion` обратно в public id и отдаёт только `.response` (+ responseId), никогда
   wrapper/credits/private headers.
   Official CountTokensRequest `generateContentRequest` разворачивается в private request, body model
   заменяется route model; ambiguous top-level contents + nested request отклоняется. Неподдерживаемые
   `serviceTier`/`store` fail closed вместо silent drop.
   Retry разрешён только до первого переведённого native SSE event. Stream startup bounded по
   time/bytes/chunks, а после первого public event ограничено число подряд идущих private/accounting
   events. После возврата Response disconnect клиента отключает downstream delivery, но task
   продолжает drain до финального usageMetadata. Shutdown deadline обязан abort-ить upstream read,
   settle-ить последний snapshot и только потом отпустить background semaphore permit для
   последующего billing flush.
   Per-profile inflight атомарно ограничен (default 6). Resolved conversation affinity — hard first
   choice до этого потолка; насыщенный home временно spill-ит без потери binding. Новая shared
   system/tools cache-root сначала прогревает два конкурентных profile, затем предпочитает warm
   copy. Unbound routing ставит fresh quota evidence перед stale, затем inflight, coarse quota
   steering только выше 50% used и rotating cursor: exact fractions не herd-ят бёрст на один
   аккаунт. Deterministic soft reserve/jitter сохраняется; если все eligible profiles ниже резерва,
   service floor fail-open до explicit zero. Локальное saturation отдаёт короткий native RetryInfo.
   `/gemini-subs` отделяет quota presence от generation health через failure streak и last
   success/failure evidence и отдаёт reviewed paid-plan identity без Google subject/email/project
   или private tier.
6. Reserve/mark-delivering/settle durable; до upstream `maxOutputTokens` урезается под полный
   консервативный hold доступного баланса. Цена только из `metering::gemini`, ledger provider только
   `registry::PROVIDER_GOOGLE`. Search metered отдельно. Google Maps/File Search и неизвестные future
   server tools fail-closed до появления authoritative ledger dimensions; нельзя proxy-ить paid SKU
   бесплатно. Image response с explicit `candidatesTokensDetails[IMAGE]` использует provider split;
   если private Antigravity отдаёт только aggregate candidates, реально доставленный `inlineData`
   выделяет официальный fixed token SKU requested size, а остаток остаётся text/thinking. Refusal
   без image не получает media charge. Metered non-stream без authoritative usage не доставляется и
   refund-ится; stream после первого байта без final usage списывает conservative hold без fake usage
   event. Public synthetic errors только native Google-shaped и без profile/project/key/upstream.
7. Gemini capacity не выводится из цены подписки или дневного request-count. Antigravity
   `retrieveUserQuotaSummary` принимается только для exact `gemini-5h`/`gemini-weekly`; `3p-*`
   исключены. Каждый успешный generation (billed или admin) durable-кредитует обслуживший opaque
   profile exact official-price nanoUSD из `metering::gemini`. Окна независимы и используют
   fixed-point fraction `10^-8`. Так как Google документирует workload-dependent quota consumption,
   point estimate — realized blend `SCALE*ΣΔspend/ΣΔused`, а НЕ фиксированный номинал подписки.
   Low/high — накопленный per-interval workload envelope с консервативной поправкой ±1 fraction
   unit; confidence перемножает sample maturity, `low/high` stability и fraction resolution.
   Prior/EMA/float money нет. Cold snapshot и первое движение после cold/reset ставят безопасный
   anchor. Cold capacity/remaining остаются `null` и dollar Prometheus series не публикуются до
   следующего complete positive-spend interval; reset сохраняет уже измеренные blend/envelope и
   заново вооружает censor. Raw observations, exact cumulative spend, CAS state и profile spend
   живут в engine authority для replay после estimator upgrade; persistence failure не
   останавливает serving, но явно виден в status.
8. Полный контракт/provisioning/runbook — `docs/engine/GEMINI_PROVIDER.md`. Проверка включает mock upstream:
   rotation fault matrix, credential stripping, RetryInfo, chunk-split SSE, no post-byte retry,
   disconnect drain+settlement и shutdown deadline barrier.


**Тюнинг под живой Anthropic** (identity/beta/UA/version) — через поля `ProxyConfig`, которые
`server` берёт из env. Значения по умолчанию — в `config.rs`.

**Проверка:** `cargo build -p forward`; полный smoke — через бинарь против мок-апстрима.

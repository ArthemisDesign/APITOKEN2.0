# crates/forward — CLAUDE.md

**Роль:** прозрачный форвардинг Claude `/v1/*` на api.anthropic.com (Шаг B) + поллер лимитов;
отдельно — optional strict OpenAI-compatible text adapter через pinned official
`codex app-server` и native Gemini surface поверх encrypted Code Assist OAuth pool. Три provider path не смешивать.

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
Credential в `x-api-key`, `x-goog-api-key` и `Authorization: Bearer` имеют OR-семантику без
приоритета заголовка: достаточно любого валидного. Это критично для Claude Code,
который может одновременно прислать stale `ANTHROPIC_API_KEY` и актуальный `ANTHROPIC_AUTH_TOKEN`.

**Multi-provider pricing Stage 3B1b/3B1c.1 (`pricing.rs`, `pricing/shadow.rs`) — dormant:** pure
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
timestamps and balance-capped actuals before any future enqueue; a bundle for another outer account
is an integrity error, not a durable rejection carrying that account's scalar. The modules have no
DB, HTTP, env, clock, metrics, queue, manifest singleton or runtime caller. Do not wire them into
`authorize`, provider admission, reserve/settle, `/ready` or snapshots without a separate
production-shadow rollout: even a read-only call adds per-request DB/queue/latency risk, and the
actual charge remains legacy scalar.

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

**Stage 3B1c.2 snapshot reserve handoff — dormant:** отдельный
`ReserveWithLegacySnapshot`/`reserve_request_with_legacy_snapshot` передаёт writer'у готовый owned
typed snapshot как единственный источник request/account/hold и вызывает guarded registry commit.
Его guard может отменить только `PENDING → CANCELED` до commit gate. После
`COMMIT_DECIDED` компенсационный `CancelReserve` запрещён: lost reply оставляет active reservation
для exact replay либо штатного lease recovery, без terminal reservation/outbox. PostgreSQL
повторяет transient operation только до commit decision; неоднозначная commit-ошибка возвращается
как ошибка и разрешается последующим exact replay. Существующий live `Reserve` и его прежняя
RAII-компенсация не изменены. Рядом существует только dormant bridge preflight: validated config
(`disabled/0` или `sampled/1..=10000 bp`), SHA-256 v1 sampler по trusted fixed provider и внутреннему
canonical lowercase UUIDv4 request ID, stable typed decisions/reasons. Sampler не читает clock/DB,
не пишет метрики и пока недостижим из runtime. Provider quote/snapshot builders, live caller и
traffic activation отсутствуют, поэтому ни config, ни новый command не участвуют в production
admission.

**Что внутри:** `ProxyConfig`, `AppState`, `Clients` (кэш http-клиентов по прокси),
`limits_from_headers`/`Limits` (unified-ratelimit из ответа), `poll_sub` (активный опрос idle),
`detect_plan` (тариф из /api/oauth/profile), `forward` (axum-хендлер), `authed`;
`codex/` содержит typed app-server transport, Responses/Chat adapters, tenant-bound history,
Codex admission/settlement и reconstruction SSE events; `gemini/` — native route allowlist,
encrypted OAuth pool, Code Assist translation и settlement. Env для обоих читает только `server::config`.

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
3. **Ротация только ДО начала стрима:** решение по статусу (429/401/403/5xx → cooling + следующая
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

**Инварианты Codex adapter (не применять к Claude byte-for-byte path):**
0. **Пул homes = тот же дисциплинарный минимум, что и Claude-флот.** `CodexGateway` держит N
   `CodexHome` (каждый — свой `CODEX_HOME`, свой attested child, своё cooling/auth-состояние).
   Любой transport сохраняет service floor в один authenticated home: одна рабочая подписка не
   становится 503 из-за отсутствия запасной. Production `shared-daemon` blue-green отдельно
   сравнивает точный opaque home set старого и candidate gateway; subset не допускается к cutover.
   Лишний умерший home карантинится и не блокирует одинаковую оставшуюся когорту.
   **Параллелизм на home НЕ ограничен** (как у Claude-флота): вместо семафора turn'ов — атомарный
   счётчик in-flight (`TurnSlot` RAII, снимается на успехе/ошибке/дисконнекте), он лишь сигнал
   загрузки для выбора, не потолок. `CLAUDE_API_CODEX_MAX_CONCURRENT` больше не режет (оставлен для
   совместимости env); единственный глобальный потолок — общий с Claude `AppState::concurrency`.
   **Выбор — cache-first (как `affinity.rs`):** сначала home, к которому закреплён этот разговор
   (`AffinityStore::resolve` через `infer_codex` → тот же стор/Redis-namespace, что у Claude).
   Новый общий cache-root без ожиданий прогревается на двух конкурентных homes; затем тёплый home
   выбирается, пока его свободная calibrated/prior USD-ёмкость не хуже 70% лучшей во флоте, иначе
   запрос сразу идёт на глобально лучший. Это soft placement, а не readiness/quorum: один рабочий
   home всегда обслуживает трафик, фонового repair-сервиса и временных окон нет. OpenAI root warmth
   живёт 30 минут (provider default), Claude cache_control сохраняет собственные 5m/1h TTL. Равные
   по capacity и in-flight кандидаты обязаны чередоваться через атомарный cursor: discovery order
   нельзя превращать в постоянный приоритет или burst-herd на первый home. После успеха
   `run_turn` пишет обслуживший home обратно (`claim`/`remember`/`rebind`/`mark_cache_warm`), чтобы
   продолжение разговора попало на тот же тёплый кеш. Affinity — fail-open оптимизация (с пулом из 1
   home — no-op). Классификация вины как в `proxy.rs`: usage-limit/auth → вина АККАУНТА (cooling до
   reset / 900s карантин, бюджет ретраев НЕ тратят, крутимся по пулу), мёртвый child/timeout → вина
   ТРАНСПОРТА (короткий cooling, ровно один ретрай), 400/context/rpc → вина КЛИЕНТА (не студим, не
   ретраим). **Ретрай только ДО первого delta:** `emitted`-флаг в `send_update` — как только байт ушёл
   клиенту, вторая попытка запрещена. Все homes за лимитом → один OpenAI-shaped 429 с ближайшим reset,
   а не ошибка конкретного аккаунта. Homes адресуются ИНДЕКСОМ (в логах/метриках нет путей и identity).
   Весь пул ограждён ОДНИМ pre-provisioned lock под root-owned `/run/apitoken`: per-home locks
   запрещены, иначе два процесса разделят homes или rename создаст второй inode. Замена home/proxy
   сначала закрывает admission, ждёт все `TurnSlot`, reaps child и лишь затем публикует поколение.
   Detached streaming tasks входят в shutdown-barrier до history+settlement; lock живёт до process exit.
1. Только official `codex app-server` и ChatGPT-owned auth store; токены не читать и не replay-ить.
2. Проверять exact binary SHA-256/version до запуска; child `env_clear`, только allowlisted proxy env.
3. Model-visible initial context = explicit client system/developer + transcript + request-local
   dynamic client tools (function, namespace/function и custom Lark grammar). Codex
   personality/environment/project/plugin/skill/permission/built-in tools запрещены. Custom tool
   выполняет клиент: gateway возвращает raw input и никогда не исполняет его сам.
4. Raw reasoning text не публиковать; только provider summary. `encrypted_content` — только по
   явному Responses `include`, но хранить tenant-bound для корректной continuity.
5. **SDK-совместимость через lenient parsing:** параметры, которые app-server не может исполнить
   (sampling/seed/logprobs/n/store/stream_options, forced
   tool_choice → degrade в "auto", parallel_tool_calls=false, strict=true tools → non-strict,
   неизвестные include, effort вне каталога модели → дефолт модели, message `name`, assistant
   `refusal`/`audio`, legacy `functions`/`function_call` → маппятся в tools/tool_choice, любые
   неизвестные/будущие поля) — принимать и игнорировать, НЕ отклонять: стоковые SDK и агентские
   терминалы шлют их по умолчанию и не должны падать. 400 остаётся только для структурно
   непригодных запросов (нет model/input, пустые messages, битая tool-history, невалидный image
   URL, >4 stop-последовательностей). User-сообщения могут нести изображения: chat `image_url` и
   Responses `input_image` части (data:image/… или http(s)://) транслируются в app-server image
   turn inputs и канонические input_image части истории; base64 data-URL в estimate для reserve
   заменяется placeholder'ом (`sanitize_estimate_images`), чтобы не завышать резерв, но только в
   estimate: `injected_items` истории обязан нести исходные data-URL дословно, иначе app-server не
   декодирует placeholder и подставляет своё «image content omitted». **Client-side
   output-контролы (chat):** `stop` обрезает выдачу по последовательности (StopFilter держит
   хвост longest-1 байт для стреддлинга дельт; сама последовательность не эмитится), а
   `max_tokens`/`max_completion_tokens` — приблизительный кап ~4 char/token с
   `finish_reason="length"`; settlement ВСЕГДА по authoritative upstream usage (клиентская обрезка
   не экономит провайдерские токены). **Полный wire-контракт:** chat стримит reasoning summaries
   как `reasoning_content` дельты (+ join в non-stream message); оба стрима шлют data-bearing SSE
   progress каждые 15с (`SSE_HEARTBEAT_INTERVAL`); Responses-стрим завершается
   `response.completed` ИЛИ `error`+`response.failed` с полным failed-объектом; non-stream ответы
   несут `x-ratelimit-*` из окна провайдера (процентная база 100). **Retrieval:** `store=true`
   ответы читаются/удаляются через `GET/DELETE /v1/responses/{id}` и `/input_items` из того же
   tenant-bound history store (TTL, зашифрованный Redis); StoredHistory хранит полный response
   object + input_count (serde default — старые записи читаются, response=None → 404);
   `store=false` не персистится и не читается. `POST /v1/responses/input_tokens` отдаёт оценку
   (estimate/4) без turn и reserve. Ограниченные диагностические
   compatibility-поля текущего Codex (`client_metadata`, `safety_identifier`) разрешено валидировать
   и отбрасывать без логирования/форвардинга; `prompt_cache_key` валидируется, отражается в публичном
   ответе и входит как strong alias в affinity. В pooled app-server передавать только стабильный
   tenant-scoped keyed digest (или такой же digest автоматически выведенной cache-lineage), никогда
   raw customer key и никогда эфемерный thread UUID. Responses `input_tokens_details` и Chat
   `prompt_tokens_details` обязаны отражать authoritative `cache_write_tokens` вместе с
   `cached_tokens`. `service_tier=fast|priority` для Fast-capable модели нормализуется в
   app-server `priority`; `thread/start` обязан подтвердить тот же tier, иначе запрос fail closed,
   потому что молчаливый downgrade нарушит биллинг. Для standard/default всегда передавать явный
   app-server sentinel `"default"` (не JSON null), чтобы локальный config home не апгрейдил трафик;
   ответ `thread/start` обязан подтвердить `"default"`. Остальные tier-значения leniently
   деградируют в default. Codex CLI 0.146 присылает client-executed `tool_search` в developer
   `additional_tools`: до обновления pinned app-server этот public wire-item транслировать во
   внутреннюю dynamic function и обратно; `tool_search_call`/`tool_search_output` истории хранить
   в публичной форме, а в app-server replay преобразовывать в function call/output. Function tool
   `defer_loading` принимать и передавать как dynamic `deferLoading`. Codex `GET /v1/models`
   (originator/UA начинается с `codex`) требует native `{"models":[]}` overlay, который CLI
   объединяет со своим version-matched bundled catalog; остальным клиентам сохранять стандартный
   OpenAI list envelope. Usage для settlement брать из authoritative completed app-server turn.
6. Не заявлять OpenAI ownership: public `owned_by` остаётся `apitoken`; полный scope и runbook —
   `docs/CODEX_APP_SERVER.md`.
7. **Цены — только из `metering::codex`** (audited, effective-dated таблица, как Claude-тарифы).
   `forward` цену не объявляет; reserve и settle резолвят её по одному и тому же clock. Fast
   множитель ChatGPT credits (GPT-5.6/5.5 = 2.5x, GPT-5.4 = 2x) применяется одинаково к reserve,
   settle, provider ledger и capacity spend; неизвестная Fast-модель резервируется консервативно
   по 2.5x.
8. **Калибровка ёмкости окна — как в Claude-пуле, на тех же гвардах.** Каждый успешный turn
   (billed ИЛИ admin) кредитует home его exact official-price cost (`billing::price_real_nano`,
   чистая математика, деньги не трогает); каждый rate-limit snapshot гоняется через
   `calibration.rs`: интервал ≥2 целых used%-поинта калибрует `cap=Δspend/Δused` только если НАШ
   расход объясняет ≥50% движения (собственное использование владельца аккаунта — не ёмкость пула)
   и sample в [0.25x, 4x] прайора (`CodexConfig.window_cap_usd_prior`, env
   `CLAUDE_API_CODEX_WINDOW_CAP_USD`, неделя=10080min — для остальных окон прайор масштабируется
   по длительности). EMA 0.7/0.3 с jump-clamp 2x, rollover окна только пере-якорит. Состояние
   in-memory; экспорт — `claude_api_codex_(home_)window_(capacity|remaining)_usd` метрики.
8. **Санитайзер ошибок:** публичный конверт не должен раскрывать пул/child/binary/ChatGPT-профиль
   или upstream-текст. Гейтит `codex::api::tests::public_errors_never_leak_internal_architecture`
   (близнец `local_err_never_leaks_*`).

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
   Generation/quota/probe/token refresh используют gaxios wire Gemini CLI 0.53.0 +
   google-auth-library 10.9.0; OAuth userinfo использует отдельный official global-fetch/Undici
   профиль того же SHA-pinned Node. Никакой approximate BoringSSL impersonation или ambient proxy/env.
   Helper получает proxy secret только первым IPC frame, multiplexes bounded NDJSON, reaps process
   group и может restart-нуться только до upstream headers. Outbound frames, inbound NDJSON/base64
   staging, OAuth response collections и short-lived header/form strings zeroized. Loopback mocks
   остаются на `wreq`.
4. Профиль владеет отдельным transport/proxy/inflight/cooling/auth и single-flight token refresh.
   Первый 401 → один refresh+retry того же profile; повторный 401/403 → auth quarantine. 429 →
   model-specific profile cooling по Retry-After/RetryInfo/quota reset и ротация без
   transport-бюджета; health probe не стирает generation cooling. `retrieveUserQuota` публикует
   sanitized model catalogue: explicit zero блокирует модель до самого позднего reset среди всех
   exhausted dimensions, stale/missing bucket fail-open.
   Network/
   408/409/425/5xx → короткий cooling и ограниченный transport retry; остальные 4xx не вращаются.
   Если были quota failures — итог 429; только auth/transport failures — 503; уже cooling pool — 429.
5. Code Assist request wrapper строится сервером; caller не может inject project/session identity.
   `request.session_id` — UUID из keyed tenant-scoped affinity lineage: стабилен для растущего чата,
   изолирован между tenant/explicit session и не содержит raw id; `user_prompt_id` повторяет
   официальный `<session UUID>########<human-turn ordinal>` (tool-result-only contents не считаются).
   Response/SSE отдаёт только `.response` (+ responseId), никогда wrapper/credits/private headers.
   Retry разрешён только до первого переведённого native SSE event. Stream startup bounded по
   time/bytes/chunks, а после первого public event ограничено число подряд идущих private/accounting
   events. После возврата Response disconnect клиента отключает downstream delivery, но task
   продолжает drain до финального usageMetadata. Shutdown deadline обязан abort-ить upstream read,
   settle-ить последний snapshot и только потом отпустить background semaphore permit для
   последующего billing flush.
6. Reserve/mark-delivering/settle durable; до upstream `maxOutputTokens` урезается под полный
   консервативный hold доступного баланса. Цена только из `metering::gemini`, ledger provider только
   `registry::PROVIDER_GOOGLE`. Search metered отдельно. Google Maps/File Search и неизвестные future
   server tools fail-closed до появления authoritative ledger dimensions; нельзя proxy-ить paid SKU
   бесплатно. Metered non-stream без authoritative usage не доставляется и refund-ится; stream после
   первого байта без final usage списывает conservative hold без fake usage event. Public synthetic
   errors только native Google-shaped и без profile/project/key/upstream.
7. Полный контракт/provisioning/runbook — `docs/GEMINI_PROVIDER.md`. Проверка включает mock upstream:
   rotation fault matrix, credential stripping, RetryInfo, chunk-split SSE, no post-byte retry,
   disconnect drain+settlement и shutdown deadline barrier.


**Тюнинг под живой Anthropic** (identity/beta/UA/version) — через поля `ProxyConfig`, которые
`server` берёт из env. Значения по умолчанию — в `config.rs`.

**Проверка:** `cargo build -p forward`; полный smoke — через бинарь против мок-апстрима.

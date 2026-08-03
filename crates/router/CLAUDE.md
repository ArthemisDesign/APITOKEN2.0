# CLAUDE.md — crates/router (claude-router)

Единый stateless вход для всех provider-плоскостей — этап 1b
`docs/engine/UNIFIED_ROUTER.md`. Отдельный bounded context ВНЕ слоёв
`registry ← pool ← forward ← server`: бинарь `claude-router`, общается с
плоскостями только по HTTP через stable loopback origins (8790/8792/8794).

## Границы (НЕ нарушать)

- **Никаких импортов** `pool`/`forward`/`registry`/`metering` — весь контакт с
  engine — HTTP к stable origins. Новая «крутая» возможность, требующая импорта
  engine-крейта, означает, что она принадлежит плоскости, а не router'у.
- **Биллинг только в плоскости.** Router не резервирует, не списывает, не
  знает `request_id`. Ключ клиента передаётся в плоскость verbatim
  (`proxy::AUTH_HEADERS`); env-секретов у router'а нет.
- **Fail-closed retry.** Native lanes и обычные single-model universal-запросы
  выполняют ровно одну попытку. При включённом `CLAUDE_ROUTER_FALLBACK_ENABLED`
  следующая модель из эффективной цепочки разрешена только после точного не-2xx
  `x-apitoken-execution-state: not_started` (кроме 401/402/прочих клиентских
  4xx; signed 429 — capacity-отказ) либо доказанного TCP `ConnectionRefused`.
  Timeout, DNS/generic connect error, unsigned 5xx, reset/обрыв и ответ после
  заголовков никогда не ретраятся (`docs/engine/ROUTING_FENCING.md` §3.3).
- **Execution identity — capability, не клиентский input.** Для каждой эффективной цепочки
  длиннее одной модели
  router один раз генерирует CSPRNG UUIDv4 и инжектирует
  `x-apitoken-execution-group` + positive attempt `1..N`. Перед инжектом клиентские копии всегда
  удаляются; native и universal single-attempt paths оба заголовка не отправляют. Caddy независимо
  стирает их на внешнем ingress. Identity допустима только во внутренних router→plane запросах и
  никогда не возвращается клиенту.
- **Никаких execution-очередей, semaphore, circuit breaker, rate limits** (инвариант 3).
  Единственное исключение — 64 MiB fail-fast budget с шагом 1 MiB на buffered universal request
  bodies: известный `Content-Length` округляется вверх, неизвестный резервирует полные 32 MiB;
  budget никогда не ждёт, освобождается после response headers и не держит native/SSE response.
  Readiness (`/health`, `/live`, `/ready`) — router-local, никогда не
  конъюнкция health плоскостей; синхронных health-check'ов на пути запроса нет.
- **SSE не буферизуется.** Тела запроса и ответа — потоки
  (`Body::wrap_stream`/`Body::from_stream`); reqwest собран без auto-decode
  (default-features off), чтобы байты и Content-Encoding шли неизменно.
  Единственное исключение — shared `routing.rs`: тело ЗАПРОСА
  `/v1/chat/completions`, `/v1/responses` и
  `/v1/messages{,/count_tokens}` читается целиком (лимит 32 MiB) ради поля
  `model`; тело ответа остаётся потоком. Дополнительное исключение — router-owned
  `x-apitoken-service-tier: fast|priority` и OpenAI-compatible body alias
  `serviceTier:"fast"|"priority"`: на исполняемых GPT Chat/Responses-запросах router
  нормализует selector в body `service_tier:"priority"`; alias и заголовок до плоскости
  не доходят. Body alias на Messages/count_tokens и любой Fast selector на non-GPT
  fail-closed отклоняются.
  Disconnect клиента обязан транзитивно рвать соединение к плоскости
  (TeeMeter drain): поэтому вокруг тела ответа нет detached-тасков.
- **Внутренняя семантика исполнения не транслируется клиенту.** Заголовок
  `x-apitoken-execution-state` (контракт `docs/engine/ROUTING_FENCING.md` §3, этап 6.1) —
  контракт движок↔router: плоскости выставляют его на отказах без исполнения
  (`not_started`), router обязан снимать его со ВСЕХ транзитных ответов перед отдачей
  клиенту (`proxy.rs` `EXECUTION_STATE_HEADER`). За условия заголовка отвечает только
  сам движок — router проверяет сигнал только внутри fallback engine и не
  транслирует его. Клиенты не должны зависеть от внутреннего состояния движка.
- **Деньги — только integer**: router денег не касается вовсе; если когда-либо
  появятся суммы — nanoUSD-строки, никакого float.

## Что здесь живёт

- `config.rs` — единственное место чтения env (`CLAUDE_ROUTER_*`), включая
  строгий off-by-default флаг `CLAUDE_ROUTER_FALLBACK_ENABLED` (`0|1|false|true`).
- `auth.rs` — uncached bodyless early-auth клиент: до чтения universal body перебирает fixed
  origins, принимает только exact schema-v1 success, считает 401 терминальным и fail closed
  обрабатывает mixed-version/transport/5xx.
- `proxy.rs` — байт-в-байт proxy native lanes, auth passthrough и классификация
  одной попытки до публичных headers: exact `not_started` / source-chain
  `ConnectionRefused`; 30-секундный deadline ограничивает только ожидание response headers и
  никогда не включает retry, response body остаётся без total timeout. Внутренний заголовок снимается до сборки ответа. Перед
  любой плоскостью также снимается публичный router capability-header
  `x-apitoken-service-tier`.
- `routing.rs` — общий model dispatch и serial fallback для всех universal
  поверхностей. Сначала выполняет bodyless auth, затем fail-fast резервирует размер body в общем
  64 MiB budget и держит его до plane response headers; overload возвращает lane-shaped 503 без
  billable call. Обычный запрос без `models`, `provider` и `preset/*` сохраняет
  исходные байты и прямой namespaced dispatch. Расширенный planner раскрывает preset,
  получает один aggregate catalog snapshot, canonical-дедуплицирует цепочку, применяет
  provider filters/order/reviewed sort и `allow_fallbacks`, затем account-policy preflight.
  Только после этого он удаляет `models`/`provider`, подставляет выбранный `model` и
  исполняет retry matrix. Эффективная цепочка длиннее одного элемента владеет одной
  CSPRNG execution-group UUIDv4 и монотонным attempt per model; после фильтрации до одной
  модели capability headers не инжектируются.
  Логи attempts содержат только surface/index, публичный catalog ID, lane,
  status и bounded retry reason — без URL, headers, credentials и тел запросов.
  Здесь же валидируются совместимые Fast selectors: header для Chat/Responses/Messages и
  camelCase body alias только для OpenAI-compatible Chat/Responses. Они разрешены лишь для GPT,
  не token counting; конфликтующие `serviceTier`/`service_tier`/Messages `speed` отклоняются до
  вызова плоскости. GPT-only проверка идёт после preferences/policy, то есть оценивает только
  исполняемые attempts.
- `policy.rs` — закрытая схема OpenRouter-shaped `provider` preferences и bounded
  клиент engine-owned `/internal/router/policy/preflight`: все значения auth-заголовков
  передаются verbatim, fixed origins перебираются последовательно, `401` терминален,
  malformed/mixed-version ответы fail closed. Credential и policy response не кэшируются.
- `pricing.rs` — bounded клиент engine-owned `/internal/router/catalog/pricing`: credential
  текущего catalog-request передаётся verbatim, fixed origins перебираются последовательно,
  `401` терминален, а schema version/unit/canonical integer strings/ordered subset проверяются
  fail closed. Персональные rate cards существуют только в памяти запроса и не кэшируются.
- `presets.rs` + `routing-presets.json` — compiled reviewed presets, integer price/latency
  ranks и проверенный context window. Manifest валидируется при старте; отсутствующие live
  members пропускаются, полностью пустой preset не исполняется.
- `metrics.rs` — compile-bounded `claude_router_fallback_total`: ровно 18 series для трёх
  namespace и двух доказательств continuation. Инкремент происходит один раз непосредственно
  перед следующей попыткой; model/credential/group/request identity в labels запрещены.
- `chat.rs` и `responses.rs` — тонкие OpenAI-shaped entrypoints в `routing.rs`.
- `messages.rs` — тонкий Anthropic-shaped entrypoint для `POST /v1/messages` и
  `POST /v1/messages/count_tokens`: namespaced `openai/*` уходит на Codex plane
  (там Messages→Responses адаптер `crates/forward/src/codex/skin.rs`),
  `anthropic/*` — на Anthropic plane как native lane, `google/*` — на Gemini
  plane по общему namespace-правилу (Messages→generateContent skin реализован
  в `crates/forward/src/gemini/skin.rs`). Для `count_tokens` выбирается та же
  плоскость: Anthropic native, reserve-grade локальный подсчёт Codex или
  quota-free native `:countTokens` Gemini.
- Stored responses endpoints (`/v1/responses/input_tokens`, `/v1/responses/{id}`,
  `.../input_items`) dispatch не используют — они остаются native OpenAI lane
  (stored responses только `openai/*`, решение 5).
- `catalog.rs` — единый `/v1/models`: агрегация трёх плоскостей, namespaced ID
  + только глобально однозначные aliases, TTL-кэш 30 с, last-good при падении плоскости, маркер деградации
  `x-apitoken-catalog-degraded`. `main.rs` после той же aggregate-auth проверки отвечает
  Codex `originator`/User-Agent backend-native overlay `{models:[]}` (CLI объединяет его со
  встроенными metadata), не меняя OpenAI-list для остальных клиентов. Consumer строго
  нормализует Anthropic native `max_input_tokens`/`max_tokens`/effort matrix и owned
  OpenAI/Gemini `apitoken.limits/capabilities`; публикует их в `apitoken` и прежних top-level
  capability mirrors. Missing legacy metadata не угадывается, malformed metadata переводит
  плоскость на last-good/degraded. Alias collision снимает alias со всех участников, но
  namespaced ID и отдельный native ID для body rewrite/pricing остаются рабочими. Здесь же — общий для
  universal dispatch'ей `pub(crate) namespace_lane` (прямой выбор плоскости без catalog fetch
  для запросов без fallback). `main.rs` добавляет только активные `preset/*` записи — если
  aggregate snapshot содержит хотя бы один member соответствующего preset. Затем `main.rs`
  отдельно получает key-scoped pricing ordered subset, фильтрует недоступные модели, публикует
  exact nanoUSD/M strings в `apitoken.pricing`, не затирая runtime metadata, и ставит
  `Cache-Control: private, no-store`;
  ошибка pricing authority даёт 503 без zero/stale fallback.
- `error.rs` — синтетические ошибки router'а в конверте соответствующего
  провайдера (ошибки плоскостей проксируются байт-в-байт, сюда не попадают).
- `main.rs` — таблица маршрутов публичного контракта + композиция.
  Loopback-only `GET /metrics` не требует отдельного секрета; Caddy не включает его в публичный
  allowlist, а Prometheus скрейпит stable Caddy origin `127.0.0.1:8802`, который следует за тем же
  single-active backend, что публичный vhost.

## Проверка

```bash
cargo test -p claude-router   # unit + интеграционные (mock-плоскости на TCP)
cargo build                   # весь workspace зелёный до коммита
cargo build && bash tests/router_fallback_smoke.sh  # concurrent 6.4c mock-load + metric deltas
```

Интеграционные тесты поднимают mock-плоскости на реальных loopback-сокетах и
покрывают: early auth до незавершённого большого body, terminal 401 и mixed-version failover,
weighted 64 MiB overload без очереди, release permit при parse error и после SSE headers,
pre-header deadline без retry, passthrough тела/заголовков, небуферизованный SSE, транзитивный
disconnect, строгую нормализацию/деградацию/stale capability-каталога, снятие конфликтующих aliases,
uncached key-scoped pricing для двух
ключей при одном shared cache, terminal pricing 401/503, canonical wire validation, alias-
разрешение моделей, 404/405, model-based dispatch chat-, responses- и
messages- и messages/count_tokens-запросов (namespaced без catalog fetch,
alias через каталог, 400 невалидного/слишком большого тела, небуферизованный
chat SSE), а также off-by-default fallback, preflight всей цепочки, точный
retry matrix (`not_started`, 429, 4xx/5xx, ConnectionRefused/timeout), rewrite
per-attempt body, provider preferences, preset publication/expansion, mixed-version policy
failover, terminal `401`, strict subset/empty `403`, Fast после policy filtering и обязательное
снятие внутреннего заголовка. Живой PostgreSQL и подписки не нужны.
Полная цепочка router→engine→mock upstream — `tests/universal_chat_smoke.sh`. Отдельный
`tests/router_fallback_smoke.sh` поднимает три deterministic TCP-плоскости и доказывает
parallel `not_started → success`, provider+strict-policy fencing до attempt 1, terminal unsigned
503, last-good catalog + killed origin → `ConnectionRefused` и точные дельты 18-series метрики.

После выката telemetry-пакета на точный GREEN SHA live canary запускается
`tests/router_fallback_live_canary.sh` с уже существующим `APITOKEN_API_KEY` и явным
`APITOKEN_CANARY_EXPECTED_SHA`. Wrapper исполняет отдельный router-процесс из реально запущенного
immutable production release, передаёт ключ только по SSH/curl stdin и всегда удаляет временные
shim/response-файлы. Он fail closed требует strict subset и две разрешённые provider-плоскости,
проверяет signed/unsigned/ConnectionRefused matrix и отсутствие роста double-winner, balance
divergence и settlement backlog. До зелёного результата unit-флаг не меняется.

## Эксплуатация

Юнит `systemd/claude-router@.service` работает в двух fixed slots `127.0.0.1:8800/8801`;
`deploy/router-bluegreen.sh` — единственный владелец их lifecycle. Он проверяет inactive slot по
exact immutable binary, атомарно переводит Caddy через root-owned runtime snippet и лишь затем
останавливает старый slot с bounded graceful drain. Legacy `claude-router.service:8798` существует
только для первого handoff/rollback horizon. Публичная граница — Caddy vhost
`router.apitoken.sale` (см. `deploy/CADDY.md`); multi-host HA этим не заявляется.

Fallback после выката остаётся выключен: отсутствие env-флага — контрактный
default. Canary включает его только явным
`CLAUDE_ROUTER_FALLBACK_ENABLED=1`; любое присутствие `models`, `provider` или
`preset/*` при выключенном флаге получает lane-shaped `400` до catalog/policy/network work.

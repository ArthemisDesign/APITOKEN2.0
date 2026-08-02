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
Funding-v2 normalization follows that split: read-only account plans use a bounded reader, exact
apply uses the existing single writer and PostgreSQL account lock, and SQLite fails closed.
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

**Execution-state контракт (этап 6.1 docs/engine/ROUTING_FENCING.md, `proxy.rs`
`EXECUTION_STATE_HEADER` + хелперы `with_not_started`/`without_not_started`):** плоскости
выставляют `x-apitoken-execution-state: not_started` на ответе, только когда выполнены ВСЕ три
условия — не-2xx, ни байта публичного ответа клиенту не ушло (per-plane граница доставки:
Anthropic — до `mark_delivering`, Codex — до `emitted`/первого SSE-кадра, Gemini — до первого
публичного события), reserve по request_id гарантированно уйдёт в refund/cancel. Денежный
инвариант «ответ с заголовком ⇒ ledger не содержит и не будет содержать charge по request_id»
доказан per-plane тестами с реальным reserve; ветки, где charge возможен (legacy-scalar
full-hold, дропнутый TeeMeter на 2xx-пути, fallback-сборки SSE после запущенного admission),
обязаны снимать заголовок через `without_not_started`. Точки выставления: Anthropic —
`local_err_for` и `stream_back` (не-2xx без метеринга), Codex — `ApiError::into_response` и
`skin_error`, Gemini — `ApiError::into_response` и Messages-skin `skin_error`. На 2xx заголовок
недопустим никогда (включая SseErrorTail внутри 200). Universal Chat/Responses-адаптеры
(`anthropic.rs`/`anthropic_responses.rs`, `gemini/chat.rs`/`gemini/responses.rs`) выставляют
сигнал на локальных pre-request отказах, сохраняют только точный авторитетный сигнал при
пересборке не-2xx плоскости и явно снимают его с ошибок разбора/сборки уже после 2xx, когда
charge возможен. Gemini Messages skin следует тому же правилу для своей поверхности. Router
обязан снимать заголовок с транзитных ответов (см. crates/router/CLAUDE.md). Публичный
`is_exact_not_started_response` — единый predicate для router-proof и server telemetry: только
non-2xx с ровно одним exact lowercase значением. `crates/server` увеличивает bounded per-plane
counter только для ответа, который fixed plane действительно вернула наружу; malformed duplicate
header и 2xx не считаются.

**Execution-group capability (этап 6.3):** `x-apitoken-execution-group` и
`x-apitoken-attempt` — только router→plane. Caddy удаляет клиентские значения на каждом публичном
ingress. Admission один раз парсит пару до money mutation: оба отсутствуют → direct execution;
оба присутствуют ровно один раз → canonical lowercase UUIDv4 + canonical positive decimal;
partial/duplicate/malformed/noncanonical → fail closed. Anthropic парсит в `proxy::forward`,
Codex/Gemini — в `begin_admission`; identity проходит через scalar, legacy-snapshot и strict-policy
reserve в `AsyncBilling`. При отправке во внешний Anthropic upstream оба внутренних заголовка
удаляются. Плоскость не генерирует и не исправляет identity самостоятельно.

**Claude capacity calibration (`anthropic_calibration.rs`, `billing.rs`, `meter.rs`):** каждый
успешный Anthropic turn, включая неметеренный admin traffic, после authoritative usage строит один
immutable event с внутренним request ID, subject/email, model, Standard/Fast, inference geography,
tariff schedule, exact input/cache-read/cache-write-5m/cache-write-1h/output/search counters и
соответствующими API nanoUSD legs. Событие сначала продвигает cumulative subject spend и только
затем response quota snapshots наблюдают новый total. Poll snapshots бесплатны: они читают durable
spend, но никогда его не увеличивают. После постановки authoritative turn event в FIFO `TeeMeter`
немедленно помечает обслужившую подписку для backend count-tokens probe и будит server poller:
боевые response headers больше не могут держать `polled_ts` свежим и откладывать post-turn pairing.
Дебаунс `Pool::request_probe` ограничивает probe не чаще одного раза за 15 секунд на подписку;
writer poll-команды перед observation сначала дренирует pending turn FIFO, поэтому backpressure не
переставляет quota раньше spend. Response snapshot и быстрый post-turn poll могут иметь одинаковую
секунду: FIFO остаётся порядком истины, равный timestamp с изменившейся quota обрабатывается, а
точный quota/reset/resolution дубль игнорируется. Decimal quota fractions парсятся в `10^-8` units без float;
реальное разрешение каждого endpoint хранится отдельно. Response и бесплатный count-tokens probe
публикуют exact fraction также без reset как ephemeral `pool::QuotaSnapshot`: server использует её
только для свежего current remaining. Durable `observe_anthropic_window`, interval history и
estimator по-прежнему требуют настоящий reset — runtime не выдумывает window identity.

Окна 5h и 7d имеют независимые identity/history/reset и оцениваются без номинала подписки,
prior/EMA/WLS: `capacity_nano = 100_000_000 × Σobserved_spend_nano /
Σobserved_fraction_units`. Первое quota-only движение ждёт один snapshot ledger catch-up; повтор
без spend становится unattributed и не раздувает ёмкость. Raw history полностью replay-ится при
смене estimator version. Runtime не усредняет noisy аккаунт как коммерческий номинал: server
pool-ит exact evidence только внутри одинакового plan + duration.

Доставка turn evidence — bounded FIFO 4096 в состоянии `AsyncBilling`, которую последовательно
дренирует billing writer. После исчерпания PostgreSQL operation retry голова остаётся pending и
блокирует более поздние Claude events и poll snapshots; следующий event/poll либо graceful
`AsyncBilling::flush` сначала повторяет её. Exact request replay
безопасен; permanent semantic conflict карантинит только конфликтную строку и не блокирует очередь.
Overflow/conflict увеличивают dropped counter. `pending_events`, `dropped_events` и
`persistence_ok` публикуются через `/capacity` и Prometheus; при pending/degraded доставке текущий
remaining fail-closed, а накопленная историческая capacity evidence остаётся видимой.

Операторский live-runner может адресовать bounded четырёхсимвольный profile hint заголовком
`x-apitoken-calibration-profile`, но только при `Authz::Admin` (forwarding-admin/доверенный
loopback). Metered/control/panel credential этот заголовок игнорирует; до Anthropic он всегда
вырезается. Pool принимает target только при ровно одном совпадении, обходит мягкий Reserve, но
сохраняет hard cap/cooling/auth-dead и запрещает spill/rebind. PostgreSQL lease получает hard-cap
семантику pinned continuation. Так exact API-nanoUSD и quota delta связываются с одной подпиской,
не открывая клиентам ручной выбор профиля. Атрибуция самого тестового turn берётся не из aggregate
delta, а из bounded множества новых immutable event request IDs с exact profile/model/tier/token
vector; customer traffic в той же aggregate-строке поэтому не загрязняет результат.

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
`cap_to_balance`, OpenAI pricing builder — неизменённый `reserve_cost`, Google builder — те же
`reservation_for_budget`/conservative Gemini rates и search reserve units, что scalar path;
provider/canonical/tariff и hold caller не задаёт.

Live metered Anthropic/OpenAI/Gemini admission теперь применяет sampler до денег. Durable identity
Gemini plane — `google`; deprecated provider ID `gemini` не создаётся. Disabled/not-sampled и typed
pre-money fallback идут в byte-equivalent scalar reserve без snapshot; selected request атомарно
сохраняет reservation+actual snapshot. После выбора atomic path invariant/DB/handoff или
idempotency conflict fail closed без второго scalar reserve. Успешный hold продолжает прежний
mark-delivering/cancel/settlement lifecycle и только после durable success передаёт snapshot в
bounded shadow queue. Default config остаётся `false/0`; включение требует явного bounded sample.
Метрики имеют только три фиксированных provider label, bounded reason labels и fixed-bucket atomic
reserve latency histogram. Strict Gemini, release-v2 reserve/settlement snapshot и Stage 9
activation этим producer checkpoint не включены.

**Целевой Stage 9 runtime:** active pricing release выбирается одним global head. B2C использует
discount rules с приоритетом model → provider → global 50%; B2B имеет независимую policy,
OpenKeys — строго 1:1. Anthropic/OpenAI/Gemini фиксируют provider-owned canonical model/tariff,
release/policy rule и ordered funding allocations в одном immutable reserve snapshot. Welcome
bonus доступен любому B2C discount rule и расходуется раньше paid; commission eligibility от
pricing mode не зависит. Service `meter_only` сохраняет official usage без balance reserve/debit.
Settlement использует pinned multiplier/tariff; cancel/RAII возвращает allocations. Старый
`track`/tier path — только migration source и не должен получать новую логику.

Read-only router policy preflight фазы 6.4a переиспользует публичные `resolve_pricing` и
`RuntimePricingManifest::from_evidence` через композицию `crates/server`: тот же customer key и один
coherent bundle фильтруют bounded catalog chain до первой router-attempt. Этот caller не строит
quote/snapshot, не резервирует деньги и не меняет admission; legacy/shadow/unbound остаются
unrestricted, strict Gemini — запрещён в соответствии с live admission выше.

Strict counters имеют только фиксированные `provider`, `mode`, `scope`, `reason`; Gemini входит в
фиксированный provider set и после product activation обязан иметь наблюдаемое admitted coverage. Typed resolver
rejections сводятся к bounded operational classes (missing policy/rule, unavailable model/switch,
unsupported capability, invalid contract), без account/model labels. Наличие этого runtime-кода
само по себе не переводит active release и не заменяет full-inventory Stage 8 evidence или Stage
5/6 materialization. Финальное включение — single-head CAS без canary и traffic drain.

**Что внутри:** `ProxyConfig`, `AppState`, `Clients` (кэш http-клиентов по прокси),
`limits_from_headers`/`Limits` (unified-ratelimit из ответа), `poll_sub` (активный опрос idle),
`detect_plan` (тариф из /api/oauth/profile), `forward` (axum-хендлер), `authed`;
`anthropic.rs` — universal Chat Completions→Messages адаптер (этапы 3.1–3.4b
docs/engine/UNIFIED_ROUTER.md): переводит chat-запрос в Messages JSON (strip
`anthropic/`-префикса ДО admission, дефолт `max_tokens` 4096, склейка одноролевых
сообщений и серий tool-ответов, capability matrix из 16 правил с `400 unsupported_parameter`
для не-дефолтных penalties/logprobs/seed) и вызывает общий `forward()` — auth, reserve,
ротация, identity-инжект, tee-метеринг и settle без изменений; ответ переводится
СНАРУЖИ (Messages SSE → `chat.completion.chunk`, JSON → `chat.completion`), а все
ошибки этого пути (включая `local_err` и пасsthrough апстрима) конвертируются в
OpenAI-конверт с сохранением HTTP-статуса (402 LowBalance тоже) и `Retry-After`.
Tools (3.2): chat `tools`/`functions` → Messages `tools[]` (`parameters`→`input_schema`),
`tool_choice`/`function_call`/`parallel_tool_calls` → `tool_choice` (+`disable_parallel_tool_use`),
история `tool_calls`/tool-ролей ↔ `tool_use`/`tool_result` блоки (legacy id —
детерминированный `callu_<name>`), в ответе `tool_use` ↔ `message.tool_calls`
(non-stream) и tool_calls-чанки из `content_block_start`/`input_json_delta` (SSE, tool
ordinal нумеруется отдельно от Messages block index); словарь событий закреплён
contract-тестами в модуле. Мультимодальность и structured output (3.4a):
image_url-части user-сообщений → Messages image-блоки (data: → base64 source,
http(s) → url source, `detail` != auto → 400), `response_format` json_schema →
GA `output_config.format` (только схема; json_object отклонён matrix).
Reasoning (3.4b/3.4c): `reasoning_effort` принимает совместимые
minimal|low|medium|high|xhigh|max и переводится в GA `output_config.effort`
(minimal клампится в low, невалидное значение → `400 invalid_request`; `effort` соседствует
с `format` в одном `output_config`). Точная нативная матрица model-specific: Claude 4.6 —
low|medium|high|max, Claude 4.7+/5 — low|medium|high|xhigh|max; уровень вне матрицы совместимой
модели отклоняется локально до reserve/upstream. Адаптер также делает инжект
`thinking: {type:"adaptive", display:"summarized"}` — без него adaptive выключен, а дефолтный
display=omitted присылает пустые thinking-блоки; явный `thinking` клиента не переопределяется.
На моделях до 4.6 upstream отвергает оба поля, поэтому валидный effort деградирует к model
default без них; явный legacy `thinking` сохраняется. Effort не создаёт отдельный metering
modifier: thinking уже входит в общий Anthropic `output_tokens`, а reserve ограничивает весь
выход через `max_tokens`.
Thinking-блоки/thinking_delta ответа → `message.reasoning_content`/reasoning_content-дельты
(signature/redacted_thinking не выставляются).
Синтетические OpenAI-ошибки адаптера рождаются ТОЛЬКО через его `chat_error` (с
`TerminalErrorReason`, как у `local_err`) и тоже без внутренностей пула.
`anthropic_responses.rs` — universal Responses→Messages адаптер (этапы 4.1–4.2
docs/engine/UNIFIED_ROUTER.md, роут `POST /v1/responses` в `ProviderMode::Anthropic`)
по той же схеме, что chat-адаптер: Responses-запрос переводится в Messages JSON
(`instructions`/system/developer items → top-level `system`, input items → сообщения со
склейкой одноролевых, `input_text`/`output_text` → text-блоки, `input_image` →
image-блоки общим переводом, replay tool-истории (4.2): function_call items →
assistant `tool_use`-блоки (`call_id` → `id`, `arguments` JSON-строка парсится в
`input`, невалидный → `400 invalid_request`), function_call_output items → user
`tool_result`-блоки (`call_id` → `tool_use_id`; output строка как есть либо склейка
text-партов через \n, нетекстовые части → 400), pairing tool_use/tool_result не
валидируется — как chat-адаптер 3.2, `tools` → `input_schema` (не-function tool → 400),
`tool_choice`/`parallel_tool_calls` → Messages `tool_choice`, `max_output_tokens` →
`max_tokens` дефолт 4096, `reasoning.effort` → та же model-specific матрица
`output_config.effort` + инжект `thinking: {type:"adaptive", display:"summarized"}` как 3.4c
(на прежних моделях hint деградирует к model default), `text.format` json_schema
→ `output_config.format`, capability matrix из 9 правил + open list) и вызывает общий
`forward()` без изменений; ответ переводится СНАРУЖИ — Messages SSE → Responses SSE
словаря 4.1 + reasoning 4.2 (`response.created`/`in_progress` → per-block
item/part/text|arguments дельты; thinking-блок → reasoning item `rs_*` +
`response.reasoning_summary_part.added` → `response.reasoning_summary_text.delta`*
(signature и пустые дельты дропаются) → `…_text.done`/`…_part.done` → item.done →
`response.completed`; ping → `: ping`; `event: error`/преждевременный EOF →
`response.failed`; output_index — плотный счётчик, включающий thinking-блоки,
redacted_thinking — без позиции; `output_tokens_details` message_delta проксируются),
JSON message → Response object (text в один message item на позиции первого
text-блока, thinking → reasoning items `rs_*` в порядке блоков (пустой thinking — без
item, redacted_thinking пропускается), tool_use → function_call items, usage с
cache/reasoning details, status по stop_reason); ошибки — общий с chat-адаптером
`convert_error_response` (OpenAI-конверт, статус 402 и `Retry-After` сохраняются).
Общие хелперы (`chat_error`, `invalid_request`, `unsupported_parameter`,
`convert_error_response`, `image_block` и `translate_reasoning_effort` с именем
параметра, `translate_tool_function`, `merge_or_push`, константы лимитов) —
`pub(crate)` в `anthropic.rs`. Временные ограничения (после 4.2): reasoning items
входа выбрасываются (подписи и encrypted content не выставляются — решение 4),
`store:true`/`previous_response_id`/`item_reference` → `400 documented_limitation`
(stored responses — только openai/*, решение 5).
`codex/` содержит native HTTPS transport (`transport.rs`), profile pool (`mod.rs`),
Responses/Chat adapters, tenant-bound history, Codex admission/settlement и reconstruction SSE
events; `codex/skin.rs` — Anthropic Skin (этап 5.1 docs/engine/UNIFIED_ROUTER.md, роуты
`POST /v1/messages` и `/v1/messages/count_tokens` в `ProviderMode::OpenAi`, dispatch по
модели — в router): Messages-запрос переводится в Responses JSON (strip `openai/`-префикса,
`speed:"fast"` и совместимые `service_tier:"fast"|"priority"` нормализуются в canonical
Responses `priority` до admission; effective tier возвращается в `usage.service_tier`,
top-level `system` → `instructions` со склейкой text-блоков через \n\n, user text/image →
`input_text`/`input_image` общим `canonical_image_part`, replay tool-истории — зеркало 4.2:
`tool_use` → `function_call`, `tool_result` → `function_call_output`, входные
thinking/redacted_thinking дропаются — решение 6; `tools[]` → function tools, `tool_choice`
→ default/required/none/named + `parallel_tool_calls`, `thinking` → `reasoning.effort`
lossy по порогам <4096 → low / <16384 → medium / иначе high, <1024 → 400; capability
matrix: stateful/неизвестный `cache_control` где угодно, stateful/неизвестный
`context_management`, `mcp_servers`, `container` → `400 invalid_request_error`. Exact
Claude Code `cache_control:{type:"ephemeral"}` на system/content/tools принимается и снимается:
Codex prompt caching автоматический, а расширенная retention-policy остаётся fail-closed. Текущий bounded
no-op Claude Code `context_management` (`edits:[]` либо ровно
`clear_thinking_20251015` + `keep:"all"`) принимается и игнорируется: входные thinking-блоки
этот stateless adapter и так дропает, а любое расширение формы остаётся fail-closed.
Messages GA `output_config.effort` low/medium/high честно переводится в
`reasoning.effort`, а exact `output_config.format` json_schema со schema-объектом — в
Responses `text.format`; неизвестные ключи и непредставимые формы fail-closed. Это покрывает
оба параллельных запроса Claude Code 2.1.220: structured title и основной adaptive turn.
`metadata` (включая `user_id`), sampling controls и неизвестные поля принимаются и
игнорируются — та же leniency, что у chat.rs, иначе сломался бы Claude Code с его
`metadata.user_id`) и идёт
через ТОТ ЖЕ turn pipeline, что chat.rs (admission, affinity, reserve, run, settle);
ответ переводится СНАРУЖИ — output items → Messages content blocks (message → text-блок
на позиции первого message item, function_call → tool_use, reasoning → thinking БЕЗ
signature), usage → Messages usage с cache/thinking details, stop_reason
tool_use/max_tokens/stop_sequence/end_turn; `stop_sequences` и output-бюджет ~4
chars/token честно обрабатываются на доставленном тексте общими с chat.rs
`StopFilter`/`enforce_output_limits` (транспорт не режет генерацию upstream); SSE —
`message_start` с нулевым usage (authoritative usage только в `message_delta`) → плотные
content_block start/delta/stop → `message_delta` → `message_stop`, heartbeat `event:
ping`, mid-stream отказ `event: error`, disconnect клиента не убивает turn до settlement;
все ошибки endpoint'а — Anthropic-конверт с сохранением статуса и `Retry-After` (503 →
529 `overloaded_error`, 402 сохраняется). `count_tokens` — тот же parse +
`parse_responses_request`/`prepare_turn` → reserve-grade оценка `input_tokens` без сети
(`max_tokens` опционален); сквозного e2e для Codex plane нет — покрытие
contract-тестами модуля, как 3.3/4.3; `gemini/` — native route allowlist, encrypted OAuth pool, Code Assist translation и
settlement; `gemini/chat.rs` — universal Chat Completions→generateContent адаптер (этапы 3.3–3.4b
docs/engine/UNIFIED_ROUTER.md) по той же схеме, что `anthropic.rs`: chat-запрос переводится в
GenerateContentRequest JSON (system/developer → `systemInstruction`, склейка одноролевых contents
и серий functionResponse, `maxOutputTokens` дефолт 4096, tool/function история ↔ functionCall/
functionResponse с восстановлением имени по tool_call_id, `tool_choice` → `functionCallingConfig`,
capability matrix из 18 правил (те же 16, что у Anthropic-плоскости, плюс `parallel_tool_calls`
и `user`) ПЛЮС закрытый список top-level полей — неизвестное поле
→ `400 unsupported_parameter`, т.к. Code Assist wrapper выбросил бы его молча), strip
`google/`-префикса ДО admission; вызывает общий `gemini_api()` через синтезированный внутренний
запрос на `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` — admission,
reserve, affinity, ротация, wrapper, tee-метеринг и settle без изменений; ответ переводится
СНАРУЖИ (GenerateContentResponse data-only SSE → `chat.completion.chunk` с role/content/finish/
usage-чанками и functionCall одним tool_calls-чанком, JSON → `chat.completion` с синтезируемыми
id `callu_<name>[_N]`), ошибки Google-конверта конвертируются в OpenAI-конверт с сохранением
статуса (402 тоже) и `Retry-After`, нативный `400 API_KEY_INVALID` → `401 authentication_error`.
Мультимодальность и structured output (3.4a): image_url-части user-сообщений → `inlineData`-парты
(принимаются только data: URL — исходящего fetch для внешних изображений на плоскости нет, поэтому
http(s) image URL → `400 invalid_request`; `detail` != auto → `400 unsupported_parameter`),
`response_format` json_object/json_schema → `generationConfig.responseMimeType`/`responseSchema`
(обёртка name/strict снимается). Общий рекурсивный sanitizer снимает из tool parameters и
structured-output schemas три неподдерживаемых Code Assist keyword: `$schema`, числовые
`exclusiveMinimum` и `exclusiveMaximum`; одноимённые ключи внутри `properties` остаются именами
параметров и не удаляются.
Reasoning (3.4b): `reasoning_effort` → `generationConfig.thinkingConfig`
(`thinkingLevel` проксируется как есть — маппинг уровня в wire model id выполняет
плоскость; `includeThoughts: true`; невалидное значение → `400 invalid_request`),
thought-парты ответа → `message.reasoning_content`/reasoning_content-дельты
(`thoughtSignature` не выставляется).
Replay tool-истории работает stateless: каждый восстановленный functionCall-парт получает
подтверждённый Code Assist marker
`thoughtSignature:"context_engineering_is_the_way_to_go"`. Реальные opaque signatures ответа
по решению 4 по-прежнему не выставляются и не сохраняются; synthetic ids и публичные response
shapes не меняются. Один helper обязателен для Chat, Responses и Messages skin.
`gemini/responses.rs` — universal Responses→generateContent адаптер (этап 4.3
docs/engine/UNIFIED_ROUTER.md, роут `POST /v1/responses` в `ProviderMode::Gemini`) —
Gemini-зеркало `anthropic_responses.rs`: Responses-сторона словаря 4.1+4.2 (item-формы,
события SSE, usage, status/incomplete_details) идентична Anthropic-адаптеру (contract-
тесты модуля на тех же табличных ожиданиях), перевод запроса и разбор ответа — по
правилам `gemini/chat.rs`: `instructions`/system/developer items → `systemInstruction`,
input items → contents со склейкой одноролевых, `input_image` → inlineData общим
переводом (только data: URL, http(s) → 400), replay tool-истории: function_call items →
functionCall-парты model-content'а (`arguments` JSON-строка → `args`), function_call_output
items → functionResponse-парты user-content'а (имя восстанавливается по карте
call_id→name — functionResponse ссылается по имени, output без пары →
`400 invalid_request`, в отличие от Anthropic-зеркала pairing валидируется), `tools` →
functionDeclarations (плоский дескриптор, `strict` снимается), `tool_choice` →
`functionCallingConfig`, `max_output_tokens` → `maxOutputTokens` дефолт 4096,
`reasoning.effort` → `thinkingConfig` (minimal НЕ клампится — отличие от Anthropic),
`text.format` → `responseMimeType`/`responseSchema` (json_object у generateContent есть),
capability matrix — те же 9 правил, что у Anthropic-зеркала, плюс `parallel_tool_calls`
(только дефолт true), ПЛЮС закрытый список top-level полей (неизвестное →
`400 unsupported_parameter`); `store:true`/`previous_response_id`/`item_reference` →
`400 documented_limitation` (решение 5). Ответ: thought-парты → reasoning items `rs_*` и
reasoning_summary события словаря 4.2 (thoughtSignature не выставляется), functionCall →
function_call items `fc_*` с синтезированными call_id `callu_<name>[_N]` и ровно одной
arguments-дельтой (functionCall приходит целиком), usage input=`promptTokenCount` /
output=`candidatesTokenCount`+`thoughtsTokenCount` (thoughts → `reasoning_tokens`),
finishReason/blockReason → status через общий `map_finish_reason` (MAX_TOKENS →
incomplete `max_output_tokens`, SAFETY и др. → incomplete `content_filter`); stream —
data-only SSE → Responses SSE, чистый EOF — норма протокола (`response.completed`, не
failed), mid-stream error-кадр → `response.failed`; ошибки — общий с chat-адаптером
`convert_error_response` (400 API_KEY_INVALID → 401). Общие хелперы (`chat_error`,
`invalid_request`, `unsupported_parameter`, `convert_error_response`, `merge_or_push`,
`gemini_image_part`/`translate_reasoning_effort`/`parse_tool_arguments` с именем
параметра, `function_declaration`, `code_assist_schema`, `replayed_function_call_part`,
`function_response_value`, `synthetic_call_id`,
`map_finish_reason`, константы лимитов) — `pub(crate)` в `gemini/chat.rs`.
`gemini/skin.rs` — Anthropic Skin (этап 5.2 docs/engine/UNIFIED_ROUTER.md, роуты
`POST /v1/messages` и `/v1/messages/count_tokens` в `ProviderMode::Gemini`, dispatch по
модели — в router) — Gemini-зеркало `codex/skin.rs`: Messages-сторона словаря идентична 5.1
(contract-тесты на эквивалентном входе), перевод и разбор — по правилам `gemini/chat.rs`:
top-level `system` → `systemInstruction` (склейка \n\n, не-дефолт `cache_control` → 400),
messages → contents общим `merge_or_push` (assistant → роль model; `tool_use` →
functionCall с `args` OBJECT — не JSON-строка, отличие от Codex-стороны; `tool_result` →
functionResponse, pairing по карте id→name валидируется — паттерн 3.3/4.3), image: только
base64 → inlineData (url source → 400 — generateContent ссылки не принимает), thinking
входа дропается; `tools`/`tool_choice` → functionDeclarations/functionCallingConfig
(`disable_parallel_tool_use:true` → 400 — у Gemini нет аналога); `thinking` →
thinkingConfig по порогам 5.1 (<1024 → 400) + `includeThoughts:true`; sampling
(temperature/top_p/top_k) и `stop_sequences` проксируются в generationConfig (умеет
нативно — плоскостное отличие от 5.1, где они игнорируются; stop_reason stop_sequence
неразличим → end_turn); capability matrix — те же 4 правила 5.1 + закрытый список
top-level полей (неизвестное → 400, как chat.rs). Ответ: text-парты → один text-блок,
thought-парты → thinking-блоки БЕЗ signature (thoughtSignature-only пропускается),
functionCall → `tool_use` с синтезируемым `toolu_<name>[_N]`, usage input=
`promptTokenCount` / output=`candidatesTokenCount`+`thoughtsTokenCount` (thoughts →
`output_tokens_details.thinking_tokens`, cached → `cache_read_input_tokens`); SSE — тот
же каркас 5.1 (message_start с нулевым usage → плотные content_block_* → message_delta →
message_stop, heartbeat `event: ping`, mid-stream отказ `event: error`); ошибки —
Anthropic-конверт (400 API_KEY_INVALID → 401, 503 → 529 `overloaded_error`, 402 и
Retry-After сохраняются). Хендлеры идут через общий `gemini_api()` внутренним Request на
`generateContent|streamGenerateContent?alt=sse|:countTokens` — admission, reserve,
affinity, ротация, wrapper, settle без единого изменения; `count_tokens` — нативный
`:countTokens` (quota-free, без reserve), `max_tokens` там опционален. Tool schemas и replayed
tool history используют те же shared sanitizer/context-engineering marker, что Chat/Responses;
реальные opaque signatures ответа остаются скрыты по решению 4.
Env для обоих читает только `server::config`.

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
(place/pin/immediate spill/rebind), ретраи = `pool.pick`. PostgreSQL capacity lease ниже остаётся
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

**Неограниченный client dispatch:** Claude, Codex и Gemini не имеют process/per-account/per-profile
request semaphore, локальной concurrency-очереди или concurrency-отказа. Каждый прошедший auth/money
admission запрос сразу выбирает профиль и запускает upstream attempt. In-flight счётчики живут всю
жизнь стрима, но используются только для балансировки и observability; мягкий Claude threshold
spill-ит на менее загруженную подписку и fail-open выбирает доступную подписку, если весь флот выше
порога. Безлимитный RAII task tracker нужен только для graceful shutdown: он мгновенно регистрирует
любое число уже запущенных задач, закрывает вход лишь при retirement процесса и дожидается их drain.
Provider quota/cooling по-прежнему честно дают native `429 + Retry-After`; retry/rotation разрешены
только до первого публичного байта, после него повторный upstream запуск запрещён.

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
   пускает OAuth-токены подписок — но клиент об этом знать не должен. Namespaced catalog id
   (`anthropic/<native id>`) снимается admission'ом до reserve и upstream (`strip_own_namespace`):
   universal dispatch router'а проксирует тело байт-идентично, и префикс доезжал бы до upstream
   как есть (404); зеркало strip'а chat-адаптера `anthropic.rs`. Нативный id не меняется.
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
   Current Codex top-level `tools` и legacy `additional_tools` принимают client-executed function,
   Lark custom и `tool_search` формы через один bounded parser; custom/tool-search call выполняет
   клиент, gateway возвращает raw call item и никогда не исполняет его. Hosted `web_search` не
   превращается в бесплатный client tool и fail-closed отклоняется.
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
6. **Калибровка ёмкости окна — native credits отдельно от API USD.** Decimal `used_percent` из
   `/wham/usage`/headers парсится без `f64` в `10^-8` fraction units. Каждый успешный turn до ingest
   quota snapshot строит один immutable dual-ledger event: effective Standard/Fast, модель,
   provider tier, fresh/cache-read/cache-write/output/reasoning counters, exact API nanoUSD и exact
   ChatGPT nanocredits. Reasoning уже входит в output, cached input — subset total input; повторно
   они не складываются. Стабильный внутренний `cal_*` request ID создаётся до выбора home и живёт
   через transport/home retries, но никогда не уходит upstream. Registry атомарно двигает оба
   cumulative ledger; exact retry идемпотентен. Для новых unpinned-разговоров normal selection
   сначала seed'ит каждый здоровый home без единого immutable turn; это только tie-break после
   Fast/freshness/in-flight и никогда не перебивает уже resolved affinity.

   Failed events остаются в bounded FIFO (4096), повторяются перед новыми и независимо дренируются
   каждым health sweep даже без нового customer turn; retire делает последний flush после закрытия
   входа новым turn. После writer recovery сначала durable становятся exact events и оба cumulative
   ledger, и только затем повторяется cached post-turn quota snapshot — обратный порядок ложно
   превращал реальный gateway spend во внешний. Pending/drop видны в `/codex-subs`; overflow не
   молчит. Permanent immutable replay conflict карантинит только одну строку и не блокирует
   последующие. Estimator v10 после credit cutover начинает shared anchor для
   обоих units: `native cap = 100_000_000*ΣΔnanocredits/ΣΔfraction`, API cap остаётся realized
   workload equivalent по `ΣΔnanoUSD`. Старое API evidence переносится в `last_*`, а не считается
   нулевым credit spend. Первое quota-only движение ждёт ledger catch-up; повторившееся движение без
   обоих ledger помечается `possibly unattributed`, но не объявляется внешним использованием.
   При одноразовом replay новой версии estimator legacy API-only snapshot, ошибочно появившийся
   после credit cutover, остаётся в raw authority, но пропускается как неполный: следующий tracked
   cumulative snapshot безопасно охватывает этот интервал. Live tracked→untracked regression
   по-прежнему fail-closed.
   Storage `10^-8` не считается точностью provider measurement: trailing-zero resolution каждого
   endpoint (целый процент = `1_000_000` units) входит в low/high, а interval не больше rounding
   uncertainty получает `high:null`. `/codex-subs.plan_cohorts` группирует exact paid plan +
   duration и делит pooled native-credit evidence на pooled fraction movement, публикуя одну
   capacity per home и fleet remaining; individual estimates/evidence не перезаписываются, API USD
   не pool-ится. Low/high, samples, confidence и missing-data reason публикуются явно. Нет
   prior/EMA/WLS/float money. Raw observations переживают restart/blue-green/reset и распознают rolling reset;
   каждое provider-reported duration калибруется независимо. Usage translation принимает оба
   реально встречающихся alias (`cache_write_tokens` и legacy `cache_creation_tokens`), предпочитая
   current spelling и никогда не складывая их дважды.
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
   Tokens/full email/domain/project/tier/proxy дешифруются только в память и не попадают в
   log/metric/response; protected `/gemini-subs` получает только заранее выведенный bounded hint из
   четырёх символов local-part.
1a. **Мёртвым credential объявляет только Google, и только словом `invalid_grant`.** Отказ refresh
   классифицируется по телу ответа, а не по коду: `400 invalid_grant` → `TokenError::Invalid`
   (профиль снимается с ротации), `401`/`403` → `TokenError::Blocked` (grant цел, отклонило
   окружение — репутация IP прокси или блок клиента; профиль остаётся authenticated и лишь
   остывает на `auth_quarantine_secs`), остальное → `Temporary`. Раньше все три схлопывались в
   `Invalid`, и живая оплаченная подписка навсегда уходила из ёмкости с красным «ошибка auth» по
   причине, которой в токене нет. Отказ логируется bounded-строкой `profile/http/error/verdict` —
   без токена, прокси и текста Google.
2. `GeminiGateway` обслуживается только startup-fixed `ProviderMode::Gemini`. Native allowlist:
   models get/list, generateContent, streamGenerateContent, countTokens. Клиентский `x-goog-api-key`
   (как и x-api-key/Bearer) авторизует наш ключ, но никогда не уходит Google; query `key`/`api_key`,
   включая percent encoding, запрещён.
3. Production HTTPS принадлежит persistent per-profile Node helper: exact pinned
   `/usr/bin/node` v24.18.0 Linux/x64 + SHA-256, native OpenSSL, HTTP/1.1 и authenticated CONNECT.
   Новые profiles обычно используют live-проверенный Antigravity 2.2.1 UA,
   `Go-http-client/2.0` refresh и reviewed bounded Antigravity
   `Client-Metadata`/`x-goog-api-client`; caller values вырезаются. Dormant explicit-test route
   `gemini-3-flash-preview` сохраняет compile-fixed UA подписанного Antigravity 2.4.3 без старых IDE
   `Client-Metadata`/`x-goog-api-client` только для воспроизводимости исследования. Финальный
   minimal-header micro-smoke сохранил 404 без usage, поэтому production/public allowlist эту модель
   не содержит. Все работающие модели и background quota/health сохраняют полный live-проверенный
   tuple.
   Старые Gemini CLI credentials сохраняют прежний wire до миграции.
   OAuth userinfo использует отдельный global-fetch/Undici профиль того же SHA-pinned Node. Никакой
   approximate BoringSSL impersonation или ambient proxy/env.
   Antigravity text обычно сохраняет live-verified configured endpoint; dormant explicit-test route
   `gemini-3-flash-preview` сохраняет compile-fixed `daily-cloudcode-pa.googleapis.com`, хотя и
   sandbox, и этот origin возвращают 404 на generation. Image generation всегда идёт на
   production `cloudcode-pa.googleapis.com`, как официальный LS: sandbox публикует image quota row,
   но отвечает 503 на генерацию. Literal loopback mocks не перенаправляются.
   Helper получает proxy secret только первым IPC frame, multiplexes bounded NDJSON, reaps process
   group и может restart-нуться только до upstream headers. Outbound frames, inbound NDJSON/base64
   staging, OAuth response collections и short-lived header/form strings zeroized. Loopback mocks
   остаются на `wreq`. Helper отдельно классифицирует target `timeout`/`tls`/`network` и bounded
   CONNECT-причины `proxy_timeout`, `proxy_auth`, `proxy_throttle`, `proxy_rejected`,
   `proxy_upstream`, `proxy_connect`, `proxy_eof`, `proxy_protocol`; runtime сводит все proxy/TLS
   классы к существующей network policy, не принимая их за IPC protocol corruption и не раскрывая
   status/header/credentials.
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
   Canonical Gemini 3 model id отдельно от private effort/quota id: dormant test support для
   3 Flash Preview отправляет публичный `gemini-3-flash-preview` без wire-подмены, но Antigravity
   `requestType=agent` проверяет по quota row `gemini-3-flash-agent` (legacy Gemini CLI сохраняет
   публичный quota id); production allowlist этот rejected route не публикует. 3.6 Flash выбирает
   `gemini-3.6-flash-{low,medium,high}`, 3.1 Pro Preview —
   `gemini-3.1-pro-low`/`gemini-pro-agent`.
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
   settle-ить последний snapshot и только потом отпустить background task guard для
   последующего billing flush.
   Per-profile in-flight не имеет потолка и служит только сигналом балансировки. Resolved
   conversation affinity — hard first choice при любой локальной нагрузке; unbound fan-out сразу
   распределяется по наименее загруженным eligible profiles. Новая shared
   system/tools cache-root сначала прогревает два конкурентных profile, затем предпочитает warm
   copy. Unbound routing ставит fresh quota evidence перед stale, затем inflight, coarse quota
   steering только выше 50% used и rotating cursor: exact fractions не herd-ят бёрст на один
   аккаунт. Deterministic soft reserve/jitter сохраняется; если все eligible profiles ниже резерва,
   service floor fail-open до explicit zero. Локальное saturation никогда не становится public
   ошибкой; native RetryInfo остаётся только для реальной provider quota/cooling.
   `/gemini-subs` отделяет quota presence от generation health через failure streak и last
   success/failure evidence, отдаёт reviewed paid-plan identity и bounded email hint (первые четыре
   символа local-part без домена), но никогда Google subject/full email/project или private tier.
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
   исключены. Каждый successful generation с terminal usage (billed или admin) строит immutable
   provider event: internal request id, opaque profile, exact paid plan/model/tariff, все token/tool/
   search facts и disjoint official API nanoUSD legs. Событие и cumulative subject spend пишутся
   атомарно; missing usage не создаёт evidence. Доставка — отдельный bounded FIFO 4096 с retained
   transient head, immutable replay, one-row conflict quarantine, poll-before-observation flush,
   pending/drop/persistence diagnostics и shutdown drain.
   Exact window authority ключуется `profile + plan + bucket + duration`; legacy rows без plan не
   мигрируются. Окна независимы, provider fraction хранится fixed-point `10^-8` вместе с реальным
   lexical decimal resolution. Cold snapshot — anchor, а первый complete positive-spend interval
   сразу публикует realized blend `SCALE*ΣΔspend/ΣΔused`. Low/high учитывают resolution обоих
   endpoints; high остаётся `null`, если движение не превосходит uncertainty. Quota может ждать один
   snapshot settlement lag, повторное quota-only движение становится unattributed. Reset/rolling
   rollover/jitter, overflow и estimator rebuild из immutable history fail closed. Prior/EMA/WLS/
   nominal/float money нет. Admin-only exact targeting принимает полный opaque profile id и
   optional canonical `x-apitoken-calibration-request-id`; metered traffic не может задать ни
   профиль, ни immutable-event identity, а target никогда не spill/rebind-ится.
8. Полный контракт/provisioning/runbook — `docs/engine/GEMINI_PROVIDER.md`. Проверка включает mock upstream:
   rotation fault matrix, credential stripping, RetryInfo, chunk-split SSE, no post-byte retry,
   disconnect drain+settlement и shutdown deadline barrier.


**Тюнинг под живой Anthropic** (identity/beta/UA/version) — через поля `ProxyConfig`, которые
`server` берёт из env. Значения по умолчанию — в `config.rs`.

**Проверка:** `cargo build -p forward`; полный smoke — через бинарь против мок-апстрима
(`tests/rotation_fanout_smoke.sh`; universal chat lane end-to-end — `tests/universal_chat_smoke.sh`).

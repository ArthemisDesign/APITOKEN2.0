# Unified router production-readiness audit — 2026-08-03

Статус: **snapshot, append-only**. Этот документ фиксирует состояние на
`origin/master` SHA `18567df6427c40114b04e5df3d81860d1aff11ed`. Последующие исправления не
переписывают выводы аудита задним числом: их SHA и итоговый повторный аудит оформляются отдельно.

## Итог

Основные и harness-happy-path сценарии unified router работают убедительно: все три provider
plane, три публичных wire-протокола, инструменты, reasoning, GPT Fast, персональная проекция цен и
production delivery прошли широкий набор автоматических и живых проверок. Однако выпуск нельзя
считать полностью production-ready для недоверенного массового трафика: найдены три блокера и
шесть high-risk дефектов на negative/resource/exact-roundtrip путях.

Главные риски:

1. несколько параллельных неавторизованных больших universal-запросов способны исчерпать память
   singleton-router до проверки ключа;
2. некорректные output-limit значения на части адаптеров fail-open превращаются в default, а GPT
   Chat с нулевым лимитом может оплатить невидимый клиенту output;
3. обновление singleton-router делает restart с окном недоступности и риском обрыва длинных SSE;
4. Gemini принимает существенно более узкое подмножество JSON Schema, чем сейчас гарантирует
   адаптер;
5. несколько точных wire-семантик — reasoning replay, тип `stream`, terminal SSE — расходятся с
   OpenAI/Anthropic-compatible контрактом.

Это не противоречит зелёной harness-матрице: она хорошо проверяет корректные запросы и реальные
agent loops, но почти не атакует границы ресурсов, неправильные JSON-типы, оборванные streams и
точный replay выданных router-расширений.

## Область и методика

Проверены:

- `crates/router`, universal adapters в `crates/forward`, catalog/policy/fallback boundaries;
- native и translated Chat Completions, Responses, Messages, Gemini Developer API;
- auth, body limits, request/response streaming, output controls, tool schemas, reasoning replay;
- персональная catalog pricing projection и преобразование каталога в OpenCode;
- systemd/Caddy/watchdog delivery path;
- contract/unit/workspace tests, mock smokes и bounded production probes без раскрытия
  credentials или содержимого запросов.

Базовый успешный набор доказательств:

- `cargo build --locked`;
- `bash deploy/sccache-cargo.sh cargo test --locked --workspace`;
- 91/91 тестов `crates/router`, 736 passed + 1 ignored Redis test в `crates/forward`;
- `tests/universal_chat_smoke.sh` и `tests/router_fallback_smoke.sh`;
- shell/docs/diff static gates;
- native production matrix 21/21;
- 21 исполнимый Standard/Fast harness-case;
- реальные OpenCode tool cycles на Claude/GPT/Gemini, Claude effort `xhigh|max`, GPT Fast;
- Cline, Continue, Kilo, Codex CLI, Claude Code, Gemini CLI, Hermes и Aider.

Destructive memory/load test на production намеренно не запускался. Resource-risk подтверждён
порядком операций в коде и лимитами systemd. Production probes ниже были bounded и использовали
минимальный output.

## Реестр находок

| ID | Severity | Поверхность | Краткий эффект |
|---|---|---|---|
| UR-01 | P0 blocker | router universal POST | неавторизованный memory DoS до auth |
| UR-02 | P0 blocker | translated Chat/Responses | fail-open output limits и риск неверного billing/delivery |
| UR-03 | P0 blocker | deployment | restart singleton даёт окно 502/обрыва SSE |
| UR-04 | P1 high | Gemini tools/structured output | неполный JSON Schema compatibility boundary |
| UR-05 | P1 high | GPT Chat replay | router не принимает собственный exact reasoning-only output shape |
| UR-06 | P1 high | Chat/Messages adapters | неправильный тип `stream` принимается как `false` |
| UR-07 | P1 high | Anthropic/Gemini translated SSE | оборванный или malformed stream может выглядеть успешно |
| UR-08 | P1 high | `/v1/models` | теряются authoritative limits/capabilities, клиент угадывает |
| UR-09 | P1 high | OpenCode cost | cache-write и произвольный long-context threshold считаются неточно |
| UR-10 | P2 medium | OpenCode plugin | transient catalog failure оставляет запуск без discovered models |
| UR-11 | P2 medium | router transport | нет deadline ожидания response headers после TCP connect |
| UR-12 | P2 medium | catalog aliases | collision выбирается порядком плоскостей, а не отклоняется |
| UR-13 | P2 release gap | fallback | реализация остаётся production default-off без GA canary |
| UR-14 | P2 coverage gap | Roo Code | transport проверен, но настоящий live-case не автоматизирован |

## Подробные находки

### UR-01 — P0: неавторизованный memory DoS

`crates/router/src/routing.rs::proxy_universal` сначала вызывает `to_bytes(body, 32 MiB)`, затем
парсит JSON и только после этого получает auth-ответ через catalog/policy/provider plane. Это
относится к `/v1/chat/completions`, `/v1/responses`, `/v1/messages` и
`/v1/messages/count_tokens`.

У router нет общего бюджета одновременно материализованных request bodies. Юнит
`systemd/claude-router.service` ограничен `MemoryMax=512M` и является единственной публичной
точкой входа на `127.0.0.1:8798`. Следовательно, небольшой набор параллельных запросов близко к
32 MiB может вытеснить процесс по памяти ещё до того, как невалидный ключ будет отвергнут.

Требуемое исправление: bounded auth preflight до чтения большого body и отдельный ограниченный
admission-бюджет для буферизуемых universal bodies. Он не должен становиться общей очередью
исполнения между provider planes: после materialization запрос освобождает body permit и дальше
сохраняет существующую процессную изоляцию.

Acceptance: неавторизованный клиент не может заставить router материализовать 32 MiB; сумма
одновременно удерживаемых universal bodies ограничена существенно ниже `MemoryMax`; overload
получает lane-shaped 429/503 без обращения к billable endpoint; authenticated native SSE не
буферизуется.

### UR-02 — P0: output-limit parsing и billing/delivery расходятся

На Anthropic Chat/Responses и Gemini Chat/Responses `max_tokens`,
`max_completion_tokens` или `max_output_tokens` извлекаются через `Value::as_u64`; ноль,
отрицательное число, строка и другой неправильный тип неотличимы от отсутствующего поля и
превращаются в default `4096`.

GPT Chat строже к типу, но принимает `0`: `parse_max_output_tokens` возвращает `Some(0)`, затем
canonical Responses request не получает upstream cap из-за фильтра `> 0`, а локальный delivered
text budget становится нулевым. Upstream способен сгенерировать и тарифицировать output, который
клиенту не будет доставлен.

Требуемое исправление: единый принцип на всех translated surfaces — missing/null имеет
документированный default/absence, а любое присутствующее non-null значение обязано быть
положительным целым в поддерживаемом диапазоне; иначе локальный 400 до reserve/upstream. Тесты
должны различать `missing`, `null`, `0`, `-1`, `1.5`, строку, object и overflow.

### UR-03 — P0: deployment outage singleton-router

`systemd/claude-router.service` запускает один процесс на одном порту, имеет
`TimeoutStopSec=30`, а `deploy/deploy.sh::restart_router_if_changed` выполняет
`systemctl restart claude-router.service`. Старый процесс прекращает принимать новые соединения,
и только после его завершения или stop-timeout запускается новый. Caddy всё это время имеет один
upstream.

Новые запросы получают 502/connection failure, а stream длиннее drain window может быть оборван.
Текущий комментарий юнита явно считает reconnect клиента достаточным, но это не соответствует
целевому provider-grade контракту.

Требуемое исправление: два router slot на разных loopback ports, health-gated promotion в Caddy,
exact-binary verification активного slot и drain старого процесса после cutover. Нельзя смешивать
это со Stage 3 multi-host HA: здесь устраняется локальное deployment-окно на текущем host.

### UR-04 — P1: Gemini JSON Schema sanitizer не описывает реальное подмножество

`crates/forward/src/gemini/chat.rs::code_assist_schema` рекурсивно удаляет только `$schema`,
числовые `exclusiveMinimum` и `exclusiveMaximum`. Это исправляет штатные AI SDK/bash schemas, но
не делает legal JSON Schema совместимой с private Code Assist parser.

Bounded production probes на `google/gemini-3.6-flash` получили `400 INVALID_ARGUMENT` ещё для
восьми legal constructs: `$defs`, `const`, `patternProperties`, `dependentRequired`,
`unevaluatedProperties`, `propertyNames`, `if/then`, `minContains`. Эквивалентные Anthropic tool
schemas были приняты. Одинаковый helper используется Chat, Responses и Messages skin, а также
structured output, поэтому радиус больше одного OpenCode tool.

Простое удаление всех неизвестных keywords неприемлемо: оно молча ослабит contract функции
(существующее удаление exclusive bounds уже делает это). Нужен явный Code Assist supported-subset
translator: эквивалентно представимые формы переводить, декоративные annotations можно снимать,
непредставимые validation constraints отклонять локальным 400 с точным schema path.

### UR-05 — P1: GPT Chat не replay-safe для собственного `reasoning_content`

Router публикует OpenAI-compatible extension `message.reasoning_content`, однако
`crates/forward/src/codex/chat.rs::translate_messages` его не читает. Production probe:

- assistant `{content:null, reasoning_content:"..."}` → 400
  `Assistant message requires content or tool_calls`;
- тот же turn с `content:""` → 200.

OpenCode проходит этот сценарий только потому, что текущая версия AI SDK сериализует пустую строку.
Другой совместимый клиент, точно replay-ящий JSON ответа, падает. Требуемое поведение должно
совпасть с Anthropic/Gemini universal решением: `reasoning_content` display-only, reasoning-only
assistant turn безопасно опускается/склеивается, действительно пустой assistant остаётся 400.

### UR-06 — P1: неправильный JSON-тип `stream` fail-open принимается как false

Anthropic Chat, Gemini Chat и Gemini Messages skin используют
`get("stream").and_then(Value::as_bool).unwrap_or(false)`. Поэтому присутствующее
`stream:"false"` становится non-stream запросом вместо локального 400. Bounded production probes
подтвердили 200 для Anthropic Chat, Gemini Chat и translated Messages; GPT Chat корректно вернул
400.

Исправление: общий strict optional-bool parser — missing/null допускаются по контракту, любой
другой тип получает lane-shaped 400 с `param=stream` там, где envelope это поддерживает.

### UR-07 — P1: translated stream может скрыть незавершённость

Anthropic Chat translator при чистом EOF без обязательного `message_stop` добавляет `[DONE]`.
Regression test `sse_clean_eof_without_message_stop_still_terminates_with_done` закрепляет именно
это поведение. Anthropic Responses тот же protocol break уже корректно переводит в
`response.failed`.

Кроме того, Chat Gemini и часть translated SSE parsers пропускают malformed provider frames. В
результате клиент может принять частичный ответ за успешный или потерять terminal error. Transport
ошибка уже обрабатывается честнее; пробел относится к clean premature EOF и malformed frames.

Исправление: обязательная terminal-state машина для каждого source protocol. Anthropic stream
успешен только после `message_stop`; malformed обязательный event/frame даёт публичный error без
`[DONE]`. Для Gemini чистый EOF допустим только если его собственная семантика и полученный
finish/usage state достаточны; нельзя переносить Anthropic terminal marker на Gemini механически.

### UR-08 — P1: агрегированный каталог выбрасывает authoritative capabilities и limits

Native Anthropic `/v1/models` уже возвращает `max_input_tokens`, `max_tokens` и подробные
capabilities/effort matrix. В живом каталоге наблюдались, например, 1,000,000/128,000 для Opus 5,
Sonnet 5 и Sonnet 4.6, но 200,000/64,000 для Haiku 4.5. Native Gemini catalog также возвращает
точные `inputTokenLimit`/`outputTokenLimit`.

`crates/router/src/catalog.rs` проецирует записи в общий минимальный shape и добавляет лишь
router-owned `reasoning_efforts`/`service_tiers`. Локальный
`~/.config/opencode/plugin/apitoken-router.js` поэтому хардкодит всем Claude 200k/64k и повторно
угадывает Gemini/GPT limits по model ID.

Требуемое исправление: provider plane catalog остаётся authority и producer-first публикует
нормализованные expand-only capabilities/limits; router валидирует и агрегирует их; OpenCode plugin
потребляет поля, а не модельные таблицы. Unknown/legacy plane не должен получать выдуманные
значения.

### UR-09 — P1: штатный OpenCode cost не может точно представить весь billing contract

`@ai-sdk/openai-compatible@2.0.41` использует
`prompt_tokens_details.cached_tokens`, но не представляет Anthropic cache-write usage
(`cacheWrite: undefined` в provider usage mapping). Cache creation поэтому считается обычным
input, хотя его ставка может быть выше на 20–50%.

OpenCode 1.18.11 имеет только фиксированный cost bucket `context_over_200k`. Plugin заполняет его
только при `threshold_tokens === 200000`; GPT long-context threshold 272000 нельзя выразить этой
schema. Серверный settlement и `apitoken.pricing` при этом остаются корректными — неточна только
клиентская оценка OpenCode.

Одним расширением `/v1/models` это не исправить. Нужен специализированный OpenCode provider/plugin,
который считает raw usage по `cache_creation_input_tokens`/другим provider details и применяет
произвольный `long_context.threshold_tokens`, либо upstream-изменение OpenCode/AI SDK schema. Цены
и тарифные значения в рамках этого исправления менять нельзя.

### UR-10 — P2: OpenCode discovery не имеет last-good local catalog

Plugin fetch-ит key-scoped `/v1/models` при старте с 10-second timeout. При любой transient ошибке
он оставляет `models={}` и не устанавливает provider models. Цены нельзя кэшировать между ключами,
но capability metadata и зашифрованный/key-bound last-good snapshot для того же credential
допустимы. Нужны TTL/identity/version guards и явная stale-индикация; нельзя молча использовать
ставки другого ключа.

### UR-11 — P2: нет deadline до response headers

Общий router `reqwest::Client` имеет только `connect_timeout(2s)` и намеренно не имеет total
timeout ради длинных SSE. После успешного TCP connect plane может зависнуть до response headers
без ограничения; клиентский disconnect является единственным выходом.

Нужен timeout только на фазу ожидания headers/first response, не на body stream. Его истечение
остаётся ambiguous и не разрешает межмодельный fallback или повтор billable request.

### UR-12 — P2: alias collision не валидируется

Агрегированный каталог поддерживает native aliases и поиск первого совпадения. Межпровайдерная
коллизия не отклоняется и зависит от стабильного порядка агрегации. На audited production catalog
23 aliases и 0 collisions, поэтому текущего инцидента нет, но публикация новой модели способна
тихо изменить routing существующего alias.

Исправление: aggregate snapshot должен проверять глобальную уникальность aliases. Коллизия
помечает затронутые plane/catalog entries degraded или снимает ambiguous alias; namespaced IDs
остаются исполнимыми. Нужен contract test с двумя plane, отдающими один alias.

### UR-13 — P2: fallback готов технически, но не прошёл GA rollout

Fencing, policy preflight, provider preferences, presets, bounded telemetry и mock-load реализованы,
но production `CLAUDE_ROUTER_FALLBACK_ENABLED` остаётся default-off. Это не runtime defect текущего
single-model контракта, а незавершённая продуктовая возможность.

До GA обязательны exact deployed-binary canary, подтверждение single-winner/zero-loser money
инвариантов, отсутствие утечки internal headers и отдельный config-only enable с наблюдением
алертов. Не включать fallback как побочный эффект других исправлений.

### UR-14 — P2: Roo Code live coverage остаётся SKIP

Roo Code 3.54.0 использует совместимый `@ai-sdk/openai-compatible` transport и его настройки
base URL/model/service tier проверены статически. У расширения нет официального headless CLI,
поэтому production harness честно отмечает case `SKIP`; имитация через Kilo/OpenCode не является
эквивалентным live доказательством.

Это coverage gap, а не известный transport defect. Закрытие возможно через поддерживаемый Roo
automation API/extension host либо ручной bounded release-case с воспроизводимым evidence.

## Недостающая regression matrix

Новые тесты должны закрыть не отдельные literals, а классы ошибок:

1. **Resource/auth:** invalid/missing credential + slow 32 MiB bodies, authenticated concurrency,
   permit release при parse error/disconnect/panic, отсутствие глобальной provider queue.
2. **Strict JSON types:** nullable/invalid matrix для output limits, `stream`, stream options и
   всех трёх translated protocol skins.
3. **Exact replay:** каждый публичный assistant/reasoning/tool output shape возвращается следующим
   запросом с `null`, пустыми и отсутствующими content forms.
4. **Terminal streams:** valid terminal, clean premature EOF, malformed event JSON, provider error
   event, transport error, frame split across chunks; `[DONE]`/completed только после доказанного
   terminal state.
5. **Gemini schemas:** supported subset, nested schema-bearing keywords, property names, каждый
   известный rejected construct и schema-path error; одна matrix для Chat/Responses/Messages.
6. **Catalog:** authoritative limit/capability propagation, missing legacy metadata, alias collision,
   partial plane outage, key-scoped pricing isolation.
7. **Harness costs:** cache read/write, 5m/1h classes, arbitrary long-context threshold, Standard/Fast,
   exact server settlement против клиентской оценки.
8. **Deployment:** inactive router readiness failure, exact-binary mismatch, promotion, rollback,
   long SSE drain и новые соединения во время cutover.

## Порядок устранения

Каждый пункт ниже должен идти отдельным task-worktree от свежего `origin/master`, отдельным
коммитом/merge и зелёным `deploy/watchdog` перед следующим зависимым пакетом:

1. UR-01 + UR-11: resource/auth admission и bounded pre-header timeout.
2. UR-02 + UR-06: единая strict validation matrix.
3. UR-05 + UR-07: exact reasoning replay и terminal stream integrity.
4. UR-04: Gemini supported-subset translator.
5. UR-08 + UR-12: authoritative catalog metadata и collision safety, producer-first если меняется
   plane→router contract.
6. UR-09 + UR-10: OpenCode client cost/discovery; если upstream schema нерасширяема, зафиксировать
   специализированный provider contract, а не объявлять неточность устранённой.
7. Полная negative/live regression matrix и повторный audit.
8. UR-03: blue-green router deployment отдельным operational пакетом.
9. UR-13 canary/GA enable — только после зелёного повторного аудита.
10. UR-14 закрыть при появлении официально автоматизируемой Roo surface; до этого сохранить честный
    `SKIP`.

## Не затронуто

- Тарифные значения, pricing releases, скидки и multipliers не менялись и не должны меняться в
  рамках remediation этого аудита.
- Старые per-provider domains остаются рабочими и не используются как оправдание дефектов unified
  endpoint.
- Multi-host Stage 3 HA не входит в этот аудит; UR-03 требует только zero-downtime локального
  router deployment.
- Production fallback не включается до отдельного canary и явного GA шага.

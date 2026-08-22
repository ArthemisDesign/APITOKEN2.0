# Аудит совместимости с Claude Code 2.1.231/2.1.239

- **Дата среза:** 2026-08-21 UTC
- **Ревизия проекта:** `c25a5adc7eef3b6498c8842cbed90377c64c67f4` (`origin/master` на момент начала аудита)
- **Официальные каналы клиента:** npm `stable=2.1.231`, `latest=2.1.239`, `next=2.1.239`
- **Предыдущая проектная baseline:** Claude Code 2.1.220
- **Область:** Claude Code с `ANTHROPIC_BASE_URL` → единый router → Anthropic/OpenAI/Gemini Messages planes; отдельно — native Anthropic subscription plane
- **Граница доказательства:** локальные exact-client probes против loopback mock, статический анализ и focused tests; production, подписки и платные запросы не использовались

## Резюме

Текущий контур **совместим с Claude Code 2.1.231 и 2.1.239 для базового Anthropic Messages turn**:

- клиент отправляет `POST /v1/messages?beta=true`;
- `x-api-key` и Bearer принимаются без приоритета одного вида credential над другим;
- `anthropic-version` и клиентский открытый список `anthropic-beta` сохраняются;
- текущая форма `thinking`, `context_management`, `output_config`, `metadata`, `tools` принимается native Claude plane;
- Claude Code → GPT через Messages skin принимает наблюдаемую форму `context_management`, `output_config.effort`, ephemeral cache markers и structured-output tool;
- SSE не буферизуется router'ом, а Messages lifecycle проходит существующие tests;
- `/v1/messages/count_tokens` реализован;
- `/v1/models?limit=1000` реализован и доступен при opt-in discovery.

Exact loopback captures 2.1.220, 2.1.231 и 2.1.239 в одинаковом Standard/low сценарии показали одинаковый набор верхнеуровневых body fields и одинаковый набор активных beta capabilities. Поэтому обновление **не содержит доказанного blocker обычного turn** относительно 2.1.220.

Полную совместимость с актуальными каналами заявлять нельзя. Найдены восемь расхождений:

| ID | Приоритет | Кратко | Эффект |
|---|---|---|---|
| CC-01 | **HIGH** | Acceptance остаётся на незакреплённом Claude Code 2.1.220 | Gate не доказывает `stable` или `latest` и не проверяет native Claude plane реальным Claude Code |
| CC-02 | **HIGH** | `refresh-fingerprint.sh` неверно разбирает новый суффикс `cc_version` | После refresh получится несуществующий составной fingerprint вида `2.1.239.0f1.dNN` |
| CC-03 | **MEDIUM** | Unified discovery отдаёт `name`, клиент читает `display_name` | `/model` теряет человекочитаемые названия gateway-моделей |
| CC-04 | **MEDIUM** | `/v1/models` не ограничен официальным трёхсекундным бюджетом | При деградации pricing discovery молча истекает у клиента |
| CC-05 | **MEDIUM** | Synthetic errors не имеют body `request_id`; router 413 имеет неверный `error.type` | Диагностика и exact Anthropic error contract расходятся |
| CC-06 | **MEDIUM** | Native plane заменяет первый attribution system block | Нарушен официальный unchanged/order contract; attribution и cache identity принадлежат gateway persona, а не клиентской сессии |
| CC-07 | **MEDIUM · CONDITIONAL** | Non-Claude Messages skins закрывают известные control shapes | Новый `output_config`/`context_management` Claude Code может дать local 400 вместо feature pass-through |
| CC-08 | **LOW** | Документация зафиксировала старый discovery filter и противоречивый UA format | Operator и следующий аудит получают неверную конфигурационную baseline |

**Финальный вердикт:** `2.1.231` и `2.1.239` пригодны для текущего базового Messages/SSE сценария, но статус проекта — **частичная совместимость, exact latest acceptance не закрыт**.

## 1. Как определена актуальная версия

На момент среза официальная npm metadata возвращала:

```text
stable  2.1.231  published 2026-08-13T08:27:21.757Z
latest  2.1.239  published 2026-08-21T17:18:54.506Z
next    2.1.239
metadata modified 2026-08-21T19:54:06.250Z
```

Это важное различие: npm одновременно держит консервативный `stable` и более новый `latest`. Аудит проверяет оба, а не называет один из них единственной «последней стабильной» версией.

Официальные wrapper и native packages были получены через npm. Проверенные SHA-256:

| Артефакт | SHA-256 |
|---|---|
| wrapper `@anthropic-ai/claude-code@2.1.231` tarball | `36ffb8163cef84434513fc16ce2798fbc07df88b4fe863fdd27d8c4b72120449` |
| native `2.1.231` Darwin arm64 binary | `ba790279cab6ef77b713864d4bf5f764fcea87d3a3eb7591a41f741e45212b5c` |
| wrapper `@anthropic-ai/claude-code@2.1.239` tarball | `6b4798c7b4fa4f6b34f51aa043d2670773e339482158cf5d84496e5005d0fc20` |
| native `2.1.239` Darwin arm64 binary | `2b4f7aafdaa65bcc2335f56a4b276317837203f2c5587b1f2a17ca78ad14e36f` |

Native binaries отдельно вернули `2.1.231 (Claude Code)` и `2.1.239 (Claude Code)`. Локально установленный `claude`, которым пользуется существующий project harness, вернул `2.1.220 (Claude Code)`.

## 2. Официальный gateway contract

Основной первичный источник — официальный [Gateway protocol reference](https://code.claude.com/docs/en/llm-gateway-protocol.md). Для `ANTHROPIC_BASE_URL` он определяет:

1. Обязательный `POST /v1/messages`.
2. Опциональный `POST /v1/messages/count_tokens`.
3. Inference query `?beta=true`; route должен сопоставлять path, а не полный URL.
4. Best-effort `HEAD /api/hello`, отказ которого не ломает работу.
5. Опциональный discovery `GET /v1/models?limit=1000` с трёхсекундным timeout и запретом redirects.
6. `anthropic-version` и `anthropic-beta` должны доходить до Anthropic-format upstream без allowlist; beta set меняется между релизами.
7. Headers и body fields являются открытыми списками.
8. Первый Claude Code attribution block в `system` должен сохранять позицию и форму, если gateway проксирует его в `api.anthropic.com`.
9. SSE должен идти инкрементально; `ping` нельзя буферизовать или выбрасывать.
10. Upstream error body нельзя оборачивать: recovery Claude Code зависит от текста некоторых ошибок.
11. Discovery читает `data[].id` и опциональный `data[].display_name`.
12. С 2.1.223 discovery сохраняет ID, **содержащие** `claude` или `anthropic` в любом месте без учёта регистра, а не только начинающиеся с них.

Дополнительные первичные источники:

- [Claude Code changelog](https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md)
- [Messages API](https://platform.claude.com/docs/en/api/messages.md)
- [Streaming Messages](https://platform.claude.com/docs/en/build-with-claude/streaming.md)
- [Errors and request limits](https://platform.claude.com/docs/en/api/errors.md)
- [Gateway connection setup](https://code.claude.com/docs/en/llm-gateway-connect.md)
- [Beta headers](https://platform.claude.com/docs/en/api/beta-headers)

### Изменения после project baseline 2.1.220

Наиболее значимые для нашего контура официальные изменения:

- **2.1.222:** custom-base stream watchdog начал учитывать gateway keep-alive traffic.
- **2.1.223:** provider-prefixed discovery IDs теперь проходят по вхождению `claude`/`anthropic`.
- **2.1.225:** добавлена gateway spend-limit UX.
- **2.1.229:** gateway keep-alive для длинного thinking; исправлена работа 1M моделей через custom base; уточнена 32 MiB request boundary.
- **2.1.232:** улучшено восстановление после stream idle error в gateway/provider modes.
- **2.1.237:** исправлен prompt caching для LLM gateway/custom base.
- **2.1.239:** исправлен Bedrock streaming за proxy, снимающим response `Content-Type`; это не прямой Anthropic custom-base delta, но усиливает значимость сохранения media type.

Большинство остальных изменений 2.1.221–2.1.239 относятся к TUI, sessions, hooks, MCP, Remote Control и local tools. Они не создают отдельные HTTP endpoints на обычном custom `ANTHROPIC_BASE_URL`.

## 3. Exact-client loopback evidence

Три реальные версии Claude Code были запущены с:

- placeholder `ANTHROPIC_API_KEY`;
- loopback `ANTHROPIC_BASE_URL`;
- пустым временным `CLAUDE_CONFIG_DIR`;
- отключёнными updater, telemetry и error reporting;
- synthetic корректным Messages SSE response;
- без production, subscription token и платного upstream.

### 3.1 Обычный Standard/low turn

| Поле | 2.1.220 | 2.1.231 | 2.1.239 |
|---|---|---|---|
| Path | `/v1/messages?beta=true` | тот же | тот же |
| User-Agent | `claude-cli/2.1.220 (external, sdk-cli)` | `claude-cli/2.1.231 (external, sdk-cli)` | `claude-cli/2.1.239 (external, sdk-cli)` |
| Stainless package | `0.94.0` | `0.112.1` | `0.112.1` |
| `anthropic-version` | `2023-06-01` | тот же | тот же |
| Body top-level keys | 10 одинаковых keys | те же | те же |
| `thinking` | `adaptive`, display `omitted` | то же | то же |
| `output_config` | `effort: low` | то же | то же |
| `context_management` | `clear_thinking_20251015`, keep `all` | то же | то же |

Во всех трёх запросах были:

```text
context_management
max_tokens
messages
metadata
model
output_config
stream
system
thinking
tools
```

Активный beta set выбранного сценария тоже совпал:

```text
claude-code-20250219
interleaved-thinking-2025-05-14
thinking-token-count-2026-05-13
context-management-2025-06-27
prompt-caching-scope-2026-01-05
effort-2025-11-24
```

Это не означает, что все режимы клиента всегда отправляют только эти betas. Официальный bundle содержит дополнительные capabilities, а протокол прямо запрещает фиксировать allowlist.

### 3.2 Изменившийся attribution format

Первый `system` block изменился:

```text
2.1.220  cc_version=2.1.220.a6e
2.1.231  cc_version=2.1.231.408
2.1.239  cc_version=2.1.239.0f1
```

Суффикс больше не соответствует старой форме `.dNN`. Это прямое доказательство CC-02.

### 3.3 Discovery 2.1.239

При `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1` exact 2.1.239 отправил:

```text
HEAD /api/hello
GET /v1/models?limit=1000
POST /v1/messages?beta=true
```

Discovery User-Agent был `claude-code/2.1.239`, credential пришёл только в `x-api-key`, а `anthropic-version` — `2023-06-01`. Это совпадает с официальным protocol reference.

### 3.4 Tool inventory

Обычный запрос с built-in tools на всех трёх версиях имел 24 tool definitions. В выбранном сценарии каждый tool содержал только `name`, `description`, `input_schema`; top-level control keys не изменились. Это доказывает отсутствие delta именно для этого inventory, но не закрывает conditional beta fields `strict`, `defer_loading`, `eager_input_streaming`, tool search и будущие tools.

## 4. Матрица совместимости

| Поверхность | Статус | Доказательство / предел |
|---|---|---|
| `POST /v1/messages?beta=true` | **GREEN** | Native engine добавляет/сохраняет query; router сохраняет path+query; exact clients используют этот path |
| `POST /v1/messages/count_tokens` | **GREEN · STATIC** | Реализован native и в router; exact 2.1.239 one-turn probe не вызвал его |
| Credential headers | **GREEN** | `x-api-key`, `x-goog-api-key`, Bearer имеют OR-semantics без header priority |
| `anthropic-version` | **GREEN** | Клиентское значение имеет приоритет над fallback |
| `anthropic-beta` | **GREEN** | Клиентские tokens сохраняются, добавляются только configured OAuth/Claude-Code identity tokens |
| Текущие body controls 2.1.231/2.1.239 | **GREEN для captured shape** | Shape совпал с 2.1.220; native plane open; Codex skin имеет focused tests known controls |
| Request open-list на native Claude path | **YELLOW** | Arbitrary body fields проходят, но gateway намеренно переписывает model/system/metadata/max_tokens и сериализует JSON заново |
| SSE incremental delivery | **GREEN · UNIT** | Router TTFB test и native SSE tail tests; production/exact-client TTFB не запускался |
| Mid-stream error | **YELLOW** | Валидный Anthropic `event:error` синтезируется вместо transport abort; это совместимо для клиента, но не byte-transparent |
| Unknown SSE events | **GREEN на router native lane** | Router не парсит response body; native plane tee читает usage, но отдаёт исходные chunks |
| `GET /v1/models?limit=1000` | **YELLOW** | Endpoint есть; `display_name` и timeout расходятся с актуальным contract |
| Upstream error forwarding | **GREEN** | Терминальный upstream body/header возвращается; local synthetic errors расходятся с exact shape |
| 32 MiB Messages request | **GREEN на native plane** | Проектный cap совпадает с официальным Messages/Token Counting limit |
| Compressed request body | **INTENTIONAL LIMIT** | Native и universal materializing routes дают 415 для non-identity `Content-Encoding`; exact probes отправили plain JSON |
| Files/Batches/Agents endpoints | **OUT OF CORE CONTRACT** | Native allowlist их не обслуживает; официальный custom-base gateway contract требует только Messages и optional count/models |
| Stable/latest acceptance gate | **RED** | Existing harness использует установленный 2.1.220 без Claude version check |

## 5. Найденные расхождения

### CC-01 — HIGH: latest/stable acceptance не закреплён

**Факт.** `docs/engine/UNIFIED_ROUTER.md:287-312` фиксирует control run Claude Code 2.1.220. `tests/router_harness_live_matrix.sh:79-87` проверяет exact version только Codex, но для `claude` проверяет лишь наличие executable. На машине сейчас установлен 2.1.220.

Claude case в `tests/router_harness_live_matrix.sh:571-600,716-717` использует `OPENAI_MODEL`, то есть проверяет Claude Code → unified Messages → Codex skin. Он не проверяет Claude Code → native Claude subscription plane.

Также Claude case:

- выключает nonessential traffic, а значит выключает gateway discovery;
- не требует факта обращения к `count_tokens`;
- не проверяет `x-claude-code-agent-id` и `x-claude-code-parent-agent-id`;
- проходит через evidence proxy, который полностью читает response и пересобирает `Content-Length`, поэтому не доказывает клиентский SSE TTFB/chunking.

**Эффект.** Изменение клиента может сломать native Claude path или новую capability, а gate останется GREEN на произвольной старой локальной версии. Документированный контрольный run не является воспроизводимым acceptance.

**Рекомендация.** Добавить offline exact-package matrix как минимум для npm `stable` и `latest`: verified package integrity, loopback mock, Standard/structured/tools/discovery/count cases и отдельные native Claude + Codex/Gemini skin paths. Paid production case оставить отдельным controlled acceptance.

### CC-02 — HIGH: fingerprint refresh неверно обрабатывает актуальный `cc_version`

**Факт.** Exact clients показали суффиксы `.408` и `.0f1`. `tools/refresh-fingerprint.sh:100-104` извлекает `cc_version`, но удаляет только regex `\.d[0-9]+$`. Затем `crates/forward/src/proxy.rs:2515-2523` снова добавляет собственный `.dNN`.

Для нового клиента преобразование фактически выглядит так:

```text
2.1.239.0f1  → stored base 2.1.239.0f1 → emitted 2.1.239.0f1.dNN
2.1.231.408  → stored base 2.1.231.408 → emitted 2.1.231.408.dNN
```

Это несуществующая комбинация. Fallback также остаётся на 2.1.195 (`crates/server/src/config.rs:1581-1629`). При этом `docs/ops/INFRASTRUCTURE.md:95-97` говорит, что timer ещё не включён, а `config.env.example:64-67` обещает автоматическую актуальность.

**Эффект.** Механизм, который должен предотвращать drift, сам создаёт fingerprint drift после перехода на новые версии. Пока timer выключен, runtime зависит от вручную актуального `config.env`; его фактическое production значение в этом read-only аудите не проверялось.

**Рекомендация.** Не угадывать build-suffix. Сохранять полную captured `cc_version` как атомарное значение и не синтезировать второй suffix, либо выделить parser только после exact tests всех наблюдаемых форм. Добавить hermetic regression для 2.1.195 `.d49`, 2.1.220 `.a6e`, 2.1.231 `.408`, 2.1.239 `.0f1`.

### CC-03 — MEDIUM: discovery публикует `name`, а Claude Code читает `display_name`

**Официальный contract.** Claude Code читает `data[].id` и опциональный `data[].display_name`.

**Текущая реализация.** `crates/router/src/catalog.rs:177-189` публикует friendly label как `name`. `crates/router/src/tests.rs:3234-3272` закрепляет именно `name`.

**Эффект.** Модели обнаруживаются, потому что `id` корректен, но `/model` не получает human-readable label из gateway и показывает namespaced identifier либо собственный fallback label.

**Рекомендация.** Добавить `display_name` в aggregated Anthropic-format list. Сохранение существующего `name` допустимо как additive metadata для других клиентов.

### CC-04 — MEDIUM: discovery не имеет end-to-end бюджета меньше трёх секунд

**Официальный contract.** `GET /v1/models?limit=1000` имеет клиентский timeout 3 секунды; redirect или timeout превращаются в silent discovery failure.

**Текущая реализация.** Catalog fetch ограничен двумя секундами на plane и идёт конкурентно (`crates/router/src/catalog.rs:31-40,523-567`). Затем `crates/router/src/main.rs:213-228` синхронно требует personalized pricing. `crates/router/src/pricing.rs:120-194` перебирает origins последовательно, по две секунды каждый, и повторяет процесс для chunks по 256 моделей.

**Эффект.** Healthy path обычно быстрый. При slow/dead pricing authority даже один request может превысить клиентские три секунды, после чего discovery молча использует cache/built-ins. Собственный документ уже требует «faster than three seconds» (`docs/engine/UNIFIED_ROUTER.md:298-312`), но код не гарантирует это на failure path.

**Рекомендация.** Ввести единый deadline меньше трёх секунд на всю model-list operation. Pricing origins внутри него выполнять hedged/concurrent либо отделить Claude discovery projection от расширенного personalized catalog, не раскрывая чужие цены и не подставляя zero/last-good pricing.

### CC-05 — MEDIUM: synthetic error contract неполный

**Официальный contract.** Ошибка содержит top-level `error.type`, `error.message`, `request_id`; response header `request-id` содержит тот же идентификатор. HTTP 413 использует `request_too_large`.

**Текущая реализация.** Native engine создаёт body без `request_id` (`crates/forward/src/proxy.rs:987-1006`), а позднее добавляет только header `x-request-id` (`:1134-1156`). Router-local Anthropic errors также не имеют `request_id` (`crates/router/src/error.rs:204-308`). Router Messages 413 имеет `error.type=invalid_request_error`, а не `request_too_large` (`:222-227`).

Upstream errors при этом проходят корректно; расхождение относится к gateway-generated failures.

**Эффект.** Claude Code продолжает распознавать большинство типов, но диагностика «API returned … / request ID» теряет официальный body identity. SDK и support tooling получают форму, отличную от прямого Anthropic API.

**Рекомендация.** Один раз создавать canonical synthetic request ID, класть его в `request-id` и body `request_id`, а 413 возвращать как `request_too_large`. Закрепить exact-shape tests в engine и router.

### CC-06 — MEDIUM: первый attribution block заменяется

**Официальный contract.** Если gateway идёт в `api.anthropic.com`, первый `system` block Claude Code нужно передать без перестановки и изменения. First-party endpoint снимает его позиционно. Перемещение, prepend или merge меняет prompt/cache semantics.

**Текущая реализация.** `inject_identity` не дублирует уже распознанный Claude Code block (`crates/forward/src/proxy.rs:1215-1266`), но per-subscription `set_billing_block` затем заменяет первый billing block (`:1269-1290,2503-2523`). Gateway также может добавить собственную identity/metadata и сериализует body заново.

**Эффект.** Basic inference работает, потому что результирующий block остаётся похож на Claude Code attribution. Но версия/fingerprint/сессия принадлежат синтезированной subscription persona, а не исходному клиенту. Полная unchanged gateway compatibility и client attribution не выполняются. CC-02 дополнительно делает этот block некорректным после refresh.

**Рекомендация.** Явно разделить два режима: transparent API-key gateway сохраняет client attribution; subscription-persona transport документирует intentional rewrite и тестирует exact accepted persona независимо. Не называть второй режим byte-for-byte request proxy.

### CC-07 — MEDIUM · CONDITIONAL: non-Claude skins не являются открытым feature pass-through

**Факт совместимости сейчас.** Captured 2.1.231/2.1.239 shape не изменился относительно 2.1.220. Codex Messages skin принимает текущий no-op `context_management`, `thinking: adaptive`, `output_config.effort|format`, metadata и ephemeral cache form. Focused tests находятся в `crates/forward/src/codex/skin.rs:3326-3457`.

**Расхождение.** Тот же adapter намеренно отклоняет неизвестные расширения `context_management` и `output_config`. Например, unknown `output_config` keys дают local 400. Официальный gateway contract говорит, что `output_config` уже объединяет effort, structured output и task-budget controls, а capability set растёт между релизами.

**Эффект.** Обычный 2.1.239 turn совместим. Режим Claude Code, который активирует новый task budget/context edit/tool beta и направляется в GPT/Gemini skin, может получить local 400, хотя native Anthropic path пропустил бы новую форму upstream.

**Рекомендация.** Для каждого non-Claude adapter иметь exact latest fixtures всех реально испускаемых Claude Code modes. Новую форму либо честно переводить, либо возвращать документированную unsupported-capability ошибку; неизвестные controls нельзя молча игнорировать, если они меняют semantics.

### CC-08 — LOW: документация и example устарели

1. `docs/engine/UNIFIED_ROUTER.md:307-312` говорит, что Claude Code принимает IDs, которые **начинаются** с `claude`/`anthropic`. С 2.1.223 официальный contract использует case-insensitive **contains**. Наши `anthropic/claude-*` совместимы с обеими формами, поэтому runtime blocker нет.
2. `crates/router/src/main.rs:207-209` повторяет старую формулировку.
3. `config.env.example:71-74` говорит о списке UA через запятую и активном `UA_SPREAD`, тогда как `crates/server/src/config.rs:1599-1607` требует разделитель `|`, а `crates/forward/src/upstream.rs:15-30` больше не варьирует patch version.
4. `config.env.example:64-67` обещает автоматическое обновление fingerprint, хотя infra contract говорит, что timer ещё не включён.

**Рекомендация.** Исправить living docs в том же remediation change, который чинит exact acceptance и fingerprint parser.

## 6. Намеренные ограничения, не классифицированные как blockers

### Native request не является byte-for-byte

Native engine:

- снимает namespace `anthropic/`;
- может уменьшить `max_tokens` по балансу;
- добавляет identity/billing/persona metadata;
- сериализует JSON заново;
- заменяет auth/UA/Stainless/session persona headers;
- снимает response content length/encoding и private rate/account headers;
- при mid-stream transport failure добавляет synthetic Anthropic `event:error`.

Это не новый 2.1.239 regression. Это текущая архитектура subscription transport. Она совместима с basic Claude Code behavior, но уже не соответствует буквальной проектной формуле «request byte-for-byte transparent».

### Compressed request body

Materializing native/universal routes отвергают non-identity `Content-Encoding` с 415. Exact 2.1.231/2.1.239 custom-base probes отправили plain JSON даже при локальной попытке включить latent gzip flag, поэтому текущий client blocker не воспроизведён. Если будущий release включит request gzip на custom base, это станет прямой несовместимостью и должно быть отдельным admission project с decompression-bomb guard.

### Files, Batches, managed skills

Официальный client bundle содержит generic SDK Files API и другие managed surfaces. Но gateway protocol для обычного `ANTHROPIC_BASE_URL` требует только Messages, optional count и optional model discovery. Нет доказательства, что local Read/Write/Edit/Bash/MCP/skills обычной сессии вызывают отдельные `/v1/files` или `/v1/skills` на custom base. Поэтому их отсутствие не записано как core blocker.

### Fast mode check

Официальный fast-mode availability check может идти напрямую в `api.anthropic.com`, а не через `ANTHROPIC_BASE_URL`. Проектный custom header выбирает Fast execution только после того, как клиент уже решил отправить Fast request. Сетевое включение `/fast` — client/deployment configuration, а не отдельный endpoint текущего gateway.

## 7. Пробелы доказательств

Не выполнено и не заявляется как доказанное:

1. Production run Claude Code 2.1.231/2.1.239.
2. Платный turn на Claude subscription через native plane.
3. Exact latest Standard/Fast production matrix.
4. Реальный `count_tokens` call от latest CLI в длинной session.
5. Discovery failure-path под реальным трёхсекундным client timeout.
6. 300-second silent-stream/watchdog test.
7. Subagent/nested-agent request с `x-claude-code-agent-id` и parent header через оба proxy hops.
8. Conditional tool betas `strict`, `defer_loading`, `eager_input_streaming` и tool search.
9. 1M context turn.
10. Production fingerprint env и timer state.
11. VS Code/Desktop/GitHub Action surfaces; аудит покрывает CLI/custom base wire.

## 8. Выполненные проверки

Успешно:

- exact `claude --version` для 2.1.231 и 2.1.239 native packages;
- paired loopback basic-turn captures 2.1.220/2.1.231/2.1.239;
- exact 2.1.239 discovery capture;
- exact 2.1.220/2.1.231/2.1.239 built-in tool inventory capture;
- `cargo test -p claude-router native_lane_` — 3 passed;
- `cargo test -p claude-router sse_stream_first_chunk_is_not_buffered` — 1 passed;
- `cargo test -p forward beta_merge_preserves_client_capabilities_and_adds_only_identity` — 1 passed;
- `cargo test -p forward capability_matrix_rejects_unsupported_values` — 1 passed;
- `cargo test -p forward output_config_maps_claude_code_effort_and_json_schema` — 1 passed;
- `python3 tests/router_harness_evidence_proxy_test.py` — 2 passed.

Проверки компилировались с существующими warnings. Они не относятся к измененияению документации и не трактуются как новые findings этого аудита.

Не запускались:

- `tests/router_harness_live_matrix.sh` — требует production credential и выполняет платные turns;
- `tests/router_native_live_matrix.py` — live/paid contract;
- production deployment или SSH;
- full Rust/TypeScript workspace gate, поскольку изменение — только новый audit snapshot и индекс.

## 9. Рекомендуемый план исправлений

### Этап A — сделать acceptance воспроизводимым

1. Добавить exact npm artifact manifest для `stable` и `latest` с integrity/hash.
2. Запускать loopback Claude Code cases на обоих каналах.
3. Разделить native Claude, Codex Messages skin и Gemini Messages skin.
4. Добавить discovery, count, structured output, tools, subagent headers и error recovery cases.
5. Не заменять offline gate платным production smoke; production acceptance остаётся отдельной ступенью.

### Этап B — исправить fingerprint

1. Перестать добавлять guessed `.dNN` к captured full `cc_version`.
2. Добавить version-format regression matrix.
3. Синхронизировать `config.env.example`, server fallback и GLM persona docs.
4. Включать timer только после hermetic test и credential-safe redesign существующего capture script.

### Этап C — закрыть wire contract

1. Добавить `display_name` в discovery.
2. Ввести end-to-end discovery deadline меньше трёх секунд.
3. Унифицировать synthetic `request-id`/`request_id` и исправить 413 type.
4. Разделить transparent client attribution и subscription persona rewrite.
5. Добавлять новые Claude controls в non-Claude skins только после exact emitted-shape fixtures.

### Этап D — controlled live acceptance

После GREEN A–C выполнить отдельно:

1. exact `stable` и `latest` native Claude turn;
2. Standard + Fast;
3. terminal usage и incremental SSE;
4. count_tokens;
5. discovery;
6. tool call/replay;
7. bounded structured output;
8. no-secrets evidence и exact implementation SHA.

## 10. Итог

На 2026-08-21 проект не сломан обновлением Claude Code 2.1.220 → 2.1.231/2.1.239 в обычном Messages/SSE сценарии. Exact local captures показывают совместимый request shape, а focused tests подтверждают ключевые router/native/adapter seams.

Главный риск находится не в одном новом body field, а в **ложной уверенности baseline**: harness остаётся на 2.1.220, версия не pin-ится, native Claude path не покрывается этим client case, а автоматический fingerprint refresh уже не понимает формат новых версий. Discovery и synthetic errors имеют отдельные текущие contract gaps.

Поэтому корректная формулировка состояния:

> **Basic Claude Code 2.1.231/2.1.239 compatibility is locally confirmed. Full current-channel and production compatibility is not yet accepted.**

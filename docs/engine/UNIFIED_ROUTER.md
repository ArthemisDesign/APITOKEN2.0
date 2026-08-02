# UNIFIED_ROUTER — единый endpoint для всех провайдеров (целевая архитектура)

Статус: **этапы 1–5 и фазы 6.1–6.3 реализованы; fallback 6.2 выкатывается
выключенным по умолчанию.**
`router.apitoken.sale` обслуживает весь публичный native-контракт через процесс
`claude-router` (singleton `127.0.0.1:8798`), единый агрегированный каталог
`GET /v1/models{,/{id}}` и universal Chat/Responses/Messages lanes с model-based
dispatch на три плоскости. `/v1/messages/count_tokens` использует тот же dispatch.
До полного OpenRouter-grade routing остаётся фаза 6.4. Документ фиксирует целевую
картину, публичный контракт, инварианты и этапный план; каждый этап при реализации
обновляет этот документ и смежные инструкции в том же коммите.

## Контекст и цель

Продуктовая цель — повторить модель OpenRouter (один аккаунт, один ключ, один баланс,
единый каталог моделей нескольких провайдеров, pay-as-you-go), но **без потери качества
для harness-агентов** (Claude Code, Codex, Gemini-клиенты). OpenRouter гонит все запросы
через один OpenAI-совместимый формат и неизбежно обрезает провайдер-специфику
(thinking signatures, Anthropic beta-поля, encrypted reasoning, stored responses). Наше
отличие: тяжёлые harness'ы получают не перевод, а настоящий API провайдера.

| | OpenRouter | Это решение |
|---|---|---|
| Один ключ / баланс / каталог | да | да |
| Универсальный OpenAI-compatible вход | да | да (universal lane) |
| Нативная точность для Claude Code / Codex | нет, всё переводится | да (native lanes) |
| Неподдерживаемые параметры | молча игнорирует | fail-closed `400 unsupported_parameter` |
| Provider preferences / fallback | да | fallback 6.2 + durable fencing 6.3 готовы default-off; preferences — фаза 6.4 |

Ключевой факт, делающий решение дешёвым: три provider-плоскости уже независимы на уровне
процессов и уже делят один fenced PostgreSQL billing authority — ключи `sk-pool-…`
работают на всех плоскостях (см. `docs/engine/ARCHITECTURE.md`,
`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`).

## Целевая архитектура

```
                    router.apitoken.sale — новая единая точка входа
                    (добавляется РЯДОМ; старые домены не отключаются,
                     см. «Миграционная политика»)
                                    |
                    +---------------+----------------+
                    |           ROUTER               |  stateless replicas
                    |  auth passthrough · каталог    |
                    |  route planner · IR-перевод    |
                    +-------+---------------+--------+
                            |               |
              +-------------+--+   +--------+---------+
              |  NATIVE LANES  |   | UNIVERSAL LANE   |
              | (точность 100%)|   | (охват клиентов) |
              +-------+--------+   +--------+---------+
                      |                     | перевод через
          +-----------+-----------+         | typed canonical IR
          v           v           v         v
     Anthropic    OpenAI      Gemini   выбирает любую плоскость
      plane        plane       plane     по model ID + policy
     8787/8788   8793/8797   8795/8799
          |           |           |
          +-----------+-----+-----+
                            v
              BILLING CORE (единый, уже существует)
     fenced PostgreSQL: ключи · reserve/settle · ledger ·
     versioned pricing catalog · owner_epoch fencing
                            |
                            v
              Пулы подписок за каждой плоскостью
        (OAuth-конверты, cooling, affinity, blue-green)
```

### Native lane

Входной протокол совпадает с backend — запрос идёт без трансляции, байт-в-байт по телу,
SSE и provider-native errors. Router только авторизует (passthrough ключа), разрешает
model ID и передаёт запрос в stable origin соответствующей плоскости. Это основной вход
для harness-агентов и гарантия качества уровня протокола.

### Universal lane

Клиент, умеющий только OpenAI Chat Completions (Aider, Continue, Roo/Kilo, Hermes,
большинство IDE-плагинов), шлёт в `/v1/chat/completions` любую модель из каталога.
Router переводит через типизированный canonical Turn/Event IR в нативный протокол
выбранной плоскости. IR обязан покрывать: system/developer messages, content blocks и
изображения, tool calls и tool results, structured output, thinking/reasoning,
prompt-cache boundaries, usage и canonical streaming events.

Контракт перевода — **строгий, fail-closed**:

- неподдерживаемая target-плоскостью capability → понятный `400 unsupported_parameter`,
  никакого молчаливого выбрасывания `strict`, `thinking`, server tools, response schema;
- opaque reasoning artifacts (Claude thinking signatures, OpenAI encrypted reasoning,
  Gemini thought signatures) имеют provider provenance: возвращаются только тому же
  провайдеру либо отклоняются; молчаливое удаление запрещено (ломает agent loop или
  раскрывает внутреннее reasoning).

## Публичный контракт

Один hostname; входной endpoint определяет wire-протокол и (на этапах 1a–1b) плоскость:

```
POST /v1/messages                                   Anthropic Messages (Claude Code)
                                                    (этап 5.1 — + любая openai/* модель
                                                    каталога через model-based dispatch
                                                    в Anthropic Skin на Codex plane;
                                                    этап 5.2 — + любая google/* модель
                                                    каталога на Gemini plane)
POST /v1/messages/count_tokens                    Anthropic token counting с model-based
                                                  dispatch: native Anthropic, локальный
                                                  подсчёт Codex или native Gemini
                                                  `:countTokens` (этапы 5.1–5.2)

POST /v1/responses                                OpenAI Responses (этап 1a — только OpenAI
                                                  plane, этап 4.1 — + любая Claude-модель
                                                  каталога через model-based dispatch)
POST /v1/responses/input_tokens                   OpenAI token counting (пока openai-only)
GET  /v1/responses/{id}                           stored response — только openai/*
GET  /v1/responses/{id}/input_items               (решение 5)

POST /v1/chat/completions                         universal OpenAI-compatible вход
                                                  (этап 1a — только OpenAI plane,
                                                   этап 3.1 — + любая Claude-модель
                                                   каталога через model-based dispatch,
                                                   этап 3.3 — + Gemini-модели)

GET  /v1/models                                   единый агрегированный каталог (этап 1b)
GET  /v1/models/{id}

GET  /v1beta/models                               Gemini native
POST /v1beta/models/{id}:generateContent
POST /v1beta/models/{id}:streamGenerateContent    (alt=sse и alt=json)
POST /v1beta/models/{id}:countTokens
```

Четыре universal POST-пути (`chat/completions`, `responses`, `messages` и
`messages/count_tokens`) принимают необязательное `models: [<id>, …]` как продолжение
цепочки после обязательного `model`. Поле активно только при
`CLAUDE_ROUTER_FALLBACK_ENABLED=1`; default-off router возвращает lane-shaped `400` до
обращения к каталогу/плоскости. Подробный preflight и retry matrix —
`docs/engine/ROUTING_FENCING.md` §§3.3, 5.

Префикс `/api/v1` (OpenRouter-совместимые пути) в MVP **не** добавляется: Cline, Codex и
большинство custom-provider конфигураций принимают свой Base URL. Он понадобится, только
если появятся клиенты, жёстко привязанные к OpenRouter path.

## Совместимость с harness-агентами

| Harness | Нужный контракт | Вход |
|---|---|---|
| Claude Code | Anthropic Messages, SSE, открытые beta/header/body lists | native Anthropic lane; Anthropic Skin (этап 5) для non-Claude моделей |
| Codex | Responses API; custom provider поддерживает только `wire_api="responses"` | native OpenAI lane; chat-only прокси недостаточен |
| OpenCode | OpenRouter preset либо custom AI SDK / OpenAI-compatible provider | universal lane, namespaced model IDs |
| Cline | OpenRouter с custom Base URL, OpenAI Compatible или Anthropic с custom Base URL | universal lane либо native Messages для Claude |
| Hermes | OpenRouter / custom providers; routing, fallback, auxiliary models; контекст ≥ 64K (меньшие окна отклоняются на старте) | universal lane; preset на router, каталог для preset — только модели ≥ 64K |
| Aider, Continue, Roo/Kilo, большинство IDE agents | OpenAI Chat Completions | universal lane |
| Native SDKs | Messages / Responses / Gemini Developer API | соответствующий native lane |

Критические требования Claude Code (контракт native Anthropic lane):

- не буферизовать SSE — буферизация полного ответа останавливает клиент;
- сохранять `/v1/messages?beta=true` и передавать `anthropic-beta` / `anthropic-version`
  в плоскость verbatim;
- headers и body fields — открытые списки: неизвестные поля проксируются, а не
  отклоняются и не выбрасываются;
- native Anthropic errors не оборачивать: Claude Code иногда восстанавливается по тексту
  ошибки;
- `GET /v1/models?limit=1000` — без redirect и быстрее трёх секунд;
- Claude Code игнорирует discovery-ID, не начинающиеся с `claude` или `anthropic`
  (поэтому `anthropic/claude-*` совместим); `/v1/messages/count_tokens` опционален —
  без него клиент считает контекст локально; model discovery выключен по умолчанию и
  требует `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1` (Claude Code v2.1.129+).

Codex: требуется полноценный Responses API, а не адаптация Chat Completions — custom
provider поддерживает только `wire_api="responses"` (это дефолт при omitted).

Namespaced ID из агрегированного каталога — исполнимый контракт, а не только discovery metadata:
router сохраняет universal request body, поэтому каждая плоскость снимает свой префикс до
admission (`anthropic/`, `openai/`, `google/`). Для GPT Fast на Responses и Chat используются
`service_tier: "fast"|"priority"`; Anthropic Messages harness может отправить нативный
`speed: "fast"`, а `service_tier: "fast"|"priority"` принимается как совместимый alias. Все
варианты нормализуются в effective `priority`, который определяет reserve, settlement и публичный
`usage.service_tier`. `GET /v1/models` по Codex `originator`/User-Agent после обычной проверки ключа
возвращает backend-native overlay `{models: []}`: Codex объединяет его со встроенным каталогом;
обычные OpenAI/OpenRouter SDK по-прежнему получают агрегированный `{object:"list",data:[…]}`.

## Инварианты

1. **Биллинг только в provider-плоскости.** Router не резервирует и не списывает деньги.
   Ключ клиента передаётся в плоскость verbatim; `request_id`, reserve → delivering →
   settle и exactly-once ledger остаются ответственностью плоскости
   (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`). Двойное списание исключено конструктивно.
2. **Router не ретраит неоднозначный исход, который мог дойти до плоскости.** Повтор по timeout после
   отправки запроса создал бы новый `request_id` и второе списание: backend мог выполнить
   запрос и settle, даже если router ответа не получил. Следующая модель явной fallback-
   цепочки допустима только после доказанного TCP ConnectionRefused либо точного не-2xx
   `x-apitoken-execution-state: not_started`, которым плоскость гарантирует отсутствие
   charge. 401/402 и клиентские 4xx не ретраятся; signed 429 — capacity-исключение.
   Подробности — «Семантика fallback».
3. **Никаких общих очередей, semaphore и circuit breaker в router.** Concurrency limits,
   breaker и cooling живут в плоскостях (процессная изоляция, см.
   `docs/engine/ARCHITECTURE.md`). Router не добавляет глобальный лимит — иначе
   перегруженная плоскость съест capacity остальных. Readiness router никогда не является
   конъюнкцией health всех плоскостей; синхронных health-check'ов на пути запроса нет.
4. **SSE не буферизуется** ни в router, ни в Caddy перед ним (требование Claude Code
   gateway protocol). Disconnect клиента транзитивно рвёт соединение router→плоскость,
   чтобы существующий TeeMeter drain дочитывал authoritative usage и settle корректно.
5. **Деньги — только integer** (bigint / nanoUSD-строки) во всех новых поверхностях.
6. **Старые per-provider домены** (`api.`, `openai.api.`, `gemini.api.apitoken.sale`)
   остаются полноценными production-входами на весь период миграции — не «аварийными
   запасными», а действующими endpoint'ами активных клиентов. Их контракт, поведение и
   SLA не меняются; см. «Миграционная политика».
7. **Router — отдельный bounded context** (`crates/router`), общается с плоскостями
   только по HTTP, не импортирует `pool`/`forward`. Control API — loopback-only
   управление аккаунтами/прайсингом; в data-plane router'а он не участвует.

## Миграционная политика (мягкий переезд)

У продукта есть активные клиенты на существующих per-provider endpoint'ах, поэтому
переезд — только мягкий, без единой даты «выключения старого»:

- **Ничего не отключаем.** `api.apitoken.sale`, `openai.api.apitoken.sale` и
  `gemini.api.apitoken.sale` продолжают обслуживать трафик бессрочно — минимум до
  отдельно объявленной deprecation-программы с измеримой долей остаточного трафика
  и персональной коммуникацией с затронутыми клиентами. Sunset-дата в этом документе
  отсутствует намеренно.
- **Unified endpoint — новый, отдельный hostname.** Существующие домены не
  переиспользуются и не меняют поведение: `api.apitoken.sale` остаётся прямым
  Anthropic-входом. Новые клиенты и новые интеграции получают unified-домен; старые
  клиенты переезжают добровольно, когда им это удобно.
- **Одинаковый backend.** Оба входа ведут в одни и те же provider-плоскости и один
  billing authority, поэтому ключ, баланс и ledger клиента идентичны на любом входе —
  переезд клиента это смена base URL, а не миграция аккаунта.
- **Новые возможности — сначала unified.** Universal lane, единый каталог и routing
  policy развиваются на unified endpoint; старые домены сохраняют текущий контракт
  (критические исправления, разумеется, общие — плоскости одни).
- **Наблюдаемость раздельная.** Метрики трафика по hostname, чтобы решение о любой
  будущей deprecation-программе опиралось на фактическую долю трафика старых доменов,
  а не на оценки.

## Модели и каталог

Единый каталог публикует namespaced ID: `anthropic/claude-*`, `openai/gpt-*`,
`google/gemini-*`. Namespace означает семейство модели, не обязательно единственного
исполнителя: при появлении альтернативных backends одной модели (Anthropic direct,
Bedrock, Vertex) route planner сможет выбирать между ними. Текущие нативные ID остаются
однозначными aliases. Источник правды для каталога — существующий versioned
multi-provider pricing catalog (`docs/engine/CONTROL_API.md`,
`crates/registry/src/pricing.rs`).

`/v1/models` — единственная коллизия путей native-плоскостей: unified endpoint обязан
агрегировать каталоги всех плоскостей (кэш, частичный каталог при падении одной
плоскости, без блокировки остальных). Именно агрегация каталога — первый код,
оправдывающий `crates/router`.

## Семантика fallback и billing fencing

Наивное правило «fallback только до первого байта» недостаточно: при timeout backend мог
выполнить запрос и settle, даже если router ответа не получил. Отсюда градация:

- **Этапы 1a–5: межмодельного fallback нет вообще.** Единственный retry — существующий
  внутри плоскости до первого публичного байта (no-byte retry boundary), он безопасен,
  потому что не создаёт новый billable request после начала доставки.
- **Фаза 6.1:** плоскости выставляют внутренний
  `x-apitoken-execution-state: not_started` только до started при гарантии refund/cancel;
  router снимает его со всех публичных ответов.
- **Фаза 6.2, MVP fallback:** default-off поле `models` задаёт serial continuation после
  обязательного `model`. Router preflight-валидирует всю цепочку по одному catalog snapshot,
  а повторяет только по точному сигналу 6.1 либо доказанному TCP ConnectionRefused. Timeout,
  unsigned 5xx, обрыв после headers и клиентские 4xx fail closed.
- **Фаза 6.3, durable fencing:** общий execution group / attempt ID, идемпотентные reservations и
  атомарный выбор единственного billable winner — reservation identity расширяется с
  `request_id` на `(group_id, attempt_id)`, а settled-запись допускает ровно один winner
  на группу (расширение текущего `UNIQUE ledger(kind, request_id)`). Реализовано migration-first:
  Caddy снимает клиентские capability headers, router инжектирует одну CSPRNG UUIDv4 на explicit
  fallback chain, плоскости валидируют и durable сохраняют пару, registry loser-settlement
  принудительно делает zero-charge/full refund. Любой loser увеличивает always-zero incident metric.
- **Ambiguous disconnect → никакого автоматического повтора на другой модели.** Клиент
  получает честную ошибку и решает сам; молчаливый retry на timeout — путь к двойному
  списанию.

## Решения universal lanes (зафиксированы 2026-08-01, перед этапом 3)

Обсуждены с владельцем продукта и утверждены; реализация этапов 3–6 следует им, а отклонение
требует пересмотра этого раздела.

1. **Перевод живёт в плоскостях, не в router.** Universal-входы реализуются адаптерами внутри
   каждой плоскости (этап 3: chat→Messages в Anthropic plane, chat→generateContent в Gemini
   plane; этап 4: Responses→native; этап 5: Messages→native). Router получает ровно одну новую
   способность — model-based routing: распарсить тело запроса, извлечь `model`, выбрать
   плоскость по namespace (`anthropic/`→8790, `openai/`→8792, `google/`→8794) или alias из
   собственного кэшированного каталога; тело дальше проксируется без изменений, namespaced ID
   резолвит admission плоскости. Перевод в router отвергнут: он дублирует provider-логику вне
   `forward`, отрывает биллинг (reserve/settle) от плоскости и раздувает router до второго
   движка.
2. **Без единого IR-типа.** «Canonical IR» из этапного плана означает контракт событий —
   типизированный словарь (text delta, tool_call delta, reasoning delta, usage, finish) —
   который каждый per-plane адаптер обязан воспроизводить и который закреплён contract-тестами
   плоскости. Общий IR-структ, в который переводятся все провайдеры, отвергнут: это путь к
   наименьшему общему знаменателю и молчаливой потере специфики (сценарий OpenRouter).
3. **Capability matrix + fail-closed с поправкой на defaults.** У каждой плоскости — явная
   матрица параметров universal-входа: honored / unsupported. Unsupported-параметр с
   не-дефолтным значением → `400 unsupported_parameter`; с дефолтным значением — принимается
   (stock SDK шлют дефолты пачками, совместимость сохраняется). Неизвестные поля проксируются
   (открытый список). Это легализует leniency существующего `crates/forward/src/codex/chat.rs`
   как «lenient для defaults» и делает её политикой всех адаптеров.
4. **Reasoning.** `reasoning_effort` мапится на native thinking-конфиг провайдера; поток
   reasoning отдаётся дельтами в задокументированном расширении `reasoning_content`
   (конвенция DeepSeek/OpenRouter). Подписи/encrypted reasoning в universal lanes **не
   выставляются** — задокументированное ограничение: harness-клиенты используют native lanes.
5. **Stored responses (этап 4) — только для `openai/*`.** `store:true` и
   `GET/DELETE /v1/responses/{id}` работают только для OpenAI-моделей; для остальных →
   `400 documented_limitation`. Кросс-провайдерное хранилище ответов не строится.
6. **Этап 5 зеркалит 3–4.** `/v1/messages` для `openai/*` и `google/*` — адаптеры
   Messages→native в соответствующих плоскостях с той же capability matrix; thinking-дельты
   без подписей; реплей thinking-блоков для non-Claude моделей не поддерживается.
7. **Этап 6: fencing и fallback реализуются фазами.** Фундамент уже есть в
   `crates/registry/src/pricing.rs` (versioned catalog, provider switches, account policy).
   Фазы 6.1–6.3 реализовали внутренний `not_started`, default-off serial fallback по
   `models` и durable execution group/единственный billable winner;
   policy/presets/GA telemetry остаются фазой 6.4. Детальный контракт —
   `docs/engine/ROUTING_FENCING.md`.

## Существующая база (что переиспользуем)

Проверено аудитом кода 2026-08-01; всё перечисленное реально существует:

- Chat Completions поверх Responses (`crates/forward/src/codex/chat.rs`) — provider-
  specific; использовать как reference и источник contract tests, не объявляя
  универсальным IR без переработки;
- типизированный диспатч `response.*` streaming-событий
  (`crates/forward/src/codex/transport.rs`);
- retry только до первого публичного SSE-события — на всех трёх плоскостях;
- disconnect drain до authoritative usage и settlement (`crates/forward/src/meter.rs`);
- per-model Gemini cooling (`crates/forward/src/gemini/pool.rs`);
- единый `AffinityStore` с provider-проекциями для Anthropic/OpenAI/Gemini
  (`crates/forward/src/affinity.rs`);
- fenced reserve/settle, `owner_epoch` fencing и exactly-once ledger
  (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`);
- versioned multi-provider pricing catalog и provider switches
  (`docs/engine/CONTROL_API.md`);
- нативные пути Gemini уже обслуживаются плоскостью (`docs/engine/GEMINI_PROVIDER.md`).

Две уточняющие оговорки аудита:

- circuit breaker кодово глобален внутри процесса (`crates/forward/src/breaker.rs`);
  per-provider изоляция достигается процессной моделью (один процесс = одна плоскость),
  а не раздельными breaker-объектами. Для router-архитектуры это достаточно, но
  формулировка «свой breaker у провайдера» означает деплоймент, а не код;
- `ProviderMode::Combined` (`crates/forward/src/state.rs`) — legacy rollout bridge для
  установок со старыми systemd-юнитами, а не «combined pool» и не целевая модель;
  Gemini он не обслуживает. Router его не использует.

## Этапный план

1. **1a. Caddy fan-in — РЕАЛИЗОВАН.** `router.apitoken.sale` маршрутизирует по форме пути
   на существующие loopback origins: `/v1/messages*` и `/balance` → 8790, `/v1/responses*` +
   `/v1/chat/completions` → 8792, `/v1beta/*` → 8794; `/health` отвечает сам Caddy (не
   конъюнкция health плоскостей), остальные пути — 404. Без нового кода; изоляция,
   биллинг и auth passthrough — «из коробки». Провайдер определяется путём, не ключом
   и не моделью. `/v1/models` намеренно не обслуживается до этапа 1b.
2. **1b. `crates/router` + единый каталог — РЕАЛИЗОВАН (код и конвейер), cutover отдельным
   шагом.** Stateless router (`crates/router`, бинарь `claude-router`, loopback `127.0.0.1:8798`,
   singleton `claude-router.service`): байт-в-байт proxy трёх плоскостей без native retries и без
   общего таймаута (стримы не обрезаются), hop-by-hop заголовки снимаются, ошибки шейпятся
   под lane пути. Единый `/v1/models` агрегирует каталоги плоскостей конкурентно: namespaced
   ID (`anthropic/…`, `openai/…`, `google/…`) + aliases, TTL-кэш 30 с + last-good без TTL,
   упавшая плоскость опускается с маркировкой `x-apitoken-catalog-degraded`, пустой каталог
   плоскости считается сбоем, 401/403 плоскости → единый 401, все плоскости без кэша → 503.
   Auth passthrough без изменений; `/health`, `/live`, `/ready` — router-local. Деплой: третий
   tested artifact в цепочке watchdog → promote → stage, `restart_router_if_changed` сравнивает
   запущенный бинарь и требует `/ready` до зелёного релиза; юнит ставится watchdog-infrastructure
   шагом. На этапе 1b — без cross-provider translation и fallback; последующие фазы
   расширяют тот же bounded context. Cutover Caddy выполнен: vhost
   `router.apitoken.sale` терминирует TLS и проксирует весь публичный контракт (включая
   `/v1/models*`) в router на `127.0.0.1:8798`; раздельные шаги (процесс, затем переворот)
   исключили окно 502 между установкой Caddyfile и запуском router'а.
3. **Universal Chat (2–4 недели).** `/v1/chat/completions` для всех моделей каталога:
   text, images, tools, structured output, streaming. Реализуется по решениям 1–4 раздела
   «Решения universal lanes»: адаптеры в плоскостях, router — только model-based routing,
   контракт событий вместо IR-структа, capability matrix. Подпакеты: **3.0** — фиксация
   решений в этом документе (РЕАЛИЗОВАН); **3.1** — router model-routing + адаптер
   Anthropic plane (text, streaming, usage) — **РЕАЛИЗОВАН**: `POST /v1/chat/completions`
   в router (`crates/router/src/chat.rs`) буферизует только тело запроса (32 MiB — потолок
   наибольшей плоскости), извлекает `model` и выбирает плоскость по namespace-префиксу без
   опроса каталога либо по alias через кэшированный каталог; тело проксируется без
   изменений, ошибки dispatch (400 невалидный JSON/нет `model`, 404 `model_not_found`,
   503 `catalog_unavailable`, единый 401) — в OpenAI-конверте. Адаптер Anthropic plane
   (`crates/forward/src/anthropic.rs`, роут в `ProviderMode::Anthropic`) переводит
   chat→Messages (system/developer → top-level `system`, склейка подряд идущих одноролевых
   сообщений, `max_completion_tokens`→`max_tokens` с дефолтом 4096, `stop`→`stop_sequences`,
   `user`→`metadata.user_id`, strip `anthropic/`-префикса до admission) и вызывает общий
   `forward()` — auth, reserve, ротация, identity-инжект, tee-метеринг и settle без
   изменений. Ответ переводится снаружи: Messages SSE → `chat.completion.chunk` (role/text/
   finish-чанки, ping→heartbeat, `event: error`→OpenAI error frame без `[DONE]`,
   usage-чанк по `stream_options.include_usage`), JSON message → `chat.completion`
   (usage включает cache-токены с `prompt_tokens_details.cached_tokens`). Capability
   matrix: structured/reasoning/penalties/n>1/store и прочие не-дефолтные
   unsupported-параметры → `400 unsupported_parameter` до этапа 3.4; дефолтные
   значения принимаются, неизвестные поля проксируются. Все ошибки этого пути (включая
   `local_err` плоскости и пасsthrough апстрима) конвертируются в OpenAI-конверт с
   сохранением статуса (402 LowBalance тоже) и `Retry-After`; **3.2** — tools/tool_choice
   + contract-тесты словаря событий — **РЕАЛИЗОВАН**: chat `tools[]` и legacy
   `functions[]` → Messages `tools[]` (`parameters`→`input_schema`, отсутствующая схема
   → `{"type":"object"}`); `tool_choice` (auto/required/none/именная функция) и legacy
   `function_call` → Messages `tool_choice` (auto/any/none/tool);
   `parallel_tool_calls:false` → `disable_parallel_tool_use:true`; дефолты (пустой
   `tools`, `auto`) в тело не вставляются. В истории assistant `tool_calls[]`/
   `function_call` → `tool_use`-блоки (`arguments` JSON-строка парсится в `input`;
   legacy id — детерминированный `callu_<name>`), role `tool`/`function` →
   user-сообщение с `tool_result`-блоками, серии tool-ответов склеиваются в одно
   user-сообщение (семантика параллельных tool calls Messages). В ответе non-stream
   `tool_use`-блоки → `message.tool_calls` (`input` сериализуется обратно в
   `arguments`-строку, `content:null` при отсутствии текста), SSE
   `content_block_start(tool_use)` → tool_calls-чанк с id/name, `input_json_delta` →
   arguments-дельты; tool ordinal нумеруется отдельно от Messages block index.
   Contract-тесты словаря событий (решение 2): табличные «каноническая
   последовательность Messages-событий → чанки» для text, одиночного и параллельных
   tool calls, text+tool и usage — в тестах `crates/forward/src/anthropic.rs`; e2e —
   `tests/universal_chat_smoke.sh` (мок отдаёт tool_use диалог, проверки tools
   non-stream/stream/history и сквозной цепочки router→engine→mock); **3.3** —
   адаптер Gemini plane — **РЕАЛИЗОВАН**: `crates/forward/src/gemini/chat.rs`,
   роут в `ProviderMode::Gemini`. Chat→GenerateContentRequest: system/developer →
   `systemInstruction`, user/assistant → `contents` с Gemini-ролями user/model и
   склейкой подряд идущих одноролевых, `max_completion_tokens`/`max_tokens` →
   `maxOutputTokens` (дефолт 4096), `stop` → `stopSequences` (≤5),
   temperature/top_p/top_k → `generationConfig`, strip `google/`-префикса до
   admission. Адаптер синтезирует внутренний запрос на
   `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` и вызывает
   общий `gemini_api()` — admission, reserve, affinity, ротация, Code Assist
   wrapper, tee-метеринг и settle без изменений. Tools: chat `tools[]`/legacy
   `functions[]` → `tools:[{functionDeclarations}]` (parameters проксируются,
   отсутствующие опускаются); `tool_choice`/legacy `function_call` →
   `toolConfig.functionCallingConfig` (auto не вставляется, required→ANY, none→NONE,
   именная → ANY+allowedFunctionNames). История: assistant `tool_calls[]`/
   `function_call` → functionCall-парты, role `tool`/`function` →
   functionResponse-парты в user-content (имя восстанавливается по tool_call_id из
   карты id→name этой же истории, неизвестный id → 400; не-JSON tool output
   заворачивается строкой в `{result}`), серии tool-ответов склеиваются. Ответ:
   non-stream candidates[0] — text-парты склеиваются, functionCall →
   `message.tool_calls` (args → arguments-строка, синтезируемые id
   `callu_<name>[_N]`, content:null без текста), finishReason → finish_reason
   (MAX_TOKENS→length, SAFETY/RECITATION/BLOCKLIST/PROHIBITED_CONTENT/SPII→
   content_filter), promptFeedback.blockReason без кандидатов → content_filter с
   пустым content, usageMetadata → usage (completion = candidates+thoughts, cached →
   `prompt_tokens_details.cached_tokens`), model = `modelVersion` либо запрошенная.
   SSE: data-only кадры GenerateContentResponse → role-чанк, content-дельты,
   functionCall целиком одним tool_calls-чанком (arguments-дельт на wire нет),
   finishReason → finish-чанк, последний usageMetadata → usage-чанк на EOF (по
   `stream_options.include_usage`) → `[DONE]`; санитизированный mid-stream
   `{error}` → OpenAI error frame без `[DONE]`; неразборчивые кадры пропускаются.
   Capability matrix — те же 17 правил Anthropic-плоскости плюс
   `parallel_tool_calls` и `user` (19 всего), и отличие плоскости: закрытый список
   top-level полей (неизвестное поле → `400 unsupported_parameter`, потому что
   Code Assist wrapper иначе выбросил бы его молча). Ошибки: Google-конверт
   `{error:{code,message,status}}` → OpenAI-конверт с сохранением статуса (402
   LowBalance тоже) и `Retry-After`; особый маппинг нативного
   `400 API_KEY_INVALID` → `401 authentication_error`. Прод-проверенное
   upstream-ограничение (2026-08-01): replayed tool-история (functionCall в
   model-turn + functionResponse в user-turn) отклоняется Code Assist с
   `400 INVALID_ARGUMENT` при любом thinking-уровне — thinking-модели Gemini
   требуют `thoughtSignature` на functionCall-парте при replay, а подписи в
   universal lanes не выставляются и на реплее не восстанавливаются
   (решение 4); прямой tool calling (модель отвечает functionCall) работает,
   ограничение общее с Responses-адаптером 4.3 (см. п. 4). Contract-тесты — табличные
   в `crates/forward/src/gemini/chat.rs` (запрос, matrix, ответ, SSE); e2e-харнесс
   для Gemini-ноги не добавлялся: native-путь плоскости покрыт своими тестами, а
   мок-харнесс не умеет AEAD-конверты профилей — шов адаптера покрыт
   unit/contract-тестами; **3.4a** — images + structured output —
   **РЕАЛИЗОВАН** (обе плоскости): image_url-части user-сообщений — Anthropic:
   data: URL → base64 source, http(s) → url source (оба нативные Messages
   image-блока); Gemini: только data: URL → inlineData (http(s) ссылки
   generateContent не принимает — честный 400, fileData требует File API
   upload); `detail` != auto отклоняется `400 unsupported_parameter` на обеих.
   `response_format` json_schema → Anthropic GA `output_config.format`
   (обёрточные name/strict/description не проксируются — только схема;
   json_object у Messages нет → matrix 400), на Gemini json_object →
   `generationConfig.responseMimeType: application/json`, json_schema →
   +`responseSchema` (обёртка аналогично снимается); **3.4b** —
   `reasoning_effort` → native thinking-конфиг + `reasoning_content` дельты
   (решение 4) — **РЕАЛИЗОВАН** (обе плоскости): вход `reasoning_effort`
   minimal|low|medium|high (null/отсутствие — выкл; любое другое не-null
   значение → `400 invalid_request` с `param: reasoning_effort`) мапится на
   Anthropic GA `output_config.effort` (minimal клампится в low,
   beta-заголовок не нужен; `effort` соседствует с `format` из 3.4a в одном
   `output_config`, не затирая его) и на Gemini
   `generationConfig.thinkingConfig` (`thinkingLevel` проксируется как есть —
   плоскость сама мапит уровень в wire model id; `includeThoughts: true`).
   **3.4c** (фикс по живым пробам native lane): на Anthropic одного `effort`
   мало — на моделях 4.6+ adaptive thinking по умолчанию выключен, а
   дефолтный `display: "omitted"` присылает thinking-блоки с пустым текстом,
   поэтому при не-null `reasoning_effort` адаптер дополнительно инжектит
   `thinking: {"type": "adaptive", "display": "summarized"}` (явный
   `thinking` клиента не переопределяется — open list; на моделях ≤4.5
   adaptive не поддержан — upstream честно отвечает 400).
   Ответ — конвенция `reasoning_content`: Anthropic thinking-блоки и Gemini
   thought-парты склеиваются в `message.reasoning_content` (non-stream, поле
   присутствует только при непустом reasoning), thinking_delta/thought-парты
   стрима → чанки `{"delta":{"reasoning_content": ...}}` в естественном
   порядке апстрима (reasoning перед content, role-чанк первый). Подписи не
   выставляются (решение 4): signature_delta/thoughtSignature выбрасываются,
   redacted_thinking игнорируется. Правило `reasoning_effort` удалено из
   capability matrix обеих плоскостей (Anthropic 17→16, Gemini 19→18).
4. **Universal Responses для Codex-parity (2–4 недели).** `POST /v1/responses` для всех
   моделей каталога: text, images, tools, reasoning, usage, streaming. Реализуется по решениям
   1–5 раздела «Решения universal lanes»: адаптеры в плоскостях, router — только model-based
   routing, stored responses — только `openai/*` (для остальных явный
   `400 documented_limitation`). Подпакеты: **4.1** — router dispatch + адаптер Anthropic
   plane (ядро: текст, usage, stream; tools в запросе и function_call в ответе) —
   **РЕАЛИЗОВАН**: `POST /v1/responses` в router (`crates/router/src/responses.rs`) повторяет
   chat-диспатч этапа 3.1 — буферизуется только тело запроса (32 MiB), извлекается `model`,
   плоскость выбирается по namespace-префиксу без опроса каталога либо по alias через
   кэшированный каталог, тело проксируется без изменений, ошибки dispatch (400 невалидный
   JSON/нет `model`, 404 `model_not_found`, 503 `catalog_unavailable`, единый 401) — в
   OpenAI-конверте. Stored endpoints (`POST /v1/responses/input_tokens`,
   `GET/DELETE /v1/responses/{id}`, `GET /v1/responses/{id}/input_items`) dispatch НЕ
   используют и остаются native OpenAI lane (решение 5; token counting пока тоже openai-only
   — задокументированное ограничение). Адаптер Anthropic plane
   (`crates/forward/src/anthropic_responses.rs`, роут в `ProviderMode::Anthropic`) переводит
   Responses→Messages и вызывает общий `forward()` (auth, reserve, ротация, identity-инжект,
   tee-метеринг, settle — без изменений). Запрос: `instructions` и system/developer items →
   top-level `system` (instructions первым), `input` строка → user-сообщение, массив items
   (message item — `{type:"message",…}` или компактная `{role, content}` без type) →
   сообщения со склейкой одноролевых, parts `input_text`/`output_text` → text-блоки,
   `input_image` → image-блоки (общий с chat-адаптером перевод: data: → base64, http(s) →
   url source, `detail` != auto → 400), `tools` → `tools[]` (`parameters`→`input_schema`,
   `strict` снимается; не-function tool → `400 unsupported_parameter`),
   `tool_choice`/`parallel_tool_calls` → Messages `tool_choice`, `max_output_tokens` →
   `max_tokens` (дефолт 4096), `reasoning.effort` → `output_config.effort` (minimal
   клампится в low) + инжект `thinking: {type:"adaptive", display:"summarized"}` (как 3.4c;
   явный `thinking` клиента не переопределяется), `text.format` json_schema →
   `output_config.format` (обёртка снимается; json_object → 400), capability matrix
   (`background`, `service_tier`, `truncation`, `include`, `prompt_cache_key`,
   `safety_identifier`, `user`, `metadata`, `max_tool_calls`, не-дефолтная `text.verbosity`)
   с не-дефолтом → `400 unsupported_parameter`, неизвестные поля проксируются (open list).
   Ответ (словарь 4.1): non-stream → Response object (`resp_*`; text-блоки склеиваются в
   один message item с одним output_text part, tool_use → function_call items `fc_*` с
   `call_id` = tool_use id и arguments-строкой; usage: input = input+cache_creation+
   cache_read с `input_tokens_details.cached_tokens`, reasoning_tokens из thinking_tokens;
   status completed/incomplete по stop_reason: max_tokens/context_window →
   `max_output_tokens`, refusal → `content_filter`); stream Messages SSE → Responses SSE
   (`response.created` → `response.in_progress` → per-block `output_item.added` /
   `content_part.added` / `output_text.delta|done` / `function_call_arguments.delta|done` /
   `output_item.done` → `response.completed` с полным объектом и usage; ping → `: ping`
   comment-кадр; mid-stream `event: error` и преждевременный EOF → `response.failed`;
   output_index — плотный собственный счётчик, thinking-блоки позицию не занимают).
   Ошибки — общий с chat-адаптером OpenAI-конверт с сохранением статуса (402 LowBalance
   тоже) и `Retry-After`. Временные ограничения 4.1: `function_call`/
   `function_call_output` items во входе → `400 unsupported_parameter` (replay истории
   tool calls — 4.2), `reasoning` items во входе принимаются и выбрасываются (подписи не
   выставляются — решение 4), thinking-блоки ответа пропускаются без reasoning-событий,
   `store:true`/`previous_response_id`/`item_reference` → `400 documented_limitation`;
   **4.2** — replay tool-истории во входе + reasoning summary события — **РЕАЛИЗОВАН**:
   входные `function_call` items → assistant `tool_use`-блоки Messages (`call_id` → `id`,
   `arguments` JSON-строка парсится в `input` — невалидный JSON/не-object →
   `400 invalid_request`, отсутствующая/пустая строка — `{}`; отсутствующие/пустые
   `call_id`/`name` → 400), входные `function_call_output` items → user
   `tool_result`-блоки (`call_id` → `tool_use_id`; `output` строка → text content как
   есть, массив text-партов склеивается через \n, нетекстовые части → 400); склейка с
   соседними message items — общая одноролевая, pairing tool_use/tool_result адаптером
   не валидируется (апстрим Messages честно отвечает 400, как в chat-адаптере 3.2).
   Thinking-блоки ответа переводятся в reasoning-словарь Responses: non-stream —
   reasoning item `{"type":"reasoning","id":"rs_*","summary":[{"type":"summary_text",
   "text":<текст блока>}]}` в output в порядке появления блоков (каждый thinking-блок —
   отдельный item; пустой thinking item не порождает; message item — на позиции первого
   text-блока); stream — `response.output_item.added` (reasoning, summary []) →
   `response.reasoning_summary_part.added` (summary_index 0, пустой summary_text part) →
   `response.reasoning_summary_text.delta`* из thinking_delta (пустые дельты и
   signature_delta дропаются) → `response.reasoning_summary_text.done` →
   `response.reasoning_summary_part.done` → `response.output_item.done`; output_index —
   плотный счётчик, теперь включающий thinking-блоки (redacted_thinking пропускается
   без позиции — решение 4), reasoning item попадает в completed output;
   `output_tokens_details` из message_delta проксируются в usage (reasoning_tokens, как
   non-stream). Подписи/encrypted_content по-прежнему не выставляются (решение 4).
   Временные ограничения после 4.2: `store:true`/`previous_response_id`/`item_reference`
   → `400 documented_limitation` и `POST /v1/responses/input_tokens` openai-only
   (решение 5), `reasoning` items во входе принимаются и выбрасываются. В router
   продублированный `namespace_lane` chat/responses dispatch'ей вынесен в общий
   `pub(crate)` в `crates/router/src/catalog.rs`; **4.3** — Gemini-зеркало
   (Responses→generateContent в Gemini plane по образцу 3.3) — **РЕАЛИЗОВАН**:
   адаптер `crates/forward/src/gemini/responses.rs`, роут `POST /v1/responses` в
   `ProviderMode::Gemini` (router не менялся — dispatch `google/*` и gemini-alias'ов
   работает с 4.1). Поток — паттерн chat-адаптера 3.3: перевод в GenerateContentRequest
   → внутренний запрос на `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse`
   → общий `gemini_api()` без изменений → перевод ответа СНАРУЖИ. Responses-сторона
   словаря 4.1+4.2 (item-формы, события SSE, usage, status/incomplete_details)
   идентична Anthropic-адаптеру и закреплена contract-тестами модуля на тех же
   табличных ожиданиях. Запрос: `instructions` и system/developer items →
   `systemInstruction` (text-парт на каждый, instructions первым), `input` строка/items
   → contents со склейкой одноролевых, `input_image` → inlineData общим переводом
   (только data: URL — http(s) generateContent не принимает → `400 invalid_request`;
   `detail` != auto → `400 unsupported_parameter`), replay
   function_call/function_call_output → functionCall/functionResponse парты
   (`arguments` JSON-строка → `args`; functionResponse ссылается на вызов по ИМЕНИ —
   карта call_id→name по function_call items истории, output без пары →
   `400 invalid_request` — отличие от Anthropic-зеркала, где pairing не валидируется),
   `tools` → `[{"functionDeclarations": …}]` (плоский дескриптор, `strict` снимается),
   `tool_choice` → `toolConfig.functionCallingConfig`, `max_output_tokens` →
   `generationConfig.maxOutputTokens` (дефолт 4096), `reasoning.effort` →
   `generationConfig.thinkingConfig` (`thinkingLevel` проксируется как есть — minimal
   НЕ клампится, отличие от Anthropic-зеркала; `includeThoughts: true`), `text.format`
   json_schema → `responseMimeType: application/json` + `responseSchema` (обёртка
   снимается), json_object → `responseMimeType` (у generateContent есть — отличие от
   Messages, где json_object → 400). Capability matrix — те же 9 правил, что у
   Anthropic-зеркала, плюс `parallel_tool_calls` (у generateContent нет
   disable_parallel_tool_use — только дефолт true); НЕИЗВЕСТНЫЕ top-level поля →
   `400 unsupported_parameter` (закрытый список, как chat-адаптер 3.3 — Code Assist
   wrapper выбросил бы их молча). Ответ: thought-парты → reasoning items `rs_*` и
   reasoning_summary события словаря 4.2 (парт с одним thoughtSignature событий не
   порождает — решение 4), functionCall → function_call items `fc_*` с
   синтезированными call_id `callu_<name>[_N]` (functionCall.id на private wire нет —
   схема chat-адаптера) и ровно одной arguments-дельтой (functionCall приходит
   целиком); usage — input = `promptTokenCount`, output =
   `candidatesTokenCount`+`thoughtsTokenCount` (та же сумма, что тарифицирует
   metering), `cachedContentTokenCount` → `input_tokens_details.cached_tokens`,
   `thoughtsTokenCount` → `output_tokens_details.reasoning_tokens`;
   finishReason/blockReason → status через общий `map_finish_reason`: MAX_TOKENS →
   incomplete `max_output_tokens`, SAFETY/RECITATION/BLOCKLIST/PROHIBITED_CONTENT/SPII
   → incomplete `content_filter`. Stream: data-only SSE → Responses SSE; нормальное
   завершение Gemini-стрима — чистый EOF (message_stop на wire нет): открытый item
   закрывается done-событиями и эмитится `response.completed` (отличие от
   Anthropic-зеркала, где EOF без message_stop → `response.failed`); mid-stream
   error-кадр `{error:{code,message,status}}` и транспортный сбой → `response.failed`
   (error.code — google.rpc status). Ошибки — общий с chat-адаптером
   `convert_error_response` (Google-конверт → OpenAI-конверт, нативный
   `400 API_KEY_INVALID` → `401 authentication_error`, 402 и `Retry-After`
   сохраняются). Временные ограничения — как после 4.2: reasoning items входа
   выбрасываются, `store:true`/`previous_response_id`/`item_reference` →
   `400 documented_limitation` (решение 5). Плюс прод-проверенное
   upstream-ограничение плоскости (2026-08-01): replay tool-истории
   отклоняется Code Assist с `400 INVALID_ARGUMENT` — `thoughtSignature`
   functionCall-партов не сохраняется (решение 4), а thinking-модели Gemini
   требуют его при replay; то же у chat-адаптера 3.3 (см. п. 3). Общие хелперы (`chat_error`,
   `invalid_request`, `unsupported_parameter`, `convert_error_response`,
   `merge_or_push`, `gemini_image_part`/`translate_reasoning_effort`/
   `parse_tool_arguments` с именем параметра, `function_declaration`,
   `function_response_value`, `synthetic_call_id`, `map_finish_reason`, константы
   лимитов) вынесены в `pub(crate)` в `gemini/chat.rs` (по образцу выноса 4.1 в
   `anthropic.rs`). e2e-smoke Gemini-цепочки не добавлялся (плоскость требует
   encrypted OAuth-пул, как в 3.3); e2e-покрытие universal lane — Anthropic-цепочка
   в `tests/universal_chat_smoke.sh`.
5. **Anthropic Skin для non-Claude моделей (3–5 недель).** Messages-вход для GPT/Gemini:
   beta fields, tool streaming, thinking, error recovery, token counting — по решению 6
   (зеркало решений 3–4, thinking без подписей). **5.1 — Anthropic Skin для `openai/*`
   моделей (Codex plane) — РЕАЛИЗОВАН.** В router `POST /v1/messages` получил model-based
   dispatch (`crates/router/src/messages.rs`) по тем же правилам, что chat/responses
   dispatch'и 3.1/4.1: буферизуется только тело запроса (32 MiB), namespace-префикс
   `openai/` выбирает Codex plane без опроса каталога (общий `catalog::namespace_lane`;
   `anthropic/` и `google/` уходят на свои плоскости — Gemini Messages skin реализован
   в 5.2 ниже),
   остальное — alias через кэшированный каталог; тело проксируется без изменений, ошибки
   dispatch — в Anthropic-конверте. Namespaced `anthropic/<id>` на Anthropic plane
   снимается admission'ом плоскости до reserve и upstream (`strip_own_namespace` в
   `crates/forward/src/proxy.rs`, зеркало strip'а chat-адаптера 3.x): до этого исправления
   префикс доезжал до upstream байт-идентично и тот отвечал 404 (прод-проба 2026-08-01).
   `POST /v1/messages/count_tokens` использует тот же model-based dispatch: native
   Anthropic lane, локальный reserve-grade подсчёт Codex или Gemini endpoint из 5.2.
   На Codex plane
   (`crates/forward/src/codex/skin.rs`, роуты `/v1/messages` и
   `/v1/messages/count_tokens` в `ProviderMode::OpenAi`) Messages-запрос переводится в
   Responses JSON и идёт через тот же turn pipeline, что chat-адаптер (admission,
   affinity, reserve, run, settle по authoritative usage): strip `openai/`-префикса,
   `speed:"fast"` и совместимые `service_tier:"fast"|"priority"` → canonical
   Responses `service_tier:"priority"` (остальные/отсутствующие значения → Standard),
   top-level `system` (строка или text-блоки, склейка \n\n) → `instructions`, user
   text/image-блоки → `input_text`/`input_image` (общий `canonical_image_part`),
   assistant text → `output_text`, replay tool-истории — зеркало 4.2 (`tool_use` →
   `function_call` с `call_id` и arguments-строкой, `tool_result` →
   `function_call_output`; pairing не валидируется), thinking/redacted_thinking входных
   блоков дропаются (решение 6), `tools[]` → function tools (`input_schema` →
   `parameters`; server tools → 400), `tool_choice` auto/any/none/tool →
   default/required/none/named (+`disable_parallel_tool_use` →
   `parallel_tool_calls:false`), `thinking` → `reasoning.effort` (lossy: disabled/
   adaptive → дефолт модели; enabled budget <4096 → low, <16384 → medium, иначе high;
   <1024 → 400), `stop_sequences` и `max_tokens` честно обрабатываются на доставленном
   тексте общим `StopFilter` и output-бюджетом ~4 chars/token (как chat.rs — транспорт
   не умеет резать генерацию upstream). Capability matrix: не-дефолтный `cache_control`
   где угодно (system, content-блоки, tools), `context_management`, `mcp_servers`,
   `container`, `output_config` → `400 invalid_request_error` с именем параметра;
   `metadata` (включая `user_id`), sampling controls и неизвестные поля принимаются и
   игнорируются (та же leniency, что у chat.rs, — Claude Code шлёт `metadata.user_id`
   в каждом запросе). Ответ — зеркало словаря 4.1+4.2: message items → text-блок на
   позиции первого message item, `function_call` → `tool_use` (arguments парсятся в
   `input`, невалидный JSON → `{}`), `reasoning` → thinking-блоки БЕЗ signature
   (summary-парты склеиваются \n\n), usage → Messages usage (cache write/read →
   `cache_creation_input_tokens`/`cache_read_input_tokens` при >0, reasoning →
   `output_tokens_details.thinking_tokens`, effective tier → `service_tier`), stop_reason:
   function_call в output →
   `tool_use`, срез output-бюджета → `max_tokens`, совпавшая stop_sequence →
   `stop_sequence`, иначе `end_turn`. SSE: `message_start` с нулевым usage
   (authoritative usage существует только в конце turn — задокументированное
   ограничение) → per-block `content_block_start`/`content_block_delta`
   (`text_delta`, `thinking_delta`, `input_json_delta`)/`content_block_stop` (плотные
   индексы, новый тип блока закрывает предыдущий) → `message_delta` (stop reason +
   usage) → `message_stop`; heartbeat — `event: ping`, mid-stream отказ — `event:
   error`; disconnect клиента не убивает turn — он добегает до authoritative usage для
   settlement (как chat.rs). Все ошибки endpoint'а (валидация адаптера, общий парсер,
   admission, billing) пересобираются в Anthropic-конверт с сохранением статуса и
   `Retry-After` (503 → 529 `overloaded_error`, 402 сохраняется — Claude Code
   восстанавливается по тексту ошибки). `POST /v1/messages/count_tokens` на плоскости —
   тот же parse + `parse_responses_request`/`prepare_turn` → reserve-grade оценка
   `input_tokens` без сети (`max_tokens` там опционален, как у официального endpoint'а).
   Ограничения 5.1: лимит тела — 8 MiB (общий `OPENAI_BODY_LIMIT` плоскости, не 32),
   сквозного e2e-smoke Codex plane нет (харнесс
   не умеет encrypted OAuth-профили — покрытие unit/contract-тестами, как 3.3/4.3).
   **5.2 — Anthropic Skin для `google/*` моделей (Gemini plane) — РЕАЛИЗОВАН.**
   Gemini-зеркало 5.1 (`crates/forward/src/gemini/skin.rs`, роуты `/v1/messages` и
   `/v1/messages/count_tokens` в `ProviderMode::Gemini`; router не менялся — dispatch
   `google/*` и gemini-alias'ов работает с 5.1): Messages-сторона словаря идентична 5.1
   (system/messages/tools/tool_choice/thinking/capability matrix, Messages SSE,
   Anthropic-конверт ошибок — contract-тесты обоих модулей на эквивалентном входе),
   перевод запроса и разбор ответа — по правилам chat/responses-адаптеров плоскости
   (3.3/4.3), общие хелперы переиспользованы из `gemini/chat.rs` без изменения его
   логики. Запрос: strip `google/`-префикса ДО admission, top-level `system` →
   `systemInstruction` (склейка \n\n, не-дефолт `cache_control` → 400), messages →
   contents общим `merge_or_push` (assistant → роль model; `tool_use` → functionCall с
   `args` OBJECT — не JSON-строка, отличие от Codex-стороны; `tool_result` →
   functionResponse, pairing по карте id→name валидируется — паттерн 3.3/4.3), image:
   только base64 → inlineData (url source → 400), thinking входа дропается (решение 6);
   `disable_parallel_tool_use: true` → 400 (у generateContent нет аналога); sampling
   (temperature/top_p/top_k) и `stop_sequences` проксируются в generationConfig (умеет
   нативно — плоскостное отличие от 5.1; stop_reason `stop_sequence` неразличим →
   `end_turn`); capability matrix — те же 4 правила 5.1 ПЛЮС закрытый список top-level
   полей (неизвестное → 400, как chat.rs). Ответ: text-парты → один text-блок,
   thought-парты → thinking-блоки БЕЗ signature, functionCall → `tool_use` с
   синтезируемым `toolu_<name>[_N]`, usage — input=`promptTokenCount`, output=
   candidates+thoughts (thoughts → `output_tokens_details.thinking_tokens`, cached →
   `cache_read_input_tokens`). Хендлеры идут через общий `gemini_api()` внутренним
   Request на `generateContent|streamGenerateContent?alt=sse|:countTokens` — admission,
   reserve, affinity, ротация, Code Assist wrapper и usage-settlement без единого
   изменения; `count_tokens` — нативный `:countTokens` (quota-free, без reserve),
   `max_tokens` там опционален. Ограничения 5.2: прод-ограничение replayed
   tool-истории (`400 INVALID_ARGUMENT` Code Assist, thoughtSignature — решение 4)
   разделяется с chat/responses lanes плоскости (прямой tool calling работает); лимит
   тела — общий плоскости; сквозного e2e-smoke Gemini plane нет (как 3.3/4.3 — покрытие
   unit/contract-тестами модуля). Router dispatch `/v1/messages/count_tokens` покрыт
   интеграционными mock-тестами namespace и alias-путей всех трёх плоскостей.
6. **OpenRouter-grade routing (2–4 недели).** Provider preferences, явные
   fallback-списки, attempt fencing (execution group / единственный billable winner,
   см. «Семантика fallback»), per-account policy, telemetry, presets. По решению 7 первым
   пакетом этапа идёт детальный дизайн на живой телеметрии этапов 3–5 — он зафиксирован в
   `docs/engine/ROUTING_FENCING.md` (фактбаза, контракт `execution_state=not_started`,
   group/attempt identity, фазировка 6.1–6.4). Фаза 6.1 реализована (2026-08-01):
   плоскости выставляют `x-apitoken-execution-state: not_started` на не-2xx отказах до
   границы started при гарантии refund/cancel reserve, router снимает заголовок со всех
   транзитных ответов. Gemini Messages skin и universal Chat/Responses-адаптеры обеих
   переводящих плоскостей сохраняют сигнал на pre-delivery не-2xx и снимают его с
   пересобранных ошибок после 2xx, когда charge возможен (§3.2 там же). Фаза 6.2
   реализована (2026-08-02): shared router engine принимает default-off `models`,
   preflight-валидирует цепочку и делает serial retry только по exact signal либо
   ConnectionRefused; timeout/unsigned 5xx/client 4xx fail closed, внутренний header
   никогда не виден клиенту. Фаза 6.3 реализована (2026-08-02): trusted group/attempt identity
   проходит router→plane→reservation, а SQLite/PostgreSQL settle выбирает ровно одного
   billable winner и полностью возвращает loser hold; `ExecutionGroupDoubleWinner` гейтит любой
   такой инцидент. Фаза 6.4 добавит policy/presets и GA telemetry. Отдельно —
   Stage 3 HA: второй host, router replicas, HA PostgreSQL (см. ограничения в
   `docs/engine/STAGE2_POSTGRES_AUTHORITY.md`: потеря единственного host пока не
   покрыта — это Stage 3, а не блокер router'а).

Полезный единый native endpoint — этапы 1a–1b. Production-grade multiprotocol parity —
ориентировочно 8–14 инженерных недель последовательно. Основная сложность не в изоляции
отказов (она уже есть), а в корректном переводе tools/reasoning/streaming и exactly-once
биллинге.

## Открытые решения

- ~~Имя публичного домена~~ — решено на этапе 1a: `router.apitoken.sale` (новый отдельный
  hostname, `api.apitoken.sale` не переиспользуется и не меняет поведение).
- ~~Политика частичного каталога `/v1/models` при падении плоскости~~ — решено на этапе 1b:
  TTL-кэш 30 с + last-good без TTL; упавшая плоскость опускается из выдачи, деградация
  маркируется заголовком `x-apitoken-catalog-degraded` со списком namespace'ов; пустой
  каталог плоскости считается сбоем и не кэшируется; 401/403 любой плоскости → единый 401
  `invalid_api_key`; все плоскости недоступны без кэша → 503 `catalog_unavailable`.
- Продуктовый охват Gemini: OpenKeys и commerce-контракты сейчас фиксируют только
  Anthropic/OpenAI (`docs/product/OPENKEYS.md`, `packages/contracts`) — расширение
  enum/каталогов идёт отдельным expand-only шагом до публичного включения Gemini
  в unified endpoint.

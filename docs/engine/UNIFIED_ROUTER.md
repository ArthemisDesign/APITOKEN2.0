# UNIFIED_ROUTER — единый endpoint для всех провайдеров (целевая архитектура)

Статус: **этап 1a реализован** (Caddy fan-in: `router.apitoken.sale` обслуживает native lanes
по форме пути на существующих loopback origins). `crates/router`, единый агрегированный
каталог `/v1/models` и universal lane пока не существуют — на этапе 1a `GET /v1/models{,/{id}}`
на unified hostname осознанно отвечает `404`, а не каталогом одной плоскости. Документ
фиксирует целевую картину, публичный контракт, инварианты и этапный план; каждый этап при
реализации обновляет этот документ и смежные инструкции в том же коммите.

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
| Provider preferences / fallback | да | да (этап 5, с attempt fencing) |

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
POST /v1/messages/count_tokens                    Anthropic token counting

POST /v1/responses                                OpenAI Responses (Codex)
POST /v1/responses/input_tokens                   OpenAI token counting
GET  /v1/responses/{id}                           stored response (семантика — этап 4,
GET  /v1/responses/{id}/input_items               либо явные ограничения)

POST /v1/chat/completions                         universal OpenAI-compatible вход
                                                  (этап 1a — только OpenAI plane,
                                                   этап 3 — любая модель каталога)

GET  /v1/models                                   единый агрегированный каталог (этап 1b;
GET  /v1/models/{id}                               на 1a — 404, коллизия native-путей)

GET  /v1beta/models                               Gemini native
POST /v1beta/models/{id}:generateContent
POST /v1beta/models/{id}:streamGenerateContent    (alt=sse и alt=json)
POST /v1beta/models/{id}:countTokens
```

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

## Инварианты

1. **Биллинг только в provider-плоскости.** Router не резервирует и не списывает деньги.
   Ключ клиента передаётся в плоскость verbatim; `request_id`, reserve → delivering →
   settle и exactly-once ledger остаются ответственностью плоскости
   (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`). Двойное списание исключено конструктивно.
2. **Router не ретраит ничего, что могло дойти до плоскости.** Повтор по timeout после
   отправки запроса создал бы новый `request_id` и второе списание: backend мог выполнить
   запрос и settle, даже если router ответа не получил. Retry допустим только до отправки
   запроса в плоскость (connection refused). Подробности — «Семантика fallback».
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
`crates/registry/src/pricing/snapshots.rs`).

`/v1/models` — единственная коллизия путей native-плоскостей: unified endpoint обязан
агрегировать каталоги всех плоскостей (кэш, частичный каталог при падении одной
плоскости, без блокировки остальных). Именно агрегация каталога — первый код,
оправдывающий `crates/router`.

## Семантика fallback и billing fencing

Наивное правило «fallback только до первого байта» недостаточно: при timeout backend мог
выполнить запрос и settle, даже если router ответа не получил. Отсюда градация:

- **Этапы 1a–4: межмодельного fallback нет вообще.** Единственный retry — существующий
  внутри плоскости до первого публичного байта (no-byte retry boundary), он безопасен,
  потому что не создаёт новый billable request после начала доставки.
- **Этап 5, MVP fallback:** повтор на другую модель только после явного внутреннего
  ответа плоскости `execution_state=not_started`. Этого контракта сейчас нет — он
  добавляется в плоскости вместе с routing-этапом.
- **Зрелая версия:** общий execution group / attempt ID, идемпотентные reservations и
  атомарный выбор единственного billable winner — reservation identity расширяется с
  `request_id` на `(group_id, attempt_id)`, а settled-запись допускает ровно один winner
  на группу (расширение текущего `UNIQUE ledger(kind, request_id)`).
- **Ambiguous disconnect → никакого автоматического повтора на другой модели.** Клиент
  получает честную ошибку и решает сам; молчаливый retry на timeout — путь к двойному
  списанию.

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
2. **1b. `crates/router` + единый каталог (~1 неделя).** Stateless router: единый
   `/v1/models` (агрегация, кэш, частичный каталог), namespaced ID + aliases,
   auth passthrough без изменений. Без cross-provider translation и fallback.
3. **Universal Chat (2–4 недели).** `/v1/chat/completions` для всех моделей каталога:
   text, images, tools, structured output, streaming через canonical IR.
4. **Universal Responses для Codex-parity (2–4 недели).** Function/custom tools,
   reasoning events, usage; stored-response semantics либо явные ограничения.
5. **Anthropic Skin для non-Claude моделей (3–5 недель).** Messages-вход для GPT/Gemini:
   beta fields, tool streaming, thinking, error recovery, token counting.
6. **OpenRouter-grade routing (2–4 недели).** Provider preferences, явные
   fallback-списки, attempt fencing (execution group / единственный billable winner,
   см. «Семантика fallback»), per-account policy, telemetry, presets. Отдельно —
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
- Политика частичного каталога `/v1/models` при падении плоскости (TTL кэша, маркировка).
- Продуктовый охват Gemini: OpenKeys и commerce-контракты сейчас фиксируют только
  Anthropic/OpenAI (`docs/product/OPENKEYS.md`, `packages/contracts`) — расширение
  enum/каталогов идёт отдельным expand-only шагом до публичного включения Gemini
  в unified endpoint.

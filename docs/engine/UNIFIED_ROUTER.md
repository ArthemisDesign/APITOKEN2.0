# UNIFIED_ROUTER — единый endpoint для всех провайдеров (целевая архитектура)

Статус: **архитектурное решение (design), не реализовано**. Ни `crates/router`, ни
публичный unified-домен пока не существуют. Документ фиксирует целевую картину, инварианты
и этапный план; каждый этап при реализации обновляет этот документ и смежные инструкции
в том же коммите.

## Контекст и цель

Продуктовая цель — повторить модель OpenRouter (один аккаунт, один ключ, один баланс,
единый каталог моделей нескольких провайдеров, pay-as-you-go), но **без потери качества
для harness-агентов** (Claude Code, Codex, Gemini-клиенты). OpenRouter гонит все запросы
через один OpenAI-совместимый формат и неизбежно обрезает провайдер-специфику
(thinking signatures, Anthropic beta-поля, encrypted reasoning, stored responses). Наше
отличие: тяжёлые harness'ы получают не перевод, а настоящий API провайдера.

Ключевой факт, делающий решение дешёвым: три provider-плоскости уже независимы на уровне
процессов и уже делят один fenced PostgreSQL billing authority — ключи `sk-pool-…`
работают на всех плоскостях (см. `docs/engine/ARCHITECTURE.md`,
`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`).

## Целевая архитектура

```
                    router.apitoken.sale — единственная публичная точка
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
SSE и provider-native errors. Claude Code говорит Anthropic Messages (включая
`/v1/messages/count_tokens`, `anthropic-beta`/`anthropic-version` verbatim, небуферизованный
SSE), Codex — Responses API, Gemini-клиент — `/v1beta/models/*:generateContent` и т.д.
Это основной вход для harness-агентов и гарантия качества уровня протокола.

### Universal lane

Клиент, умеющий только OpenAI Chat Completions (Aider, Continue, Roo/Kilo, Hermes,
большинство IDE-плагинов), шлёт в `/v1/chat/completions` любую модель из каталога.
Router переводит через типизированный canonical Turn/Event IR (content blocks, tool
calls/results, structured output, thinking/reasoning, prompt-cache boundaries, usage,
canonical streaming events) в нативный протокол выбранной плоскости.

Контракт перевода — **строгий, fail-closed**:

- неподдерживаемая target-плоскостью capability → понятный `400 unsupported_parameter`,
  никакого молчаливого выбрасывания `strict`, `thinking`, server tools, response schema;
- opaque reasoning artifacts (Claude thinking signatures, OpenAI encrypted reasoning,
  Gemini thought signatures) имеют provider provenance: возвращаются только тому же
  провайдеру либо отклоняются; молчаливое удаление запрещено (ломает agent loop).

## Инварианты

1. **Биллинг только в provider-плоскости.** Router не резервирует и не списывает деньги.
   Ключ клиента передаётся в плоскость verbatim; `request_id`, reserve → delivering →
   settle и exactly-once ledger остаются ответственностью плоскости
   (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`). Двойное списание исключено конструктивно.
2. **Router не ретраит ничего, что могло дойти до плоскости.** Повтор по timeout после
   отправки запроса создал бы новый `request_id` и второе списание: backend мог выполнить
   запрос и settle, даже если router ответа не получил. Retry допустим только до отправки
   запроса в плоскость (connection refused). Автоматический межмодельный fallback при
   неоднозначном disconnect запрещён; безопасный fallback — только после явного
   `execution_state=not_started` (контракта пока нет, появится на этапе 5).
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
   остаются аварийными прямыми входами — клиент всегда может обойти router.
7. **Router — отдельный bounded context** (`crates/router`), общается с плоскостями
   только по HTTP, не импортирует `pool`/`forward`. Control API — loopback-only
   управление аккаунтами/прайсингом; в data-plane router'а он не участвует.

## Модели и каталог

Единый каталог публикует namespaced ID: `anthropic/claude-*`, `openai/gpt-*`,
`google/gemini-*`. Namespace означает семейство модели, не обязательно единственного
исполнителя: при появлении альтернативных backends одной модели route planner сможет
выбирать между ними. Текущие нативные ID остаются однозначными aliases. Источник правды
для каталога — существующий versioned multi-provider pricing catalog
(`docs/engine/CONTROL_API.md`, `crates/registry/src/pricing/snapshots.rs`).

`/v1/models` — единственная коллизия путей native-плоскостей: unified endpoint обязан
агрегировать каталоги всех плоскостей (кэш, частичный каталог при падении одной
плоскости, без блокировки остальных). Именно агрегация каталога — первый код,
оправдывающий `crates/router`.

Поведение harness'ов учтено в namespaces: Claude Code игнорирует discovery-ID, не
начинающиеся с `claude`/`anthropic`, поэтому `anthropic/claude-*` совместим
(`/v1/messages/count_tokens` для Claude Code опционален; model discovery требует
`CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`). Codex требует полноценный Responses API —
custom provider поддерживает только `wire_api="responses"`, chat-only прокси недостаточно.

## Этапный план

1. **1a. Caddy fan-in (~1 день).** Новый публичный hostname с path-based маршрутизацией
   на существующие loopback origins: `/v1/messages*` → 8790, `/v1/responses` +
   `/v1/chat/completions` → 8792, `/v1beta/*` → 8794. Без нового кода; изоляция,
   биллинг и auth passthrough — «из коробки». Провайдер определяется путём, не ключом
   и не моделью.
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
   fallback-списки, attempt fencing (execution group / единственный billable winner),
   per-account policy, telemetry, presets. Отдельно — Stage 3 HA: второй host,
   router replicas, HA PostgreSQL (см. ограничения в
   `docs/engine/STAGE2_POSTGRES_AUTHORITY.md`).

Полезный единый native endpoint — этапы 1a–1b. Production-grade multiprotocol parity —
ориентировочно 8–14 инженерных недель последовательно. Основная сложность не в изоляции
отказов (она уже есть), а в корректном переводе tools/reasoning/streaming и exactly-once
биллинге.

## Открытые решения

- Имя публичного домена: новый `router.apitoken.sale` или существующий `api.apitoken.sale`
  как единая точка (с переносом прямого Anthropic-входа).
- Политика частичного каталога `/v1/models` при падении плоскости (TTL кэша, маркировка).
- Продуктовый охват Gemini: OpenKeys и commerce-контракты сейчас фиксируют только
  Anthropic/OpenAI (`docs/product/OPENKEYS.md`, `packages/contracts`) — расширение
  enum/каталогов идёт отдельным expand-only шагом до публичного включения Gemini
  в unified endpoint.

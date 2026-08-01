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
- **Fail-closed retry.** Native lanes и universal-запросы без явного `models`
  выполняют ровно одну попытку. При включённом `CLAUDE_ROUTER_FALLBACK_ENABLED`
  следующая модель из явной цепочки разрешена только после точного не-2xx
  `x-apitoken-execution-state: not_started` (кроме 401/402/прочих клиентских
  4xx; signed 429 — capacity-отказ) либо доказанного TCP `ConnectionRefused`.
  Timeout, DNS/generic connect error, unsigned 5xx, reset/обрыв и ответ после
  заголовков никогда не ретраятся (`docs/engine/ROUTING_FENCING.md` §3.3).
- **Никаких очередей, semaphore, circuit breaker, rate limits** (инвариант 3).
  Readiness (`/health`, `/live`, `/ready`) — router-local, никогда не
  конъюнкция health плоскостей; синхронных health-check'ов на пути запроса нет.
- **SSE не буферизуется.** Тела запроса и ответа — потоки
  (`Body::wrap_stream`/`Body::from_stream`); reqwest собран без auto-decode
  (default-features off), чтобы байты и Content-Encoding шли неизменно.
  Единственное исключение — shared `routing.rs`: тело ЗАПРОСА
  `/v1/chat/completions`, `/v1/responses` и
  `/v1/messages{,/count_tokens}` читается целиком (лимит 32 MiB) ради поля
  `model`; тело ответа остаётся потоком.
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
- `proxy.rs` — байт-в-байт proxy native lanes, auth passthrough и классификация
  одной попытки до публичных headers: exact `not_started` / source-chain
  `ConnectionRefused`; внутренний заголовок снимается до сборки ответа.
- `routing.rs` — общий model dispatch и serial fallback для всех universal
  поверхностей. Без `models` сохраняет исходные байты и прямой namespaced
  dispatch; с `models` до первой попытки валидирует всю цепочку одним aggregate
  catalog snapshot, отбрасывает дубликаты alias/namespaced одной модели,
  удаляет `models`, подставляет выбранный `model` и исполняет retry matrix.
  Логи attempts содержат только surface/index, публичный catalog ID, lane,
  status и bounded retry reason — без URL, headers, credentials и тел запросов.
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
  + aliases, TTL-кэш 30 с, last-good при падении плоскости, маркер деградации
  `x-apitoken-catalog-degraded`. Здесь же — общий для universal dispatch'ей
  `pub(crate) namespace_lane` (прямой выбор плоскости без catalog fetch для
  запросов без fallback).
- `error.rs` — синтетические ошибки router'а в конверте соответствующего
  провайдера (ошибки плоскостей проксируются байт-в-байт, сюда не попадают).
- `main.rs` — таблица маршрутов публичного контракта + композиция.

## Проверка

```bash
cargo test -p claude-router   # unit + интеграционные (mock-плоскости на TCP)
cargo build                   # весь workspace зелёный до коммита
```

Интеграционные тесты поднимают mock-плоскости на реальных loopback-сокетах и
покрывают: passthrough тела/заголовков, небуферизованный SSE, транзитивный
disconnect, агрегацию/деградацию/stale каталога, 401/503 каталога, alias-
разрешение моделей, 404/405, model-based dispatch chat-, responses- и
messages- и messages/count_tokens-запросов (namespaced без catalog fetch,
alias через каталог, 400 невалидного/слишком большого тела, небуферизованный
chat SSE), а также off-by-default fallback, preflight всей цепочки, точный
retry matrix (`not_started`, 429, 4xx/5xx, ConnectionRefused/timeout), rewrite
per-attempt body и обязательное снятие внутреннего заголовка. Живой PostgreSQL
и подписки не нужны.
Полная цепочка router→engine→mock upstream — `tests/universal_chat_smoke.sh`.

## Эксплуатация

Юнит `systemd/claude-router.service` (singleton, `127.0.0.1:8798`; blue-green
реплики — этап 6 документа). Публичная граница — Caddy vhost
`router.apitoken.sale` (см. `deploy/CADDY.md`). Перезапуск рвёт живые стримы
клиентов: плоскости корректно settle-ят через TeeMeter drain, клиенты
переподключаются сами.

Fallback после выката остаётся выключен: отсутствие env-флага — контрактный
default. Canary включает его только явным
`CLAUDE_ROUTER_FALLBACK_ENABLED=1`; поле `models` при выключенном флаге получает
lane-shaped `400` до catalog fetch или обращения к плоскости.

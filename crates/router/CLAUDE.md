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
- **Ноль ретраев.** Отказ соединения → честная 502 (`error::upstream_unavailable`).
  Повтор после отправки запроса создал бы второй billable request_id в плоскости
  (инвариант 2 документа). Connection retry на уровне Caddy origins уже есть —
  здесь не дублируем.
- **Никаких очередей, semaphore, circuit breaker, rate limits** (инвариант 3).
  Readiness (`/health`, `/live`, `/ready`) — router-local, никогда не
  конъюнкция health плоскостей; синхронных health-check'ов на пути запроса нет.
- **SSE не буферизуется.** Тела запроса и ответа — потоки
  (`Body::wrap_stream`/`Body::from_stream`); reqwest собран без auto-decode
  (default-features off), чтобы байты и Content-Encoding шли неизменно.
  Единственное исключение — `chat.rs`, `responses.rs` и `messages.rs`: тело
  ЗАПРОСА `/v1/chat/completions`, `/v1/responses` и
  `/v1/messages{,/count_tokens}` читается целиком (лимит 32 MiB) ради поля
  `model`; тело ответа остаётся потоком.
  Disconnect клиента обязан транзитивно рвать соединение к плоскости
  (TeeMeter drain): поэтому вокруг тела ответа нет detached-тасков.
- **Внутренняя семантика исполнения не транслируется клиенту.** Заголовок
  `x-apitoken-execution-state` (контракт `docs/engine/ROUTING_FENCING.md` §3, этап 6.1) —
  контракт движок↔router: плоскости выставляют его на отказах без исполнения
  (`not_started`), router обязан снимать его со ВСЕХ транзитных ответов перед отдачей
  клиенту (`proxy.rs` `EXECUTION_STATE_HEADER`). За условия заголовка отвечает только
  сам движок — router не транслирует чужое обещание. Клиенты не должны зависеть от
  внутреннего состояния движка; failover по этому сигналу — фаза 6.2, до неё заголовок
  на публичной границе недоступен нигде.
- **Деньги — только integer**: router денег не касается вовсе; если когда-либо
  появятся суммы — nanoUSD-строки, никакого float.

## Что здесь живёт

- `config.rs` — единственное место чтения env (`CLAUDE_ROUTER_*`).
- `proxy.rs` — байт-в-байт прокси native lanes + auth passthrough.
- `chat.rs` — model-based dispatch `POST /v1/chat/completions` (этап 3.1):
  буферизует только тело ЗАПРОСА (32 MiB), извлекает `model`, выбирает
  плоскость по namespace-префиксу без опроса каталога (общий
  `catalog::namespace_lane`) либо по alias через кэшированный каталог; тело
  проксируется без изменений (namespaced ID резолвит admission плоскости),
  ошибки dispatch — в OpenAI-конверте.
- `messages.rs` — model-based dispatch `POST /v1/messages` и
  `POST /v1/messages/count_tokens` (этапы 5.1–5.2) по тем
  же правилам, что `chat.rs` (та же `catalog::namespace_lane`), но с ошибками
  dispatch в Anthropic-конверте: namespaced `openai/*` уходит на Codex plane
  (там Messages→Responses адаптер `crates/forward/src/codex/skin.rs`),
  `anthropic/*` — на Anthropic plane как native lane, `google/*` — на Gemini
  plane по общему namespace-правилу (Messages→generateContent skin реализован
  в `crates/forward/src/gemini/skin.rs`). Для `count_tokens` выбирается та же
  плоскость: Anthropic native, reserve-grade локальный подсчёт Codex или
  quota-free native `:countTokens` Gemini.
- `responses.rs` — model-based dispatch `POST /v1/responses` (этап 4.1) по
  тем же правилам, что `chat.rs` (та же `catalog::namespace_lane`). Stored
  responses endpoints (`/v1/responses/input_tokens`, `/v1/responses/{id}`,
  `.../input_items`) dispatch не используют — они остаются native OpenAI lane
  (stored responses только `openai/*`, решение 5).
- `catalog.rs` — единый `/v1/models`: агрегация трёх плоскостей, namespaced ID
  + aliases, TTL-кэш 30 с, last-good при падении плоскости, маркер деградации
  `x-apitoken-catalog-degraded`. Здесь же — общий для universal dispatch'ей
  `pub(crate) namespace_lane` (выбор плоскости по namespace-префиксу модели;
  правила обеих lanes обязаны совпадать).
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
messages- и messages/count_tokens-запросов
(namespaced без catalog fetch, alias через каталог, 400 невалидного/слишком
большого тела, небуферизованный chat SSE). Живой PostgreSQL и подписки не нужны.
Полная цепочка router→engine→mock upstream — `tests/universal_chat_smoke.sh`.

## Эксплуатация

Юнит `systemd/claude-router.service` (singleton, `127.0.0.1:8798`; blue-green
реплики — этап 6 документа). Публичная граница — Caddy vhost
`router.apitoken.sale` (см. `deploy/CADDY.md`). Перезапуск рвёт живые стримы
клиентов: плоскости корректно settle-ят через TeeMeter drain, клиенты
переподключаются сами.

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
  Disconnect клиента обязан транзитивно рвать соединение к плоскости
  (TeeMeter drain): поэтому вокруг тела ответа нет detached-тасков.
- **Деньги — только integer**: router денег не касается вовсе; если когда-либо
  появятся суммы — nanoUSD-строки, никакого float.

## Что здесь живёт

- `config.rs` — единственное место чтения env (`CLAUDE_ROUTER_*`).
- `proxy.rs` — байт-в-байт прокси native lanes + auth passthrough.
- `catalog.rs` — единый `/v1/models`: агрегация трёх плоскостей, namespaced ID
  + aliases, TTL-кэш 30 с, last-good при падении плоскости, маркер деградации
  `x-apitoken-catalog-degraded`.
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
разрешение моделей, 404/405. Живой PostgreSQL и подписки не нужны.

## Эксплуатация

Юнит `systemd/claude-router.service` (singleton, `127.0.0.1:8798`; blue-green
реплики — этап 6 документа). Публичная граница — Caddy vhost
`router.apitoken.sale` (см. `deploy/CADDY.md`). Перезапуск рвёт живые стримы
клиентов: плоскости корректно settle-ят через TeeMeter drain, клиенты
переподключаются сами.

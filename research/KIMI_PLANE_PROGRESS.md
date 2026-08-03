# KIMI plane — журнал прогресса (resumable)

**Назначение.** Этот файл существует, чтобы работа над плоскостью KIMI не терялась при сбросе
контекста агента. Он — источник истины о том, что сделано и что делать следующим. После любого
компакта/рестарта агент читает **сначала его**, а не восстанавливает план по памяти.

Правила ведения:

- Обновляется **в том же коммите**, что и сам шаг. Отдельный «коммит про прогресс» отстаёт от
  реальности и хуже отсутствующего.
- Строка «сделано» обязана нести SHA. Без SHA утверждение непроверяемо.
- Раздел «следующее действие» всегда содержит ровно одну конкретную задачу, а не список пожеланий.

Контракты: `docs/engine/PROVIDER_ONBOARDING.md` (что доказать),
`docs/engine/PROVIDER_WIRING_CHECKLIST.md` (что редактировать),
`docs/engine/KIMI_PROVIDER.md` (факты о провайдере и открытые `unknown`).

## Терминальное состояние (когда работа считается законченной)

1. Оператор создаёт оффер KIMI, продавец проходит путь целиком, профиль публикуется в roster.
2. Движок читает roster, профиль становится маршрутизируемой ёмкостью и отдаёт трафик.
3. Трафик тарифицируется по официальному rate card **по served-модели**.
4. Калибровка пишет immutable evidence из реального движения квоты `/usages`.
5. Точный production SHA зелёный, live-матрица пройдена на нашей подписке.

Пункты 1–4 достижимы без живой подписки (моки/гейты). Пункт 5 без неё заблокирован.

## Сделано

| Шаг | Артефакт | SHA |
|---|---|---|
| research / capability manifest | `docs/engine/KIMI_PROVIDER.md` | `dfb6ff52` |
| официальный rate card | `crates/metering/src/kimi.rs` | `f9234643` |
| calibration authority (schema) | `migrations_pg/0027_kimi_window_calibration.sql` | `2afdd183` |
| credential | `crates/kimi-credential` | `b895587a` |
| типы наблюдений + estimator | `registry/src/kimi_calibration.rs`, `forward/src/kimi_calibration.rs` | `2ff081a9` |
| Auth Bot: device-code протокол | `crates/authbot/src/kimi_oauth.rs` | `f95f9f05` |
| Auth Bot: продукт и кнопки тарифов | `crates/authbot/src/bot.rs` | `4ed97c07` |
| механическая карта подключения | `docs/engine/PROVIDER_WIRING_CHECKLIST.md` | `98ab7093` |
| **всё вышеперечисленное в master, watchdog GREEN** | — | `62c49ab6` |
| правило завершённости в скилле и карте | `SKILL.md`, чеклист | `bef32d75` |
| resumable-леджер как общее правило | `SKILL.md`, чеклист | `2645edab` |
| **всё вышеперечисленное в master, watchdog GREEN** | — | `f3974ac4` |
| Плоскость: refresh + разбор `/usages` | `crates/forward/src/kimi/client.rs` | `c2117d2e` |
| Плоскость: цикл попыток и граница первого байта | `crates/forward/src/kimi/pool.rs` | `4a4e3ae0` |
| Durable-калибровка: типы turn event + валидация | `crates/registry/src/kimi_calibration.rs` | `07df7519` |
| Durable-калибровка: PostgreSQL read/write + CAS | `crates/registry/src/pg.rs` | `34380519` |
| Плоскость: bounded turn-FIFO и порядок settle→quota | `crates/forward/src/kimi/queue.rs` | `de47a4d4` |
| Плоскость: конфиг, default-off switch, readiness | `crates/forward/src/kimi/config.rs` | `c233a1b9` |
| Durable-калибровка: concurrent replay + real-PG CAS/idempotency | `crates/registry/src/pg.rs` | `10443334` |
| Server: strict default-off env → typed plane config | `crates/server/src/config.rs` | текущий checkpoint |
| Auth Bot: публикация roster | `crates/authbot/src/kimi_roster.rs` | `dc175204` |
| Auth Bot: обработчик `km_ready` (шов №1 закрыт) | `crates/authbot/src/bot.rs` | `391032bc` |
| Плоскость: загрузчик roster | `crates/forward/src/kimi/roster.rs` | `d8a37422` |
| Плоскость: классы ошибок + single-flight refresh | `crates/forward/src/kimi/transport.rs` | `9b61c443` |
| Плоскость: выбор профиля | `crates/forward/src/kimi/selection.rs` | `9b61c443` |

## Открытые швы (выглядит подключённым, не работает)

1. ~~`km_ready` не обработан.~~ **Закрыт.** Путь продавца проходится целиком: device-код, поллинг
   под generation guard, `/me`, публикация в roster до завершения выплаты.
2. **Профиль в roster ещё не становится обслуживающей ёмкостью.** Loader, selector, refresh/quota
   client, pure attempt policy, FIFO и config уже есть в `crates/forward/src/kimi/`, но `server` их
   не загружает, а generation/stream/settlement orchestration ещё не соединена в живой handler.
   До закрытия обеих частей этого шва говорить «подписка в пуле» нельзя.

## Следующее действие

**Живой KIMI gateway в `crates/forward` + server composition:** загрузить last-good roster, создать
per-profile transport state, выполнить authenticated identity readiness и провести Anthropic
Messages request через selection/one-byte attempt policy. Успешный turn обязан пройти
reserve→delivering→terminal usage settlement→bounded FIFO; quota poll разрешён только после drain.
Пока этого нет, включённый и валидный config намеренно логируется как dormant и не считается
маршрутизируемой ёмкостью.

**Процессные заметки (обе уже стоили потерянного мёржа):**

1. Не редактировать worktree, пока в нём идёт `agent-merge.sh`: дерево меняется под гейтом, и он
   падает на несобранном срезе.
2. Новая зависимость крейта — сразу коммить `Cargo.lock`. Гейт идёт с `--locked` и отказывается
   работать с грязным деревом.
3. После мёржа worktree становится clean+merged и его сносит `DELETE_WORKTREE`. Коммить первый
   же файл сразу — иначе новая работа исчезнет вместе с деревом (уже случалось трижды).
4. Красная deployment-полоса бывает транзиентной. Прогон 2026-08-03 дал
   `typescript=0 rust=0 deployment=1 static=0`, но все её тесты
   (`lib`, `codex-homes-migrate`, `sccache-cargo`, `agent-worktree`, `delete-worktree-agent`,
   `next-cache`, `typescript-*`, `commerce-release-bundle`, `agent-merge.suite`) зелёные и по
   отдельности, и подряд. Полосы gate'а идут параллельно, а deployment-тесты создают временные
   git-репозитории — вероятна гонка за ресурсы. Прежде чем чинить, **воспроизведи**: один
   красный прогон при зелёном повторе — не повод править тесты.

## Очередь после этого

1. `crates/forward` + `crates/server`: живой generation/stream/settlement handler и readiness
   через `/me`. **Пробу нельзя вешать на `/v1/models`** — он негейтед и отвечает 200 на мёртвый ключ.
2. Затем roster reload,
   поллер `/usages` с дренажом turn-FIFO перед каждым observation и shutdown drain.
3. `tools/kimi_calibration/run_live.py` — dry-run по умолчанию, целочисленный бюджет, точная
   атрибуция по immutable request id.
4. Observability, blue-green, live-матрица.

## Заблокировано человеком

- **Живая подписка Kimi Code.** Без неё не снимаются 8 `unknown` из §6 манифеста: auth-заголовок
  Anthropic-маршрута, форма terminal usage, реальная инкрементальность SSE, единица `used`,
  различение 401/403, набор планов, поведение месячного потолка, платные tool/search-единицы.

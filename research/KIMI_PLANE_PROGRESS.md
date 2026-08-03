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
| Server: strict default-off env → typed plane config | `crates/server/src/config.rs` | `76893b5f` |
| Auth Bot: публикация roster | `crates/authbot/src/kimi_roster.rs` | `dc175204` |
| Auth Bot: обработчик `km_ready` (шов №1 закрыт) | `crates/authbot/src/bot.rs` | `391032bc` |
| Плоскость: загрузчик roster | `crates/forward/src/kimi/roster.rs` | `d8a37422` |
| Плоскость: классы ошибок + single-flight refresh | `crates/forward/src/kimi/transport.rs` | `9b61c443` |
| Плоскость: выбор профиля | `crates/forward/src/kimi/selection.rs` | `9b61c443` |
| Cleanup daemon: untouched managed worktree больше не считается merged | `deploy/agent-worktree.sh`, regression-suite, runbook | `05810766` |
| Плоскость: exact Messages gateway, `/me`, refresh/reseal, stream lifecycle и settlement | `crates/forward/src/kimi/gateway.rs`, server composition | текущий checkpoint |

## Открытые швы (выглядит подключённым, не работает)

1. ~~`km_ready` не обработан.~~ **Закрыт.** Путь продавца проходится целиком: device-код, поллинг
   под generation guard, `/me`, публикация в roster до завершения выплаты.
2. ~~Профиль в roster не становился обслуживающей ёмкостью.~~ **Закрыт на mock-гейтах.** Server
   загружает roster, `/me` подтверждает identity, exact Kimi aliases идут через selection,
   pre-byte policy, transparent non-stream/SSE и reserve→delivering→settlement→FIFO. Cold или
   повреждённый initial roster остаётся отдельным fail-closed Kimi path и не утекает в Claude.
3. **Roster пока является startup snapshot.** После публикации/замены credential работающий
   процесс не перечитывает roster; при ошибке последующего чтения ещё нет last-good reload,
   сохраняющего текущую ёмкость.
4. **Quota authority не подключена к runtime.** `/usages` parser и FIFO-ordering готовы, но нет
   poll loop, который сначала полностью доставляет turn evidence, затем пишет quota observation и
   обновляет estimator CAS. Поэтому пункт 4 терминального состояния ещё не выполнен.

## Следующее действие

**Last-good roster reload в `crates/forward` + server poller:** периодически полностью прочитать и
провалидировать roster, атомарно заменить runtime profile snapshot только после полного успеха,
сохранить существующую ёмкость при любой ошибке и authenticated `/me`-проверкой ввести новые
профили. Reload не должен обрывать in-flight lease или сбрасывать refresh single-flight.

**Процессные заметки (обе уже стоили потерянного мёржа):**

1. Не редактировать worktree, пока в нём идёт `agent-merge.sh`: дерево меняется под гейтом, и он
   падает на несобранном срезе.
2. Новая зависимость крейта — сразу коммить `Cargo.lock`. Гейт идёт с `--locked` и отказывается
   работать с грязным деревом.
3. Cleanup-демон исправлен в `05810766`: managed worktree, чей HEAD ещё равен creation base,
   классифицируется как unstarted и сохраняется; malformed metadata fail closed. Активный Kimi
   checkpoint дополнительно держится lifecycle lock. Быстрый первый коммит всё равно полезен как
   явная граница восстановления, но больше не является страховкой от ошибочного auto-delete.
4. Красная deployment-полоса бывает транзиентной. Прогон 2026-08-03 дал
   `typescript=0 rust=0 deployment=1 static=0`, но все её тесты
   (`lib`, `codex-homes-migrate`, `sccache-cargo`, `agent-worktree`, `delete-worktree-agent`,
   `next-cache`, `typescript-*`, `commerce-release-bundle`, `agent-merge.suite`) зелёные и по
   отдельности, и подряд. Полосы gate'а идут параллельно, а deployment-тесты создают временные
   git-репозитории — вероятна гонка за ресурсы. Прежде чем чинить, **воспроизведи**: один
   красный прогон при зелёном повторе — не повод править тесты.

## Очередь после этого

1. Last-good roster reload без потери in-flight state и с `/me` admission новых profiles.
2. Поллер `/usages` с полным дренажом turn-FIFO перед каждым observation, immutable quota write,
   estimator CAS, reset-aware health steering и shutdown drain.
3. Observability: bounded-cardinality metrics, alerts/runbook и admin-only operational evidence.
4. Blue-green/deploy wiring с default-off secret/config и rollback gate.
5. `tools/kimi_calibration/run_live.py` — dry-run по умолчанию, целочисленный бюджет, точная
   атрибуция по immutable request id.
6. Live-матрица на owned Kimi Code subscription; без неё generation не запускается.

## Заблокировано человеком

- **Живая подписка Kimi Code.** Без неё не снимаются 8 `unknown` из §6 манифеста: auth-заголовок
  Anthropic-маршрута, форма terminal usage, реальная инкрементальность SSE, единица `used`,
  различение 401/403, набор планов, поведение месячного потолка, платные tool/search-единицы.

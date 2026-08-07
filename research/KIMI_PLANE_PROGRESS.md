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
| Плоскость: exact Messages gateway, `/me`, refresh/reseal, stream lifecycle и settlement | `crates/forward/src/kimi/gateway.rs`, server composition | `137dec16` |
| Плоскость: last-good atomic roster reload + server discovery loop | `crates/forward/src/kimi/{gateway,roster}.rs`, `crates/server` | `31f27baa` |
| Плоскость: `/usages` → FIFO/spend → immutable observation/CAS → steering | `crates/forward/src/{billing,kimi/**}`, `crates/server` | `23e7baba` |
| Auth Bot: ввод прокси текстом на `km_proxy`, каноникализация до `km_ready`, карточка готовности на buyer/IPRoyal путях | `crates/authbot/src/bot.rs` | `3089ce72` |
| Observability: extended operational status, admin-only `GET /kimi-subs`, aggregate метрики, алерты + runbook + consistency-тест | `crates/{forward,server}`, `observability/**`, `deploy/**`, `docs/**` | `54db2afc` |
| Blue-green/default-off delivery plane: `ProviderMode::Kimi`, слоты 8804/8805 + origin 8803, capability markers, rollback-ветка, scrape `provider: kimi`, fail-closed `/v1/messages` | `crates/{forward,server}`, `systemd/claude-api-kimi{,@}.service`, `deploy/**`, `observability/**`, `docs/**` | `598b46a4` |
| Admin: KIMI capacity board в control-room подписок | `apps/admin/**`, `docs/product/ADMIN_PANEL.md` | `8797a7a8` |
| Pool parity: pre-byte rotation holes, pinned CLI UA, Retry-After cooling | `crates/forward/src/kimi/**`, `crates/kimi-credential` | `9ad22a58` |
| Точная атрибуция: admin calibration headers, pinned profile + immutable request id, bounded recent-turns read, поля `/kimi-subs` | `crates/{forward,server,registry}` | `fbc000fc` |
| Безопасный live-runner: `run_live.py` + 43 offline теста + ops runbook | `tools/kimi_calibration/`, `docs/ops/KIMI_CALIBRATION.md` | `a8b69c6d` |
| Pool parity 2: warm-home placement + early claim + cooling hints, per-model failure axis, barriered burst-доказательство | `crates/forward/src/kimi/**` | `beb98be4` |
| Включение плоскости: reviewed argv-пин `CLAUDE_API_KIMI_ENABLED=1` после live evidence 2026-08-04. Первый деплой `4efa3186` ушёл RED: unit-сandbox (`ReadOnlyPaths` на весь roster-каталог) не давал runtime перезапечатать конверт после обязательного refresh, и истёкший access token читался как auth-смерть. Фикс — read-only roster + writable `credentials/` | `systemd/claude-api-kimi{,@}.service`, `deploy/watchdog-lib.test.sh` | `4efa3186` + `0de59e28` |
| **всё вышеперечисленное в master, watchdog GREEN** | — | `0de59e28` |
| Прод-верификация плоскости после включения: слот `claude-api-kimi@8805` active, preflight `live_profiles=1`, ошибок reseal больше нет; origin 8803 `/ready`=true; `/kimi-subs` отдаёт `enabled:true`, 1 live-профиль, окна 5h 0/100 и weekly 98/100, `persistence_ok:true`, FIFO пуст; Prometheus `up{provider="kimi"}=1`, все 6 aggregate-серий на месте, 5 алертов `Kimi*` загружены; админка задеплоена из релиза `8797a7a8` (KIMI capacity board в bundle) | прод-хост (loopback checks) | `0de59e28` |
| Dry-run live-runner'а против прода: план из 12 legs (k3-256k low/high/max/off, k3 1m ×4 — требуют reviewed plan, kimi-for-coding ±highspeed high/off), guards на месте, `paid_requests:0`; при потолке $0.0001 ни один leg не проходит worst-case bound — runner останавливается до dispatch, как задумано | `tools/kimi_calibration/run_live.py` | `a8b69c6d` (прогон 2026-08-04) |

| Ревью готовности к проду: модели/wire/биллинг/пул/наблюдаемость против эталонов; исправлены два вывода прошлого аудита (429 и 403 у Kimi классифицированы верно) | `docs/audits/2026-08-07-kimi-production-readiness.md` | этот коммит |

| Live-runner: capability-probes на tools/media с отдельной авторизацией `--capability-probe-budget-usd`, запись «непроверено» вместо молчаливого пропуска, 6 новых офлайн-тестов | `tools/kimi_calibration/`, `docs/ops/KIMI_CALIBRATION.md` | этот коммит |

## Открытые швы (выглядит подключённым, не работает)

1. ~~`km_ready` не обработан.~~ **Закрыт.** Путь продавца проходится целиком: device-код, поллинг
   под generation guard, `/me`, публикация в roster до завершения выплаты.
2. ~~Профиль в roster не становился обслуживающей ёмкостью.~~ **Закрыт на mock-гейтах.** Server
   загружает roster, `/me` подтверждает identity, exact Kimi aliases идут через selection,
   pre-byte policy, transparent non-stream/SSE и reserve→delivering→settlement→FIFO. Cold или
   повреждённый initial roster остаётся отдельным fail-closed Kimi path и не утекает в Claude.
3. ~~Roster являлся startup snapshot.~~ **Закрыт на fault-гейтах.** Server каждые 15 секунд
   обнаруживает атомарную публикацию; gateway повторно использует exact runtime state неизменённых
   profiles, проверяет новые/изменённые через `/me`, публикует только целую generation и сохраняет
   last-good при read/decrypt/client/probe failure или исчезновении файла. Валидный пустой roster
   закрывает только новые admissions; in-flight lease продолжает жить.
4. ~~Quota authority не была подключена к runtime.~~ **Закрыто на mock + real-PG гейтах.** Первый
   `/usages` anchor выполняется после preflight, далее cadence независима от 15-секундного roster
   discovery. Pending FIFO блокирует сам HTTP; generation во время GET инвалидирует snapshot без
   customer semaphore; после ответа повторный drain предшествует exact spend read, immutable
   independent-window observations и CAS. Steering/full-reset публикуются только после durable
   успеха всех окон, а shutdown отменяет steady poll и повторяет порядок под общим deadline.

## Следующее действие

**Платный прогон live-матрицы runner'ом** (матрица теперь включает и capability-probes) на подключённой подписке Vivace — только после обоих
внешних условий: (1) ресет недельной квоты подписки (`resets_at` ≈ 2026-08-07, на подключении
израсходовано 98/100), (2) явное бюджетное разрешение человека с лимитом выше $0.0001 — сейчас
ни один leg не проходит worst-case full-context bound, и runner fail-closed останавливается до
dispatch (проверено dry-run против прода 2026-08-04: план из 12 legs, `paid_requests:0`).

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

**Уточнено ревью 2026-08-07** (`docs/audits/2026-08-07-kimi-production-readiness.md`): цепочка
построена, остались четыре разрыва между «плоскость обслуживает» и «продукт можно продавать».
Порядок ниже — по возрастанию стоимости, и первые два не требуют человека.

0. Счётчики запросов и вердиктов апстрима. `kimi/gateway.rs` не трогает `Metrics` ни разу: есть
   aggregate-гейджи и 5 алертов, но **долю ошибок посчитать нечем**. Это та же слепота, из-за
   которой разбор Gemini 2026-08-06 занял сутки. Закрывать до прихода клиентов, а не после.
0b. Окно `CLAUDE_API_SMOOTH_WAIT_MS`. Три готовые реализации (Claude/Gemini/Codex) — ждать только
   когда раунд не собрал вердикт провайдера. Сейчас при **1 доступном профиле из 3** пустой пул
   не гипотеза, а ожидаемое состояние на почти исчерпанной недельной квоте.
1. Live-матрица на owned Kimi Code subscription (подписка подключена 2026-08-04): incremental SSE,
   quota movement pairing, 401/403 classification и остальные unknown из §6 манифеста.

2. Авторитет цен. `SnapshotProvider` знает только `anthropic`/`openai`/`google`; KIMI резервирует
   легаси-путём (`reserve_request_for_execution`) и не резолвит release-v2 политику. Пока клиентов
   нет — безвредно; в момент публикации это ровно та стена, что дала 953 отказа 2026-08-06.
3. Публикация в каталог роутера — **последней**. Гейт `pricing-catalog-coverage` (2026-08-07)
   уронит сборку, если `kimi/*` появится в `routing-presets.json` без записи в каталоге цен.

## Заблокировано человеком

- ~~**Живая подписка Kimi Code.**~~ Подписка Vivace подключена через Auth Bot 2026-08-04;
  live smoke (`/me`, `/usages`, одна минимальная генерация с exact metering) прошёл.
- **Бюджет на платную калибровку.** При потолке $0.0001 ни один leg не проходит worst-case
  full-context bound — runner останавливается до dispatch. Нужно явное разрешение с бо́льшим
  лимитом. Недельная квота подписки на момент подключения исчерпана на 98/100.

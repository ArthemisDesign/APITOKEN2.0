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

| Паритет возможностей: сужен отказ `validate_priced_surface` до провайдер-исполняемой работы; client-side tools, `tool_use`/`tool_result` и inline-медиа разрешены (их уже ограничивает холд по длине тела) | `crates/forward/src/kimi/gateway.rs`, манифест | этот коммит |

| Live-runner: контракт `/kimi-subs` — читать `quota`, карантин из `cooling.auth_until`, fail-closed при отсутствии окон; фикстуры привязаны к `kimi_profile_value` | `tools/kimi_calibration/` | `5d4cce51` |
| Live-runner: атрибуция по провайдерскому имени + пересчёт денежных плеч под ожидаемым тарифом; фикстуры привязаны к `kimi_turn_value` | `tools/kimi_calibration/` | `30d2a9fa` |
| **Платный прогон базовой возможности выполнен** 2026-08-07: 2 leg'а, `complete: true`, $0.0002539 из $10 | `tools/kimi_calibration/run_live.py` | `30d2a9fa` |

| **Корень найден:** `KIMI_REVIEWED_PLANS` была пуста — любая подписка резалась до базовой модели нашим же `supports()`, отказ выходил как 429, неотличимый от апстрима. Внесены все 6 документированных тарифов, поиск регистронезависимый | `crates/kimi-credential`, манифест §1.1 | `00e7f016` |
| Видимость: `unreviewed_plan_profiles` в `/kimi-subs` и `/metrics` + алерт `KimiUnreviewedPlanProfiles` с рунбуком | `crates/{forward,server}`, `observability/**`, `docs/ops/MONITORING.md` | `00e7f016` |
| Smooth-окно ёмкости на общем `CLAUDE_API_SMOOTH_WAIT_MS`; ждём только раунд без вердикта провайдера | `crates/forward/src/kimi/gateway.rs`, `crates/server/src/config.rs` | `74b170c5` |
| Опровергнут реройт thinking-off → `kimi-k2.6`: `k3-256k` с `off` тарифицируется по карточке k3 (3000/15000). Алиас решает тариф, усилие — нет | `tools/kimi_calibration/` | `74b170c5` |
| **Доступ снаружи:** Kimi жила только на loopback-слоте 8805; публичный вход идёт на Anthropic-слоты, там плоскость не собиралась → 529. Собрана и там (embedded-режим) | `systemd/claude-api-anthropic@.service` | `8980549e` |
| **Прод-верификация по артефакту** 2026-08-07: `k3-256k`, `kimi-for-coding`, `kimi-for-coding-highspeed` через `https://api.apitoken.sale/v1/messages` → 200 | публичный вход | `8980549e` |

| **Все 5 алиасов подписки доказаны в проде** 2026-08-08 через `https://api.apitoken.sale/v1/messages`: `kimi-for-coding`, `kimi-for-coding-highspeed`, `k3-256k`, `k3`, `k3[1m]` → 200. Вектор usage полон; на уникальном промпте `input_tokens=100`, нули ранее — реальные попадания в кэш | публичный вход | `c5dddf6e` |
| Алиас и проводное имя разделены: `k3[1m]` уходит как `k3`. Прежде тело слалось дословно, провайдер такого id не знает, отказ выходил ёмкостным 429 | `crates/metering`, `crates/forward/src/kimi`, манифест | `c5dddf6e` |

## Аудит паритета с Gemini/Claude/GPT (2026-08-08, 5 осей из 8)

**Опасное — нет мягкой оси охлаждения и выхода из неё.** Любой 401/403 сажает профиль в жёсткий
`auth_quarantined_until` на 300 с (`AUTH_QUARANTINE_SECS`), и отбор не умеет ослабить его, даже
когда это последняя ёмкость. Gemini чинил это в августе (`mark_auth_blocked` +
`select_routed_ignoring_env_cooling`) после девяти обнулений пула за двое суток; у Codex есть
`admission_ignoring_soft_cooling` с двумя ступенями; у Claude дефекта не было. Для Kimi хуже:
провайдер отдаёт 401 в том числе на «возможность выше тарифа». **Это следующая правка.**

**Заметное.** Нет счётчиков запросов и вердиктов апстрима — долю ошибок и частоту пропавшего
usage посчитать нечем (есть только тег расчёта `kimi-terminal-usage-missing`). Нет lifecycle-полей
подписки в проекции, тогда как Gemini и Codex их публикуют.

**Паритет подтверждён.** SSE error-tail при обрыве; корректная обработка отсутствия терминального
usage (консервативный холд без неизменяемого события); аффинити с прогревом дома, ранним захватом
и подсказками охлаждения; ретраи только до первого байта; last-good ростер; порядок settle→quota;
smooth-окно.

**Оси 6–8 проверены 2026-08-08 — чисто.** Health-свип не конкурирует с ходами: `poll_profile_quota`
выходит немедленно при `inflight != 0`, а сам свип под мьютексом (та защита, отсутствие которой на
Codex глушило рабочие дома). Shutdown: финальная калибровка идёт после закрытия приёма и простоя
финализаторов, повторяет порядок turn→quota, ограничена общим дедлайном, незавершённый дренаж
логируется как ошибка. Границы: ответ читается с потолком 32 МБ, длина исходного тела участвует в
консервативном холде, `validate_priced_surface` фейлится закрыто на провайдер-исполняемой работе.

**Аудит закрыт: 8 осей из 8.** Найдено и исправлено два дефекта — отсутствие аварийного выхода из
средового охлаждения (`78fb6c48`) и полное отсутствие счётчиков запросов (`065d6ac7`). Остальные
шесть осей соответствовали Claude/Gemini/Codex изначально.

**Живые доказательства 2026-08-08.** Кэш и липкость подтверждены одним замером: повтор того же
промпта дал `input 58 → cache_read 58` на второй попытке, то есть 58 токенов переехали в чтение
кэша (тариф $0.19/M против $0.95/M промаха) и запрос сел на тот же профиль — иначе попадания бы не
было. Ничего не осталось неоплаченным.

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

**Сделано 2026-08-08 (решение владельца отменяет запрет 2026-08-07):** публикация в каталог
роутера. Kimi получила **четвёртую catalog-плоскость** поверх существующего Anthropic-лейна:
`Lane` — это протокол, и Kimi говорит на Anthropic Messages, поэтому нового лейна нет. Откуда
берётся список моделей — отдельный вопрос: `/v1/models` Anthropic-плоскости проксируется
байт-в-байт с `api.anthropic.com`, и дописать туда наши алиасы значило бы сломать прозрачность
для всех клиентов. Поэтому discovery вынесен во внутренний producer
`GET /internal/router/catalog/kimi` рядом с `/internal/router/catalog/pricing`. Рекламируются
только 5 подписочных алиасов; официальные Open Platform id — тарифные ключи, шлюз их не
принимает, и ни producer, ни прайсинг их не резолвят. `kimi-k2.6` не рекламируется вовсе.

**ОТКРЫТО И СРОЧНО: strict-ключи не могут пользоваться Kimi.** Шлюз отказывает strict-ключу
первой же проверкой в `handle()`, до любой попытки прайсинга (`kimi_strict_pricing_unavailable`),
а `router_pricing.rs` такому ключу `kimi/*` не котирует — поверхности согласованы, клиент видит
чистое отсутствие, а не загадочный 5xx.

**Это НЕ то же, что у Gemini** (в более раннем варианте этого журнала было написано неверно).
`reserve_gemini_billing` сначала пробует `reserve_gemini_release_v2` и strict-аккаунт им
обслуживает; strict-отказ там — только фоллбэк на случай, когда релиз не резолвится вообще. У Kimi
такого пути нет: `SnapshotProvider` знает только `anthropic`/`openai`/`google`.

Срочность даёт ретайрмент release-v2: `00348678` заводит новые аккаунты (B2C и invited B2B) сразу
strict, `5b1af567` бэкфиллит существующих под `PRICING_BACKFILL_ENABLED`. По мере прогона бэкфилла
Kimi перестаёт быть доступна практически всей базе. OpenKeys-ключи strict by construction — там
Kimi недоступна уже сейчас.

Чинить по образцу Gemini, а не воскрешая механику релизов: добавить `SnapshotProvider::Kimi` и
заменить безусловный отказ на резолюцию релиза с фоллбэком по `is_model_unpriced` на точный
легаси-тариф — тот самый путь, что ратифицирован вместе с генерацией 55 как последней.

**Живая матрица выполнена 2026-08-09.** 12/12 плеч `complete: true` на каждом из двух доступных
профилей Vivace (`kimi-1beecf16c84925f0`, `kimi-0b8722ae13e138c2`): 4 семьи моделей × оба
контекстных режима × все принимаемые reasoning_effort. Потрачено $0.0105 на профиль, $0.021
суммарно из выданных $10. Каждое денежное плечо пересчитано из точных счётчиков токенов; тарифные
пины — горячие override `moonshot/kimi/<модель>/v2`.

- **1M закрыт.** Стоял как «не проверено» из-за худшего случая ~$3.15 за плечо; четыре плеча
  `k3:1m` реально стоили $0.0049 вместе.
- **Цена покрытия**: 13 юнитов пятичасового окна и 3 недельного за полный прогон. Прогонять по
  профилям дёшево.
- **Третий профиль** (`kimi-3e80dc1f94df83de`, недельная 100/100) раннер отверг ДО отправки —
  `exact KIMI profile is cooling`, потрачено ноль. Записан как недоступный, а не пропущенный.
- **`kimi-k2.6` матрицей не покрыта**: до неё не доходит ни один опубликованный алиас. Это
  независимо подтверждает решение её не рекламировать.
- **Инструменты и медиа** остались `unavailable` со `skipped_before_dispatch: true` — стоимость
  единицы за запрос не доказана, отдельный `--capability-probe-budget-usd` не брался.
- Прогон нашёл дефект в самом раннере (`1bfadaa6`): он падал на первом плече, требуя точного
  совпадения строки тарифа с компилированным id, тогда как движок пинит горячий override. Цена
  при этом сходилась. Офлайн-фикстуры это поймать не могли — они видели только компилированный id.

**Поправка к цене матрицы (2026-08-09).** Ранее записано «недельная цена полного прогона — 3
юнита». Это неверно: сложены были только РАЗРЕШЁННЫЕ дельты, остальные плечи дали `None`. По
счётчикам профиля `kimi-1beecf16c84925f0` недельная выросла 53 → 78, пятичасовая 11 → 61, то есть
покрытие стоит около 25 недельных юнитов. При этом у `kimi-0b8722ae13e138c2` после такого же
полного прогона счётчики не сдвинулись вообще (89/100 и 0/100) — двенадцать оплаченных ходов,
точное списание, нулевое движение квоты. Это неатрибутированный случай из контракта калибровки, а
не ошибка учёта; фиксируем как есть.

**`kimi-k2.6` — тарифный ключ, а не адресуемая модель.** Она есть в компилированном каталоге
`metering` (нужна как ставка официального API для оценки замещающей стоимости), но подписочный
маршрут её не публикует и ни один алиас до неё не ведёт. Сторонние руководства утверждают, что
запрос с выключенным thinking «спускает» K3/K2.7 до K2.6 — на наших живых прогонах это
опровергнуто: плечи `k3:*:off` тарифицировались по карточке K3 (`moonshot/kimi/kimi-k3/v2`).
Подтвердить со стороны провайдера перечнем моделей нельзя: `GET /v1/models` подписочного маршрута
оказался ЗАКРЫТЫМ (на проверочный ключ вернул `invalid_authentication_error`), вопреки записи в
`crates/forward/src/kimi/transport.rs`, которая утверждала обратное — заметка исправлена.

**Третий профиль ждёт сброса квоты.** `kimi-3e80dc1f94df83de` выбрал недельное окно 100/100;
раннер отвергает его до отправки (`exact KIMI profile is cooling`). Команда готова, запускать
после сброса:

```bash
python3 tools/kimi_calibration/run_live.py --execute \
  --profile kimi-3e80dc1f94df83de \
  --production-capacity-over-ssh --production-api-over-ssh \
  --one-m-plans Vivace --budget-usd 10 \
  --report /tmp/kimi-matrix-p2.json
```

**Ёмкость.** Доступен 1 профиль из 3 (двое в quota-cooling, у живого 89/100 пятичасового окна и
82/100 недельного). Все три подписки — Vivace. Пока это единственное, что ограничивает Kimi в
проде; функционально плоскость работает.

**Не проверено:** миллионный контекст (`k3[1m]`). Плечо стоит ~$3.15 худшего случая; лестница его
разрешает (Allegretto+), но живого подтверждения нет — в отличие от `k3-256k` и highspeed, которые
подтверждены 200-ми через публичный вход.

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
   легаси-путём (`reserve_request_for_execution`) и не резолвит release-v2 политику. Поверхности
   согласованы (strict-ключ не видит Kimi нигде), но strict-ключ и не может ею пользоваться.
3. ~~Публикация в каталог роутера~~ — сделана 2026-08-08. Гейт `pricing-catalog-coverage`,
   которым эта задача была заблокирована, удалён на master в `b4a694c3`: его посылка умерла
   вместе с механикой релизов.
4. Фронт: доки, дашборд, OpenKeys, карточки usage — и разбивка по Kimi в «Расход клиентов».

## Заблокировано человеком

- ~~**Живая подписка Kimi Code.**~~ Подписка Vivace подключена через Auth Bot 2026-08-04;
  live smoke (`/me`, `/usages`, одна минимальная генерация с exact metering) прошёл.
- ~~**Бюджет на платную калибровку.**~~ Разрешение на $10 выдано 2026-08-07, прогон выполнен,
  израсходовано $0.0002539. Остаток бюджета не тронут: покрывать им нечего, пока план не отревьюен.
- **Ревью тарифного плана.** См. «Следующее действие»: без записи в `KIMI_REVIEWED_PLANS`
  обслуживается только базовая возможность, и 3 из 4 моделей недостижимы.
- (историческое) При потолке $0.0001 ни один leg не проходил worst-case
  full-context bound — runner останавливался до dispatch. Требовалось разрешение с бо́льшим
  лимитом. Недельная квота подписки на момент подключения исчерпана на 98/100.

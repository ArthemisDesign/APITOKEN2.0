# DEVBOT.md — дизайн dev-бота Telegram (`apps/devbot`)

Статус: **этапы 1–3 реализованы** (alert-webhook + команды, деплой-поллер, journald/silence —
приложение `apps/devbot`; systemd-юнит, watchdog-lane, Alertmanager-рендер и heartbeat-алерт —
деплой/мониторинг-контур). Остаётся **этап 4** — позитивные бизнес-события commerce; он требует
нового expand-only контракта от commerce и до его появления не начинается (см. «Открытые
вопросы»). Каждый источник событий ниже подтверждён файлом в
репозитории — если при реализации код разойдётся с этой картой, верен код, а документ
обновляется в том же коммите.

## 1. Цели и обзор

Сейчас оператор узнаёт о проблемах двумя способами: email от Alertmanager
(`observability/alertmanager/alertmanager.yml.template`, единственный receiver
`production-email`) и GitHub commit statuses (`deploy/watchdog` и фазовые контексты).
Позитивных событий нет вообще: успешный деплой, завершившаяся миграция и восстановление
после карантина никуда не уведомляются. Email медленный, GitHub-статусы нужно идти смотреть.

Цель: одна Telegram-группа с топиками (forum-группа), куда бот `apps/devbot` доставляет все
события жизненного цикла проекта — алерты, деплои, миграции, инциденты движка, результаты
валидаций — с разумной дедупликацией, и отвечает на команды о текущем состоянии систем.

Нецели:

- бот не заменяет Alertmanager и Prometheus rules — он потребитель их сигналов, а не новая
  система алертинга;
- бот не пишет в продакшн-системы (никаких действий вроде «перезапусти сервис» из чата на
  первых этапах); команды только читают состояние, единственное исключение — `/silence` в
  Alertmanager (этап 3);
- бот не дублирует runbook'и — он ссылается на них.

## 2. Карта источников событий

Три транспорта доставки событий в бота:

| Транспорт | Что несёт | Точка подключения |
|---|---|---|
| **Alertmanager webhook** | Все 43 alert-правила (26 critical, 17 warning) | новый `webhook_configs` receiver в `observability/alertmanager/alertmanager.yml.template` на loopback-порт бота; email-receiver сохраняется (expand-only) |
| **GitHub API поллер** | commit statuses `deploy/*` и Deployments `production-*` | тот же контракт, что у `deploy/agent-merge.sh` (`am_status`) и `deploy/watchdog-github.sh`; интервал 30–60 с |
| **Journald tail** (этап 3) | ручные вмешательства и откаты, не отражённые в GitHub | префиксы `[watchdog]`, `[agent-merge]`, `[admin-deploy]`, `[sales-deploy]`, `[openkeys-deploy]`; чтение через `journalctl -f` |

### 2.1 Деплой-конвейер (host-watchdog)

Источник истины — `deploy/watchdog.sh` (таймер `systemd/apitoken-deploy-watchdog.timer`,
полл `origin/master` каждые ~5 с). События:

| Событие | Сигнал | Где читать | Severity |
|---|---|---|---|
| Новый кандидат в master | `pending deploy/watchdog` + `pending deploy/tests` (`watchdog.sh:113-122`) | Commit Status API | info |
| Тестовая лейна упала (TypeScript/Rust/Codex/Static) | `failure deploy/tests`, SHA в карантине (`rejected.sha`, `watchdog.sh:279-295`) | Status API | critical |
| Миграция commerce/engine | `pending/success/failure deploy/migration` (`watchdog.sh:1992-2001`); отложенная — `pending-migration.sha` | Status API | high/critical |
| Компонентный роллаут | Deployments `production-{database,engine,backend,sales,openkeys,admin}` (`watchdog.sh:87-104`) | Deployments API | info/high |
| Откат health-gate | `health check FAILED … rolled back` vs «rollback target also unhealthy — manual intervention required» (`deploy/admin-deploy.sh:95-105`, аналоги в sales/openkeys) | journald | warning / **critical** |
| Карантин SHA | `failure deploy/watchdog` + алерт `DeployQuarantined` | Status API + Alertmanager | critical |
| Пайплайн зелёный | `success deploy/watchdog` «All selected production components verified» (`watchdog.sh:2300`) | Status API | info |
| Пайплайн завис | алерты `DeployPipelineStale`, `DeployStuckInPhase` | Alertmanager | high |
| Ручной retry / rollback | `apitoken-watchdog retry` (`deploy/watchdog-control.sh:48-57`), `deploy/rollback.sh` | journald | warning (вмешательство человека) |

### 2.2 Алерты Prometheus/Alertmanager

Полный инвентарь — 43 правила в двух файлах; каждое имеет runbook-анкор
`docs/ops/MONITORING.md#<alertname>` (согласованность гейтится
`deploy/monitoring-config.test.sh`).

`observability/prometheus/rules/application.yml` (25 правил):

- **Движок (Anthropic)**: `EngineCircuitBreakerOpen` (critical), `EngineHasNoSubscriptions`
  (critical), `EngineAllSubscriptionsCooling` (critical), `EngineUpstreamAuthFailures`
  (critical), `EngineUpstreamRateLimited`/`EngineUpstreamServerErrors`/
  `EngineAffinityRedisErrors` (warning).
- **Codex-провайдер**: `CodexProviderDown`, `CodexNoAvailableHomes`, `CodexHomeUnresponsive`,
  `CodexAccountDead` (critical); `CodexHomeUnauthenticated`, `CodexHomeNearRateLimit`,
  `CodexHomeRateLimited`, `CodexHomeSnapshotStale`, `CodexCalibrationPersistenceFailed`,
  `ConversationsForcedOffTheirHome` (warning).
- **Gemini-провайдер**: `GeminiProviderDown`, `GeminiNoAvailableProfiles` (critical);
  `GeminiProfileUnauthenticated`, `GeminiUpstreamRateLimited` (warning).
- **Деньги и durable-состояние**: `DurableQueueBacklog` (warning),
  `DurableQueueOldestItemStale`/`DurableQueueDeadItems` (critical), `FailedWebhooksPresent`
  (critical), `StaleCheckoutSessions` (warning), `EngineSettlementBacklog` (critical),
  `EngineExpiredLeasePresent` (critical), `BalanceDivergenceDetected` (critical),
  `BackupStale`/`BackupMissing` (critical), `DeployQuarantined`/`DeployPipelineStale`
  (critical), `DeployStuckInPhase` (warning), `DeployMigrationUncommitted` (critical),
  `SalesReferralReconciliationBacklog` (warning), `SalesPayoutBatchFailed` (critical),
  `CaddyUpstreamFiveXxRateHigh` (warning).

`observability/prometheus/rules/operations.yml` (17 правил): `MonitoringTargetDown`,
`PublicEndpointDown`, `ProxyUpstreamPairDown`, `BusinessCollectorStale`,
`BusinessCollectorMissing`, `SystemdCollectorFailed`, `HostClockSkew`, `HostDiskSpaceCritical`,
`PostgresUnavailable`, `ProjectSystemdUnitFailed`, `CriticalTimerFailed` (critical);
`CertificateExpiresSoon`, `JournalDeliveryFailing`, `HostDiskSpaceLow`, `HostInodesLow`,
`HostMemoryPressure`, `HostCpuSaturated`, `ServiceRestartLoop`, `PostgresConnectionsHigh`,
`PostgresDeadlocksDetected` (warning).

Все они уже структурированы (alertname, severity, component, summary, description,
runbook-анкор) и приходят в один webhook — бот не опрашивает Prometheus сам.

### 2.3 Рантайм-инциденты движка и коммерции

Почти всё покрыто алертами выше (не дублируем). Дополнительные сигналы, доступные командам
бота (не пуши):

- `GET /ready` движка по слотам с reason `draining`/`authority_unavailable`/
  `provider_unavailable` (`crates/server/src/http.rs:2999-3023`);
- `GET /settlement-health` — settlement outbox, failed за 24 ч, backlog
  (`crates/server/src/http.rs`, control-ключ);
- `GET /pool`, `/codex-subs`, `/gemini-subs` — состояние пула подписок (control/readonly
  ключи);
- `GET /health|/ready` коммерции (`apps/api/src/health.controller.ts` — БД + engine),
  sales-api (`apps/sales-api/src/health.controller.ts`), openkeys
  (`apps/openkeys/src/app/api/ready/route.ts`);
- структурированные `customer_http_error` события в journald
  (`crates/server/src/http.rs:399-458`) — этап 3, агрегированный дайджест, не пуш каждого
  события.

### 2.4 Бизнес-события (этап 4, требует новых источников)

Позитивных событий (успешная оплата, новый клиент, успешный деплой как бизнес-веха) в
текущем observability-контуре нет — `deploy/collect-monitoring-metrics.sh` снимает только
агрегаты сбоев. Источники-кандидаты: durable-очереди commerce БД (состояния
`pending/paid/canceled/refunded` вебхуков, `packages/payments/src/provider.ts`), payout-цикл
(`apps/sales-api/src/payout/payout.service.ts`). Это новая кросс-контекстная связь
(commerce → devbot), оформляется отдельно по правилам expand-only (см. «Открытые вопросы»).

## 3. Структура топиков группы

Группа — forum-группа (topics включены). Topic id выдаёт Telegram при создании топика; id
фиксируются в env-конфиге бота (см. раздел 7), не в коде.

| Топик | Что попадает | Политика |
|---|---|---|
| **🚨 Critical** | Все critical-алерты (firing + resolved), карантин SHA, падение миграции, «rollback target also unhealthy» | Немедленный пуш. Resolved — обязательное сообщение-закрытие |
| **🚀 Deploys** | Новый SHA, старт пайплайна, фазовые статусы, роллауты компонентов, зелёный финал, ручные retry/rollback, отложенные миграции | Пуш каждой вехи; фазы одного SHA сворачиваются в одно редактируемое сообщение |
| **⚠️ Warnings** | Все warning-алерты (firing + resolved) | Пуш со сворачиванием повторов (раздел 4); resolved — правка исходного сообщения |
| **💰 Commerce** | Денежные алерты (`FailedWebhooksPresent`, `StaleCheckoutSessions`, `SalesPayoutBatchFailed`, `DurableQueue*`, `BalanceDivergenceDetected`, `EngineSettlement*`) — дублируются из своего severity-топика; позитивные события (этап 4) | Дубль только заголовком + ссылкой-ответом на сообщение в Critical/Warnings |
| **📊 Digest** | Ежедневная сводка (см. `/digest`) | Одно сообщение в сутки |

Критерии severity: severity-лейбл правила (`critical`/`warning`) — из Alertmanager;
события деплоя классифицируются ботом по таблице раздела 2.1 (карантин и «manual
intervention required» = critical → дубль в 🚨 Critical).

Принцип «один взгляд»: в 🚨 Critical всегда видно всё критичное по всем доменам; доменные
топики дают контекст. Дублирование critical в 💰 Commerce сделано ответом (reply) на
исходное сообщение, а не копией текста, чтобы не плодить рассинхрон resolved-статусов.

## 4. Состав уведомлений

Формат: `parse_mode: HTML`, `disable_web_page_preview: true` — как в существующем клиенте
`crates/authbot/src/tg.rs`. Максимум 4096 символов на сообщение (лимит Bot API) — описание
алерта обрезается с эллипсисом, полная версия по ссылке на runbook.

### 4.1 Алерт (firing)

```
🔴 <b>EngineCircuitBreakerOpen</b> [critical · claude-engine]
<i>Аварийный размыкатель движка открыт ≥1 мин</i>
{annotations.description}
Runbook: {ссылка на docs/ops/MONITORING.md#enginecircuitbreakeropen в GitHub}
Started: {startsAt, local tz} · Fingerprint: {короткий hash}
```

Resolved: правка исходного сообщения (если оно в пределах 48 ч — лимит Bot API на edit) —
заголовок `🟢 RESOLVED` + длительность; иначе новое сообщение-ответ. Для 🚨 Critical
resolved идёт отдельным сообщением всегда (важно видеть закрытие в ленте).

### 4.2 Деплой-веха

Одно редактируемое сообщение на SHA в топике 🚀 Deploys. Пока деплой идёт:

```
🚀 <b>Deploy</b> <code>1bd14c3</code> — feat(registry): expand provider calibration…
👤 <b>3xcalibur @3xcalibur-tech</b> · <i>Started 13:44</i> · <a href="{commit url}">commit</a>
✅ tests · ✅ migration · 🔄 engine · ⏳ backend
⏳ sales · ⏳ openkeys · ⏳ admin · ⏳ devbot
```

👤 — автор коммита: git author name, плюс `@login`, когда email коммита привязан к
GitHub-аккаунту (для агентских адресов GitHub отдаёт `author: null` — остаётся git-имя).
Чеклист — две фиксированные строки 4+4, чтобы перенос не рвал фазы посередине. Для всех
восьми фаз authority — соответствующий commit status `deploy/*`: так зелёный skip/no-change
виден сразу. Deployment `production-*` используется только как ранний fallback, пока commit
status этой фазы ещё не опубликован, и не может откатить уже известный status назад в pending.

Каждый фазовый статус — правка этого сообщения (счётчик правок не ограничен). Финал по
зелёному `deploy/watchdog` — компактная сводка без промежуточных фаз:

```
✅ <b>Deployed</b> <code>1bd14c3</code> — feat(registry): expand provider calibration…
👤 <b>3xcalibur @3xcalibur-tech</b> · <i>done in 12m</i> · <a href="{commit url}">commit</a>
```

Состояние при этом остаётся правдивым: все фазы без явного failure помечаются success
(зелёный watchdog означает, что прошли все лейны, включая те, чей последний видимый статус
ещё 🔄/⏳). Карантин: заголовок `❌ <b>Deploy failed</b>`, чеклист СОХРАНЯЕТСЯ (это
диагностика) + строка упавшей фазы + отдельное сообщение в 🚨 Critical с автором и первыми
~500 символами причины (`wd_die`-строка из journald, этап 3; до этапа 3 — только фаза и
ссылка на статус).

Финал гарантирован даже когда HEAD уходит вперёд: agent-merge пушит следующий master сразу
после зелени предыдущего, поэтому `deploy/watchdog=success` предыдущего SHA почти всегда
ускользает от diff'а по HEAD. Поллер держит «хвост» — предыдущий SHA с нетерминальным
watchdog — и опрашивает его статусы параллельно с HEAD, доизлучая его phase/green/
quarantine; роутер хранит `previousDeploy`, и события хвоста правят сообщение ИМЕННО того
деплоя. Одного слота достаточно: следующий master не может быть запушен, пока предыдущий
не дойдёт до терминала (merge-lock + проверка зелени в agent-merge). Поздняя фаза для
завершённого деплоя игнорируется — финальная сводка не разворачивается обратно.

События одного snapshot обрабатываются строго последовательно и поллер ждёт весь пакет до
сохранения snapshot: `new-sha` сначала получает Telegram `message_id`, затем выполняются
фазовые правки, а terminal watchdog — последним. Это исключает потерю финала быстрого
no-change/docs-деплоя и перестановку конкурентных `editMessageText`.

Все операторские отметки времени и ежедневный дайджест используют IANA-зону
`DEVBOT_TIME_ZONE` (по умолчанию `Asia/Tbilisi`), независимо от timezone production-хоста.

### 4.3 Дедупликация и сворачивание

- Ключ дедупликации алерта: `fingerprint` из webhook Alertmanager. Повторный firing с тем
  же fingerprint (repeat_interval Alertmanager: 4 ч для warning, 1 ч для critical) — не
  новое сообщение, а правка существующего: суффикс `×N, last: {время}`.
- Буря алертов: если за 60 с приходит >5 различных critical — после пятого бот шлёт одно
  сводное «🔥 N active critical alerts: {список alertname}» и дальше сворачивает все в него
  до конца бури (10 мин без новых). Защита от лавины при каскадном падении
  (`MonitoringTargetDown` по многим job'ам).
- Warnings с частотой выше 1/10 мин по одному alertname — принудительное сворачивание с
  счётчиком, минимальный интервал правки 5 мин (Telegram лимитит частоту правок).

### 4.4 Троттлинг на стороне Telegram

- Исходящая очередь с лимитом 1 сообщение/с в группу и 20/мин суммарно (лимиты Bot API для
  групп); превышение — коалесцинг в сводку.
- Retry отправки: 429 от Telegram → honour `retry_after`; сетевые ошибки — экспоненциальный
  backoff до 5 попыток, дальше событие теряется с записью в лог (бот не должен падать
  из-за недоступности Telegram).
- Токен вырезается из всех строк ошибок перед логированием — практика `redact()` из
  `crates/authbot/src/tg.rs:75-77`, иначе токен утекает в journalctl через URL запроса.

## 5. Команды бота

Команды принимаются long polling'ом (`getUpdates`, webhook у бота явно удалён — как в
`crates/authbot/src/tg.rs:200-203`), обрабатываются только из разрешённого chat_id и только
от user id из allowlist (`DEVBOT_ADMIN_IDS`); чужие сообщения игнорируются молча.

| Команда | Ответ | Источники |
|---|---|---|
| `/status` | Сводка: фаза пайплайна (последний `deploy/watchdog` статус + SHA), активные алерты по severity, readiness всех плоскостей (engine слоты `/ready`, router, commerce `/health`, sales, openkeys, admin) | GitHub Status API, Alertmanager API `GET /api/v2/alerts`, HTTP-пробы loopback-эндпоинтов |
| `/alerts` | Список активных алертов: severity, alertname, возраст; группировка как в Alertmanager | Alertmanager API |
| `/deploys [N]` | Последние N (дефолт 5) SHA со статусами фаз и итогом | GitHub Deployments API + Status API |
| `/pool` | Пул подписок: живые/cooling/dead по провайдерам (Anthropic `/pool`, `/codex-subs`, `/gemini-subs`) | Control API движка readonly-ключом |
| `/settlement` | Денежная диагностика `/settlement-health`: outbox по состояниям, failed за 24 ч, backlog | Control API движка control-ключом |
| `/silence <alertname> <длительность>` (этап 3) | Создание silence в Alertmanager | Alertmanager API `POST /api/v2/silences` |
| `/digest` (и ежедневно в 10:00 `DEVBOT_TIME_ZONE`) | Сводка за 24 ч в топик 📊 Digest: деплои (успех/карантин), сработавшие алерты по count, топ повторяющихся warning | Журнал событий самого бота (state-файл) |
| `/help` | Список команд | — |

Ответы на команды идут в тот топик, где команда вызвана (`message_thread_id` из апдейта).

## 6. Архитектура `apps/devbot`

Новое приложение в pnpm workspace: `apps/devbot`. Node 24, TypeScript, без NestJS (нет DI-
графа, очередей и БД — plain service; зависимость только на встроенный `fetch`). Bot API
вызывается тонким клиентом по образцу `crates/authbot/src/tg.rs` — без telegram-фреймворков:
surface мал (send/edit/answer/getUpdates), а самописный клиент уже проверен в репо.

```
apps/devbot/src/
  main.ts            — wiring, config (zod, как apps/api/src/config.ts)
  tg.ts              — Bot API клиент: send/edit, thread ids, retry/429, redact токена
  am-webhook.ts      — HTTP-сервер 127.0.0.1:DEVBOT_PORT, приём webhook Alertmanager
  github-poller.ts   — поллер commit statuses/deployments (30–60 с), diff-логика вех,
                       tail-опрос предыдущего SHA до терминала deploy/watchdog
  journald.ts        — (этап 3) tail journalctl, парсеры префиксов
  router.ts          — маршрутизация событие → топик/форматтер
  dedup.ts           — fingerprint-store, сворачивание, шторм-коалесцинг
  commands.ts        — long polling, admin-гейт, команды
  state.ts           — JSON state-файл /var/lib/apitoken/devbot/state.json
```

Потоки событий:

1. **Alertmanager → webhook**: новый receiver в `alertmanager.yml.template`
   (`webhook_configs` на `127.0.0.1:DEVBOT_PORT/alerts/{secret}`), route продолжает
   вестись по существующему дереву — email получает всё, как сейчас; webhook получает те же
   группы (expand-only: email-ветка не меняется). Группировка и inhibit остаются на
   Alertmanager — бот получает уже сгруппированные нотификации.
2. **GitHub поллер**: читает statuses для `origin/master` HEAD и список deployments
   (`production-*`); diff против последнего известного состояния в
   state-файле → упорядоченный пакет вех деплоя, который роутер полностью обрабатывает до
   следующего snapshot. Токен — отдельный read-only PAT (см. раздел 7), не
   переиспользует `/etc/apitoken/github-watchdog.env` (root-only, чужой владелец).
3. **Journald** (этап 3): `journalctl -f -o json` с фильтрами по syslog-идентификаторам
   watchdog/deploy-скриптов; события «rolled back», «manual intervention», `retry`, запуск
   `rollback.sh`.
4. **Команды**: long polling → allowlist → обработчик → loopback-пробы и API.

Состояние: один JSON-файл (последний обработанный SHA и его фазовое сообщение,
fingerprint-store с TTL 48 ч, счётчики для дайджеста). БД не нужна; потеря state-файла =
переотправка текущего статуса, не катастрофа.

Границы (по `docs/DEPENDENCIES.md`): devbot — потребитель Alertmanager webhook-контракта,
GitHub API и публичных/loopback HTTP-эндпоинтов здоровья и Control API движка (только
readonly/control GET). В engine PostgreSQL, commerce/sales/openkeys БД бот НЕ ходит.
При реализации связи `alertmanager → devbot`, `github → devbot`, `engine Control API →
devbot` добавляются в `docs/DEPENDENCIES.md`, а `apps/devbot` — в карту `AGENTS.md` в том
же коммите.

## 7. Безопасность

- **Токен бота**: `DEVBOT_TELEGRAM_TOKEN`, отдельный бот от партнёрского
  (`TELEGRAM_BOT_TOKEN` в `apps/sales-api` занят login-виджетом, см.
  `apps/sales-api/src/telegram.ts`). Env-файл `/etc/apitoken/devbot.env`, mode 0600, владелец
  сервисного пользователя; в репо — только ключи в `.env.example`.
- **Redaction**: токен вырезается из ошибок/логов (patтерн из `crates/authbot/src/tg.rs`);
  в сообщения группы не попадают управляющие ключи, токены и внутренние account/key id
  (account id в алертных данных допустим, секретов там нет — формат алертов уже
  privacy-safe, см. тест `customer_error_event_is_structured_and_redacts_request_data` как
  образец требования).
- **Webhook Alertmanager**: bind только `127.0.0.1`; путь содержит 128-битный секрет
  (`DEVBOT_AM_SECRET`); чужие пути — 404. Наружу через Caddy не экспонируется.
- **Группа**: бот обрабатывает апдейты только с `DEVBOT_CHAT_ID`; команды — только от
  `DEVBOT_ADMIN_IDS` (user id, не username — username сменный). Бота нельзя добавить в
  другую группу с эффектом: чужой chat_id игнорируется молча.
- **GitHub**: fine-grained PAT, scope только read commit statuses/deployments/repository
  metadata; хранится в том же env-файле.
- **Временная зона**: `DEVBOT_TIME_ZONE` — валидная IANA-зона для времени в сообщениях и
  ежедневного дайджеста; default `Asia/Tbilisi`, timezone хоста на вывод не влияет.
- **Control API**: для `/pool`, `/settlement` — readonly/control ключи движка через env;
  используется только GET-эндпоинты.
- Исходящие соединения только к `api.telegram.org` и `api.github.com`; systemd-юнит с
  `NoNewPrivileges`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/apitoken/devbot` —
  по образцу существующих юнитов в `systemd/`.

## 8. Деплой и наблюдаемость самого бота

- Юнит `systemd/apitoken-devbot.service`: `Restart=always`, env из
  `/etc/apitoken/devbot.env`, `After=network-online.target`. Порт `DEVBOT_PORT=3800`
  (loopback; занятые порты: 8790–8799 плоскости движка и router, 3000/3001,
  3100, 3200, 3300, 3410, 3500, 3600, 3700 приложения, 9090–9187, 9115, 12345 стек
  мониторинга) — зафиксирован в `docs/DEPENDENCIES.md`. До provisioning
  `/etc/apitoken/devbot.env` юнит остается неактивным через
  `ConditionPathExists` (без crash-loop), а watchdog-lane завершается зелёным skip'ом, не
  двигая `devbot.sha` — первый реальный роллаут происходит автоматически после появления
  секретов.
- Деплой — отдельная lane host-watchdog по образцу `deploy/admin-deploy.sh`
  (single-instance, health-gate `GET /health` самого бота с откатом symlink): runner
  `deploy/devbot-deploy.sh`, релизный корень `/opt/apitoken/devbot-releases`, статус-контекст
  `deploy/devbot`, deployment-окружение `production-devbot`. Реализовано в
  `deploy/watchdog.sh` (классификатор `wd_path_is_devbot` — `deploy/watchdog-lib.sh`).
  Кандидат без TypeScript-лейны (deploy/observability/engine-only diff) не несёт собранный
  `apps/devbot/dist` — по построению классификатора его devbot-код идентичен запущенному
  релизу, поэтому lane завершается зелёным deferral'ом, НЕ двигая `devbot.sha`; реальный
  роллаут происходит на ближайшем master с TypeScript-лейной. Двигать baseline здесь нельзя:
  иначе после provisioning каждый TypeScript-less master уходил бы в карантин
  (`devbot-deploy.sh` падает на отсутствующем dist).
- Наблюдаемость бота:
  - падение юнита ловит существующий `ProjectSystemdUnitFailed` (паттерн `apitoken-*`);
  - heartbeat: каждые 60 с бот атомарно переписывает
    `/var/lib/apitoken/monitoring/textfile/devbot.prom`
    (`devbot_heartbeat_timestamp_seconds`); алерт `DevBotHeartbeatMissing`
    (warning) в `observability/prometheus/rules/application.yml` срабатывает, когда юнит
    активен, а heartbeat отсутствует или старше 300 с; runbook-секция —
    `docs/ops/MONITORING.md#devbotheartbeatmissing`, согласованность гейтит
    `deploy/monitoring-config.test.sh`. Файл публикуется с режимом 0644 независимо от
    `UMask=0077` юнита — node-exporter читает textfile от `nobody`; каталог держат
    group-deploy writable (`root:deploy 0775`) и `install-monitoring.sh`, и рутовый
    коллектор `collect-monitoring-metrics.sh` (он пересоздаёт каталог каждую минуту —
    откат ownership ломает heartbeat с EACCES);
  - деградация Telegram API видна по логу ошибок отправки (journald) — без отдельного
    алерта на этапе 1.
- Канал-последней-инстанции: если бот мёртв, email-receiver Alertmanager продолжает
  работать — поэтому email-ветка в конфиге Alertmanager не удаляется никогда (expand-only).

## 9. План внедрения

Каждый этап — отдельный мёрдж через `deploy/agent-merge.sh`; этапы независимы по ценности
(после этапа 1 бот уже полезен).

**Этап 1 — алерты (MVP).** `apps/devbot`: tg-клиент, am-webhook, router, dedup, команды
`/status`, `/alerts`, `/help`. Конфиг Alertmanager: webhook-receiver рядом с email
(expand-only), `deploy/render-alertmanager.mjs` + `install-monitoring.sh` — рендер нового
env (`DEVBOT_AM_SECRET`). systemd-юнит, `.env.example`, watchdog-lane, алерт
`DevBotHeartbeatMissing` + runbook + monitoring-config.test. Доки: этот файл (статус →
реализовано частично), `docs/README.md`, `docs/DEPENDENCIES.md`, `AGENTS.md` (карта).

**Этап 2 — деплои.** github-poller, топик 🚀 Deploys, команда
`/deploys`, сворачиваемые фазовые сообщения, дубль карантина в 🚨 Critical.

**Этап 3 — journald и silence.** journald.ts (откаты, retry, rollback.sh, agent-merge
события), команда `/silence`, дайджест `customer_http_error` по reason'ам.

**Этап 4 — бизнес-события.** Позитивные события commerce (оплаты, регистрации, payout
батчи) — требует expand-only контракта от commerce (webhook/outbox); оформляется
производитель-первым по правилам `AGENTS.md`, связь добавляется в `docs/DEPENDENCIES.md`.
Ежедневный `/digest` по расписанию.

## Открытые вопросы

- **Позитивные бизнес-события** (этап 4): готового транспорта нет — нужен новый контракт от
  commerce (webhook на бота или чтение durable-очередей). До решения этап 4 не начинать.
- **Дублирование critical в 💰 Commerce**: если на практике окажется шумным — свернуть до
  одного топика 🚨 Critical; решение по факту эксплуатации, структуру топиков менять дёшево
  (env-конфиг).
- **Логовые алерты из Loki**: правил по логам сейчас нет (кроме `JournalDeliveryFailing`);
  потенциальный источник для warning-топика через Loki ruler — не задействован, требует
  отдельного дизайна.
- **Vercel-деплои `apps/web`**: статусы постит Vercel вне этого репо
  (`deploy/agent-merge.sh:293-298` не доверяет combined status). Читать Vercel Deployments
  API возможно, но это новый внешний контракт — не включён в этапы 1–3.

# PANEL.md — единая админ-панель admin.apitoken.sale

`admin.apitoken.sale` — единый центр управления для владельца: commerce-пользователи и деньги,
engine-аккаунты и ёмкость, партнёрские аккаунты, CRM service account, безопасность, B2B и аудит.
Легаси `panel.apitoken.sale` удалён после переноса всех его функций сюда.

## Архитектура

```
браузер ──Basic credentials──▶ Caddy forward_auth ──▶ commerce admin identity store
                          │
                          ▼
                       Caddy (admin.apitoken.sale)
                          ├─ /                         → engine GET /admin-panel
                          ├─ /admin-panel.js          → engine GET /admin-panel.js
                           ├─ /overview /capacity
                           │  /metrics /subs
                           │  /fleet-history
                           │  /settlement-health       → engine balancer :8790 (+ control key)
                           ├─ /codex-subs              → OpenAI origin :8792 (+ control key)
                           ├─ /gemini-subs             → Gemini origin :8794 (+ control key)
                          ├─ /admin/*                 → commerce balancer :8791 /v1/admin/*
                          │                              (+ commerce admin key + actor)
                          ├─ /openkeys-admin/*        → OpenKeys :3410 /api/internal/admin/*
                          │                              (+ server-side control key + actor)
                          └─ /partner-admin/*         → sales-api :3100 /v1/admin/*
                                                         (+ sales admin key)
```

- HTML и JavaScript живут только в `crates/server/src/admin-panel.html` и
  `crates/server/src/admin-panel.js`; оба вкомпилированы в engine как `GET /admin-panel` и
  `GET /admin-panel.js`. Статических копий и второго panel HTML нет. JavaScript загружается как
  same-origin asset, чтобы CSP не зависела от ручного SHA-256 после каждого изменения панели.
- Caddy `forward_auth` — единый human gate. Commerce PostgreSQL хранит только password hashes,
  статус и grants для четырёх управляемых доменов. Control, commerce-admin и sales-admin ключи
  инжектятся только server-side и никогда не попадают в HTML, browser storage, ответы или логи.
- Проверенная identity передаётся commerce как `x-admin-actor` и `x-admin-account-id` через
  `forward_auth copy_headers`. Global directive order ставит anti-spoof `request_header` clear перед
  authentication, поэтому Caddy сначала удаляет клиентские подделки, затем устанавливает
  проверенную identity, а auth остаётся перед terminal `handle` routes. Downstream proxy сохраняет
  эти headers без override, чтобы аудит различал операторов и self-service password rotation.
  Internal auth API закрыт на публичном `backend.apitoken.sale` и доступен Caddy только через
  loopback.
- Внешние vhost и приложения видят только стабильные origins `127.0.0.1:8790` (Anthropic/control),
  `127.0.0.1:8791` (commerce) и `127.0.0.1:8792` (OpenAI). Только первые два Caddy-balancer знают
  blue-green slot-порты; обычный
  application `503` не исключает живой slot, депулинг выполняется active `/ready` checks.
  Однорелизный OpenAI bridge на 8792 описан в `deploy/CADDY.md` и не меняет admin routing.
- Engine-данные (`/overview`, `/capacity`, `/subs`, `/metrics`) определены в
  `crates/server/src/http.rs`. `/overview` содержит полный список engine accounts без API-ключей.
  `/fleet-history` отдаёт историю metrics.db (минутные снапшоты флота за 90 дней) окнами
  24h/7d/30d/90d с бакетированием до ≤ ~500 точек, опционально per-sub ряд по маске email.
  `/spend-stats` помимо accounts/providers отдаёт `models[]` — top-20 моделей по charge за
  каждое окно (served model id из usage_events, тот, что реально тарифицирован). Опциональные
  query `from`/`to` (epoch-секунды, обязательны вместе) добавляют к ответу блок `custom` той же
  формы, что окно `periods`, плюс эхо границ: та же usage_events-агрегация за полуоткрытый
  диапазон [from, to) — стыкующиеся диапазоны не задваивают события. Ширина ≤ 92 дней, `to` в
  будущем зажимается до now+1, диапазон целиком в будущем/from ≥ to/мусор — 400 с текстом причины.
  `custom` считается на каждый запрос и не попадает в TTL-кэш (в кэше только стандартные окна),
  поэтому ответы с разными границами не смешиваются.
  `/settlement-health` — денежная диагностика settlement pipeline: counts settlement_outbox по
  state (pending/processing/done/failed), failed всего и за 24ч, backlog несеттленых старше
  5 минут, последние ≤10 failed с last_error (обрезан до 200 символов, секретов в нём нет) и
  лаг pricing-консьюмера ledger'а (max(ledger.id) против ledger_consumer_checkpoints + возраст
  старейшей неподтверждённой строки). Растущий backlog/failed/unacked — сигнал «тихо застрявших»
  денег, раньше видимый только в stderr.
  `/codex-subs` (per-home статус GPT/Codex-флота) отдаёт только OpenAI-runtime — на Anthropic-
  процессе codex не настроен и endpoint вернул бы `enabled:false`, поэтому Caddy шлёт этот путь
  в стабильный OpenAI origin, а не в engine balancer. `/gemini-subs` аналогично читается только со
  стабильного Gemini origin `127.0.0.1:8794`; ответ содержит opaque profile/model quota/cooling,
  cache-affinity counters и отдельные attested gaxios/Undici CLI/Node/JA3/JA4, но не Google
  identity/project/proxy/OAuth.
- Commerce-данные находятся в `apps/api` за `AdminGuard`; authoritative live balance по-прежнему
  живёт только в engine.
- Partner-данные читаются через sales admin API. Main admin получает только server-side proxy;
  отдельная полная партнёрская админка остаётся тем же sales-web `/admin` на
  `admin.partners.apitoken.sale`.
- OpenKeys остаётся отдельным bounded context со своей PostgreSQL. Единая панель читает его
  маскированный каталог через `/openkeys-admin/*`: Caddy пропускает маршрут только после
  managed-admin auth, инжектит проверенного actor и server-side credential. Публичный
  `openkeys.apitoken.sale/api/internal/*` всегда возвращает `404`; полные `sk-pool` и складской
  шифротекст во внутренний контракт не входят.
- CRM-код находится в отдельном репозитории. Main admin показывает его engine service account
  с handle `crm-parsing` и ссылку на `crm.apitoken.sale`.
- Независимые read-источники деградируют отдельно: ошибка одного API не заменяет всю страницу.
  Панель показывает закрываемое уведомление, проверяет источник каждые 5 секунд и перезагружает
  страницу после полного восстановления. Переключение вкладки отменяет устаревшие запросы.
- Автообновление включено только для live-страниц: сводка — 30 секунд, система и подписки —
  10 секунд; в фоновой вкладке polling приостанавливается. Пользователи и partner accounts
  загружаются страницами, а live engine balances для commerce page читаются одним batch-запросом.
- Caddy сжимает ответы `zstd`/`gzip` и задаёт CSP, Permissions Policy и запрет индексации.
  CSP разрешает скрипты только с того же origin (`script-src 'self'`); inline JavaScript в HTML
  панели запрещён и проверяется тестом.

## Возможности

- Сводка всех контуров: commerce, engine, partners и CRM. Партнёрская карточка показывает и
  деньги контура: комиссии, сумму к выплате (requested + approved) и выплачено.
- Аккаунты: все engine/service accounts, все partner accounts (с полной пагинацией источника),
  commerce total и переход к полному пользовательскому workflow.
- Партнёры: read-only вид sales-контура — overview (партнёры/рефералы/оборот, комиссии, к выплате,
  выплачено), состояние окна выплат payout-движка, авто-список «к выплате за период» с
  eligible-причинами (ниже минимума, нет кошелька, неактивен, окно закрыто) и суммой eligible,
  аналитика партнёров с серверной сортировкой (12 полей) и пагинацией, история выплат и
  сворачиваемый список on-chain батчей. Источники — `/partner-admin/*`: `overview`,
  `payouts/engine`, `payout-list`, `partner-analytics`, `payouts`, `payouts/batches`; каждый
  деградирует отдельным warn-блоком. Суммы — nanoUSD-строки sales-api. Без автообновления.
- Администраторы: создание identity с одним или несколькими domain grants, точный фильтр по домену,
  password rotation любого account (включая текущий), enable/disable и защита последнего active
  main-admin от lockout.
- Пользователи: серверный поиск/фильтры и bounded pagination, серверная сортировка
  (`sort` ∈ created_at/last_seen_at/paid_total/topup_total/spent_30d, `dir` ∈ asc/desc; live-поля
  движка balance/spent не сортируются), live баланс/расход, платежи, ключи,
  тир, 2FA, начисление баланса, revoke всех сессий, сброс 2FA, enable/disable. Карточки
  «баланс/расход видимых» суммируют только показанную страницу и рядом подсказывают
  платформенные итоги из `/overview` (demand.balance_usd/spent_usd). Кнопка «CSV» выгружает
  текущую загруженную страницу (`users-YYYY-MM-DD.csv`).
- OpenKeys: отдельный список выпущенных ключей с обязательным отображением метки/партии/продавца,
  серверными фильтрами по партии, статусу и использованию (`unused`, `used`, `exhausted`, no-live),
  bounded pagination и обратимым enable/disable. Live-балансы читаются batch-запросами движка,
  а не N+1 по каждому ключу.
- Деньги: подтверждённые платежи, состояние engine credit, незавершённые checkout; серверные
  фильтры `GET /admin/topups` — `q` (подстрока email), `provider`, `status` (один фильтр на оба
  списка), пагинация `limit`/`offset` с `payments_total`/`checkouts_total` (один «Назад/Дальше»
  листает оба списка). Кнопка «CSV» выгружает текущую страницу обоих списков одним файлом
  (`topups-YYYY-MM-DD.csv`) с колонкой kind=payment|checkout.
- Финансы: prepay-метрики одним экраном — выручка 30 дней с дельтой к предыдущим 30, ARPU/ARPPU,
  доля платящих и распределение клиентов по тирам; SVG-график выручки по дням (окна 7/30/90) с
  разбивкой по провайдерам; воронка чекаутов (создано → оплачено/отменено/ошибка/истекло) с
  конверсией, средним временем до оплаты и средним чеком; топ клиентов по пополнениям и по
  расходу с долей от суммы окна; возвраты и диспуты с пагинацией; недельные когорты регистраций
  и сигналы оттока плативших клиентов. Источники — read-only эндпоинты commerce за AdminGuard:
  `GET /admin/finance/{overview,revenue,funnel,top-customers,cohorts,churn-signals}` и
  `GET /admin/refunds`. Суммы — integer nanoUSD-строки, агрегация на стороне PostgreSQL.
  Авторитет статуса возврата — `payments.status`; engine_adjustments (дебет движка по возврату)
  пока наполняется не полностью. Без автообновления. Внизу вкладки — здоровье денежных
  пайплайнов: вердикт, карточки и последние сбои `GET /admin/pipeline-health` (кредиты движка,
  вебхуки, почта, pricing-джобы) плюс settlement движка из `GET /settlement-health` (outbox
  pending/backlog/failed, лаг pricing-consumer — отставание передачи расхода в коммерцию);
  при verdict≠ok или settlement failed/backlog сводка показывает warn/bad баннер со ссылкой
  на эту вкладку. Модалка «Кто тратит» (/spend-stats) показывает и таблицу «по моделям» —
  top-20 served models активного окна со списанием, real-API эквивалентом и скидкой. Рядом с
  вкладками d1/d7/d30 — произвольный диапазон дат «с/по» («по» включительно, на сервер уходит
  полуоткрытая граница +1 сутки): рендерится блок `custom` с теми же подтаблицами
  accounts/providers/models, а 400 валидации показывается текстом в модалке.
- Pipeline health: `GET /admin/pipeline-health` за AdminGuard — read-only сводка сбоев денежных
  пайплайнов (engine_credits/webhook_events/email_outbox/engine_pricing_jobs: counts по
  статусам, dead, retry-backlog, последние сбои без payload, nano-сумма зависших кредитов) с
  общим вердиктом ok/warn/bad; суммы — integer nanoUSD-строки.
- B2B: одноразовые invite-ссылки с индивидуальной скидкой. Email необязателен: с email ссылка
  привязана к адресу и письмо атомарно ставится в durable outbox; без email панель создаёт
  shareable link и сразу копирует его. Активный инвайт можно повторно скопировать, отозвать или
  заменить новой ссылкой и отправить заново. Список показывает delivery status/error, а B2B-
  клиенты — pending/retry/failed/confirmed синхронизацию цены с engine.
- Подписки: отдельная страница по трём флотам. Claude — lifecycle (added/peaks/дни до замены),
  live util/reset/cooling по окнам 5h/7d и прокси; GPT (OpenAI Codex) — per-home статус, окна
  primary/secondary, лимиты и official-price spend; Gemini — per-profile auth/inflight,
  per-model availability/cooling, официальный quota remaining/reset/type, probe freshness,
  missing-usage settlement counter и точные gaxios/Undici transport attestations.
- Система: verdict, 1d/5h/7d supply, headroom, coverage, fleet demand, рекомендации и все
  engine accounts; детальный per-sub вид вынесен в «Подписки».
- Тренды: история флота из metrics.db (окна 24ч/7д/30д/90д) — SVG-графики доступной ёмкости,
  утилизации, дефицита подписок (gap/subs_needed) и баланса клиентов с потенциальным спросом;
  per-sub ряд cap/util по маске email показывает деградацию ёмкости подписки. Без автообновления —
  только ручной refresh и смена окна.
- Аудит: operator/user/provider события и причины административных действий; серверные фильтры
  `GET /admin/audit` — `action`, `actor_type`, `q` (подстрока по target_id и metadata::text),
  `from`/`to` (ISO 8601), пагинация `limit`/`offset` с `total`; `GET /admin/audit/actions` —
  distinct-список action для выпадайки (панель тянет его лениво один раз). Кнопка «CSV»
  выгружает текущую страницу (`audit-YYYY-MM-DD.csv`).

CSV-выгрузки панели («Пользователи», «Пополнения», «Аудит») — всегда текущая загруженная страница,
без авто-дозагрузки остальных: разделитель `;`, экранирование кавычек/разделителей/переводов строк
по RFC 4180, BOM в начале файла для корректного UTF-8 в Excel, деньги — сырыми числами USD,
даты — ISO 8601.

Ручное начисление принимает целые USD, UUID idempotency key и обязательную причину. Положительное
зачисление идемпотентно; подарок не считается платным top-up. Отключение пользователя сначала
блокирует authoritative engine account, затем commerce mapping и сессии.

Создание B2B-инвайта также принимает UUID idempotency key и причину. Допустимы только целая скидка
0–95% и срок 1–30 дней. Повтор с тем же ключом возвращает исходную ссылку, а не создаёт вторую.
Перевод существующего B2C-клиента в B2B принимает договорную скидку в том же атомарном действии;
actor, причина, старая и новая ставка записываются в аудит.

## Домены

- `admin.apitoken.sale` — единая главная админка.
- `admin.partners.apitoken.sale` — прежнее содержимое partner admin, изменён только hostname.
- `partners.apitoken.sale` — публичный партнёрский сайт, не менялся.
- `crm.apitoken.sale` — прежнее содержимое CRM, изменён только hostname; доступ выдаётся отдельным
  domain grant и не появляется у main-admin identity автоматически.
- `panel.apitoken.sale`, `partners.panel.apitoken.sale`, `crm.panel.apitoken.sale` — удалены без
  redirect и должны считаться ошибкой production verification, если снова начнут обслуживаться.

## Как добавить источник данных

1. Engine: новый endpoint за `control_authed`/`readonly_authed`, затем разрешить путь в
   `@admin_data` Caddy.
2. Commerce: endpoint за `AdminGuard`; `/admin/*` уже проксируется в `/v1/admin/*`.
3. Sales: endpoint за `AdminKeyGuard`; main admin использует `/partner-admin/*`.
4. OpenKeys: internal endpoint проверяет credential, инжектированный Caddy после human gate;
   публичный OpenKeys vhost обязан блокировать `/api/internal/*`.
5. UI: вкладка/ветка `refresh()` в `admin-panel.html`. Частичный источник должен показывать
   degraded state, а не просить секрет у оператора.
6. Деплой: обычный push в master. Watchdog применяет Caddy, проверяет marker HTML, 401 human-auth
   gate на четырёх active managed hosts и отсутствие трёх retired hosts.

## Секреты

| Секрет | Где живёт | Кто проверяет |
|---|---|---|
| admin password hashes + domain grants | commerce PostgreSQL | `apps/api` internal auth |
| engine control key | live Caddy + engine env | `control_authed` |
| OpenKeys internal credential (тот же engine control key) | live Caddy + `openkeys.env` | OpenKeys internal route |
| `COMMERCIAL_ADMIN_KEY` | live Caddy + commerce env | `AdminGuard` |
| `SALES_ADMIN_KEY` | live Caddy + sales env | `AdminKeyGuard` |

`deploy/render-caddy.awk` переносит service keys из live Caddy по placeholder-ам. Значения никогда
не добавляются в репозиторий. Human admins создаются и изменяются во вкладке «Админы»; новые и
rotated passwords хешируются Argon2id. Одноразовый cutover importer переносит старые Caddy bcrypt
rows до reload и abort-ит cutover, если main-admin или CRM access не сохранён.

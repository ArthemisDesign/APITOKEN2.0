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
                           ├─ /overview /capacity
                           │  /metrics /subs           → engine balancer :8790 (+ control key)
                           ├─ /codex-subs              → OpenAI origin :8792 (+ control key)
                          ├─ /admin/*                 → commerce balancer :8791 /v1/admin/*
                          │                              (+ commerce admin key + actor)
                          └─ /partner-admin/*         → sales-api :3100 /v1/admin/*
                                                         (+ sales admin key)
```

- HTML живёт только в `crates/server/src/admin-panel.html` и вкомпилирован в engine как
  `GET /admin-panel`. Статических копий и второго panel HTML нет.
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
  `/codex-subs` (per-home статус GPT/Codex-флота) отдаёт только OpenAI-runtime — на Anthropic-
  процессе codex не настроен и endpoint вернул бы `enabled:false`, поэтому Caddy шлёт этот путь
  в стабильный OpenAI origin, а не в engine balancer.
- Commerce-данные находятся в `apps/api` за `AdminGuard`; authoritative live balance по-прежнему
  живёт только в engine.
- Partner-данные читаются через sales admin API. Main admin получает только server-side proxy;
  отдельная полная партнёрская админка остаётся тем же sales-web `/admin` на
  `admin.partners.apitoken.sale`.
- CRM-код находится в отдельном репозитории. Main admin показывает его engine service account
  с handle `crm-parsing` и ссылку на `crm.apitoken.sale`.
- Независимые read-источники деградируют отдельно: ошибка одного API не заменяет всю страницу.
  Панель показывает закрываемое уведомление, проверяет источник каждые 5 секунд и перезагружает
  страницу после полного восстановления. Переключение вкладки отменяет устаревшие запросы.
- Автообновление включено только для live-страниц: сводка — 30 секунд, система и подписки —
  10 секунд; в фоновой вкладке polling приостанавливается. Пользователи и partner accounts
  загружаются страницами, а live engine balances для commerce page читаются одним batch-запросом.
- Caddy сжимает ответы `zstd`/`gzip` и задаёт CSP, Permissions Policy и запрет индексации.
  Inline-script hash проверяется тестом и должен обновляться вместе с HTML.

## Возможности

- Сводка всех контуров: commerce, engine, partners и CRM.
- Аккаунты: все engine/service accounts, все partner accounts (с полной пагинацией источника),
  commerce total и переход к полному пользовательскому workflow.
- Администраторы: создание identity с одним или несколькими domain grants, точный фильтр по домену,
  password rotation любого account (включая текущий), enable/disable и защита последнего active
  main-admin от lockout.
- Пользователи: серверный поиск/фильтры и bounded pagination, live баланс/расход, платежи, ключи,
  тир, 2FA, начисление баланса, revoke всех сессий, сброс 2FA, enable/disable.
- Деньги: подтверждённые платежи, состояние engine credit, незавершённые checkout.
- B2B: одноразовые invite-ссылки с индивидуальной скидкой. Email необязателен: с email ссылка
  привязана к адресу и письмо атомарно ставится в durable outbox; без email панель создаёт
  shareable link и сразу копирует его. Активный инвайт можно повторно скопировать, отозвать или
  заменить новой ссылкой и отправить заново. Список показывает delivery status/error, а B2B-
  клиенты — pending/retry/failed/confirmed синхронизацию цены с engine.
- Подписки: отдельная страница по обоим флотам. Claude — lifecycle (added/peaks/дни до замены),
  live util/reset/cooling по окнам 5h/7d и прокси; GPT (OpenAI Codex) — per-home статус, окна
  primary/secondary, лимиты и official-price spend. Gemini пока не выводится.
- Система: verdict, 1d/5h/7d supply, headroom, coverage, fleet demand, рекомендации и все
  engine accounts; детальный per-sub вид вынесен в «Подписки».
- Аудит: operator/user/provider события и причины административных действий.

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
4. UI: вкладка/ветка `refresh()` в `admin-panel.html`. Частичный источник должен показывать
   degraded state, а не просить секрет у оператора.
5. Деплой: обычный push в master. Watchdog применяет Caddy, проверяет marker HTML, 401 human-auth
   gate на четырёх active managed hosts и отсутствие трёх retired hosts.

## Секреты

| Секрет | Где живёт | Кто проверяет |
|---|---|---|
| admin password hashes + domain grants | commerce PostgreSQL | `apps/api` internal auth |
| engine control key | live Caddy + engine env | `control_authed` |
| `COMMERCIAL_ADMIN_KEY` | live Caddy + commerce env | `AdminGuard` |
| `SALES_ADMIN_KEY` | live Caddy + sales env | `AdminKeyGuard` |

`deploy/render-caddy.awk` переносит service keys из live Caddy по placeholder-ам. Значения никогда
не добавляются в репозиторий. Human admins создаются и изменяются во вкладке «Админы»; новые и
rotated passwords хешируются Argon2id. Одноразовый cutover importer переносит старые Caddy bcrypt
rows до reload и abort-ит cutover, если main-admin или CRM access не сохранён.

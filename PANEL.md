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
- Проверенная identity передаётся commerce как `x-admin-actor` и `x-admin-account-id`, чтобы аудит
  различал операторов и self-service password rotation. Internal auth API закрыт на публичном
  `backend.apitoken.sale` и доступен Caddy только через loopback.
- Внешние vhost и приложения видят только стабильные origins `127.0.0.1:8790` (engine) и
  `127.0.0.1:8791` (commerce). Только эти два Caddy-balancer знают slot-порты; обычный
  application `503` не исключает живой slot, депулинг выполняется active `/ready` checks.
- Engine-данные (`/overview`, `/capacity`, `/subs`, `/metrics`) определены в
  `crates/server/src/http.rs`. `/overview` содержит полный список engine accounts без API-ключей.
- Commerce-данные находятся в `apps/api` за `AdminGuard`; authoritative live balance по-прежнему
  живёт только в engine.
- Partner-данные читаются через sales admin API. Main admin получает только server-side proxy;
  отдельная полная партнёрская админка остаётся тем же sales-web `/admin` на
  `admin.partners.apitoken.sale`.
- CRM-код находится в отдельном репозитории. Main admin показывает его engine service account
  с handle `crm-parsing` и ссылку на `crm.apitoken.sale`.

## Возможности

- Сводка всех контуров: commerce, engine, partners и CRM.
- Аккаунты: все engine/service accounts, все partner accounts (с полной пагинацией источника),
  commerce total и переход к полному пользовательскому workflow.
- Администраторы: создание identity с одним или несколькими domain grants, точный фильтр по домену,
  password rotation любого account (включая текущий), enable/disable и защита последнего active
  main-admin от lockout.
- Пользователи: поиск/фильтры, live баланс/расход, платежи, ключи, тир, 2FA, начисление баланса,
  revoke всех сессий, сброс 2FA, enable/disable.
- Деньги: подтверждённые платежи, состояние engine credit, незавершённые checkout.
- B2B: одноразовые email-bound invite-ссылки и индивидуальная скидка.
- Система: полный бывший panel view — verdict, 1d/5h/7d supply, headroom, coverage, fleet demand,
  рекомендации, lifecycle/peaks/proxy, все engine accounts и live util/reset/cooling по подпискам.
- Аудит: operator/user/provider события и причины административных действий.

Ручное начисление принимает целые USD, UUID idempotency key и обязательную причину. Положительное
зачисление идемпотентно; подарок не считается платным top-up. Отключение пользователя сначала
блокирует authoritative engine account, затем commerce mapping и сессии.

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

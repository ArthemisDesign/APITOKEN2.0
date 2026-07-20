# PANEL.md — единая админ-панель admin.apitoken.sale

`admin.apitoken.sale` — единый центр управления для владельца: commerce-пользователи и деньги,
engine-аккаунты и ёмкость, партнёрские аккаунты, CRM service account, безопасность, B2B и аудит.
Легаси `panel.apitoken.sale` удалён после переноса всех его функций сюда.

## Архитектура

```
браузер ──basic_auth──▶ Caddy (admin.apitoken.sale)
                          ├─ /                         → engine GET /admin-panel
                          ├─ /overview /capacity
                          │  /metrics /subs           → engine :8787/:8788 (+ control key)
                          ├─ /admin/*                 → commerce :3000/:3001 /v1/admin/*
                          │                              (+ commerce admin key + actor)
                          └─ /partner-admin/*         → sales-api :3100 /v1/admin/*
                                                         (+ sales admin key)
```

- HTML живёт только в `crates/server/src/admin-panel.html` и вкомпилирован в engine как
  `GET /admin-panel`. Статических копий и второго panel HTML нет.
- Caddy Basic Auth — единый human gate. Control, commerce-admin и sales-admin ключи инжектятся
  только server-side и никогда не попадают в HTML, browser storage, ответы или логи.
- Проверенный Basic Auth login передаётся commerce как `x-admin-actor`, чтобы аудит различал
  операторов. Admin API не публикуется через клиентский web.
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
- `crm.apitoken.sale` — прежнее содержимое CRM, изменён только hostname; CRM human credentials
  остаются отдельной группой `crm_admins`.
- `panel.apitoken.sale`, `partners.panel.apitoken.sale`, `crm.panel.apitoken.sale` — удалены без
  redirect и должны считаться ошибкой production verification, если снова начнут обслуживаться.

## Как добавить источник данных

1. Engine: новый endpoint за `control_authed`/`readonly_authed`, затем разрешить путь в
   `@admin_data` Caddy.
2. Commerce: endpoint за `AdminGuard`; `/admin/*` уже проксируется в `/v1/admin/*`.
3. Sales: endpoint за `AdminKeyGuard`; main admin использует `/partner-admin/*`.
4. UI: вкладка/ветка `refresh()` в `admin-panel.html`. Частичный источник должен показывать
   degraded state, а не просить секрет у оператора.
5. Деплой: обычный push в master. Watchdog применяет Caddy, проверяет marker HTML, Basic Auth на
   трёх активных admin/CRM hosts и отсутствие трёх retired hosts.

## Секреты

| Секрет | Где живёт | Кто проверяет |
|---|---|---|
| bcrypt admin group | live Caddy `panel_admins` | Caddy |
| engine control key | live Caddy + engine env | `control_authed` |
| `COMMERCIAL_ADMIN_KEY` | live Caddy + commerce env | `AdminGuard` |
| `SALES_ADMIN_KEY` | live Caddy + sales env | `AdminKeyGuard` |
| CRM bcrypt group | live Caddy `crm_admins` | Caddy |

`deploy/render-caddy.awk` переносит секретные строки из live Caddy по placeholder-ам. Значения
никогда не добавляются в репозиторий. Для изменения человека: сгенерировать bcrypt через
`caddy hash-password`, изменить соответствующий live snippet и reload Caddy.

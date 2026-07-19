# PANEL.md — админ-панель panel.apitoken.sale

Единый центр управления для владельца: спрос/предложение пула, жизненный цикл подписок,
пользователи коммерции. Читай этот файл ПЕРЕД тем, как развивать панель.

## Архитектура (кто что делает)

```
браузер ──basic_auth──▶ Caddy (panel.apitoken.sale)
                          ├─ /                    → движок GET /panel (HTML, include_str! в бинаре)
                          ├─ /overview /capacity
                          │  /metrics /subs      → движок :8787/:8788 (+ header_up x-api-key = CONTROL key)
                          └─ /admin/users        → commerce-backend :3000/:3001 /v1/admin/users
                                                   (+ header_up x-admin-key = COMMERCIAL_ADMIN_KEY)
```

- **Один вход — basic auth Caddy.** Все секреты (control-ключ движка, admin-ключ коммерции)
  инжектятся server-side; браузер их не видит. Экран «ключ панели…» в HTML — только фолбэк для
  прямого доступа к движку в обход Caddy; за Caddy он появляться не должен.
- **HTML панели живёт в `crates/server/src/panel.html`** и вкомпилирован в бинарь движка
  (`GET /panel`, без авторизации — данные всё равно требуют ключей). Деплой движка = деплой панели,
  статических копий НЕТ (раньше была `/srv/claude-api/panel/index.html` — упразднена, дрейфовала).
- **Данные движка** (`/overview`, `/capacity`, `/subs`, `/metrics`) — `crates/server/src/http.rs`.
- **Данные коммерции** (`/v1/admin/users`, `POST /v1/admin/users/:id/credit`) —
  `apps/api/src/admin.controller.ts` (AdminGuard по `x-admin-key`), агрегат —
  `packages/db/src/admin-overview.ts` + live-деньги через engine Control API.
  Начисление: целые USD строкой цифр; кредитует движок идемпотентно по ref, след — в audit_log;
  тир НЕ двигает (мимо payments/engine_credits — это подарок, не пополнение).

## Как добавить новую вкладку / источник данных

1. **Данные движка** → новый эндпоинт в `crates/server/src/http.rs` за `control_authed`
   (или `readonly_authed`), добавить путь в матчер `@data` в `deploy/Caddyfile`.
2. **Данные коммерции** → эндпоинт в `apps/api` за `AdminGuard`; в `deploy/Caddyfile` расширить
   матчер `@users` (path`/admin/…`) — rewrite на `/v1/…` и proxy на :3000/:3001 уже настроены.
3. **UI** → вкладка в `panel.html`: добавить кнопку в `tabsHtml()`, ветку в `tick()`, свой
   `render…()`. На 401 данных коммерции НЕ показывать экран ключа — только инлайн-ошибку.
4. Деплой: обычный merge в master (watchdog). Изменения `deploy/Caddyfile` watchdog применяет сам
   (`--apply-caddy`), секретные строки переносит из живого конфига `install-caddy.sh`.

## Вкладка «Партнёры» (sales bounded context)

`/partners-admin/*` → Caddy strip prefix + rewrite `/v1/admin{uri}` → sales-api :3100 с
server-side `header_up x-sales-admin-key` (ОТДЕЛЬНЫЙ заголовок, чтобы перенос секретов не путал
его с commerce `x-admin-key`; sales-api принимает оба). UI: сводка, заявки в программу
(approve/reject c bps), очередь выплат (BSC-кошелёк, approve/paid/reject), таблица партнёров
(проценты prompt'ом, заморозка, удаление — только без истории), «+ пригласить» (корневой инвайт).
Полная админка остаётся на partners.apitoken.sale/admin.

## Секреты (четыре, все только server-side)

| Секрет | Где живёт | Кто проверяет |
|---|---|---|
| basic auth админов (bcrypt, по строке на человека: `Q`, `R`, `M`, легаси `admin`) | `/etc/caddy/Caddyfile` | Caddy |
| CONTROL-ключ движка (`x-api-key`) | `/etc/caddy/Caddyfile` + env движка | движок `control_authed` |
| `COMMERCIAL_ADMIN_KEY` (`x-admin-key`) | `/etc/caddy/Caddyfile` + `/etc/apitoken/api.env` | backend `AdminGuard` |
| `SALES_ADMIN_KEY` (`x-sales-admin-key`) | `/etc/caddy/Caddyfile` + `/etc/apitoken/sales.env` | sales-api `AdminKeyGuard` |

**Инвариант:** значение `x-admin-key` в Caddyfile ОБЯЗАНО совпадать с `COMMERCIAL_ADMIN_KEY` в
`api.env` (реальный инцидент 2026-07-17: ключи разошлись → 401 → панель просила «ключ»).
`install-caddy.sh` переносит строки секретов из живого Caddyfile по placeholder'ам
(`<BASIC_AUTH_USERS_PLACEHOLDER>` — ВСЕ строки `user $2y$…` разом, `<CONTROL_KEY_PLACEHOLDER>`,
`<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>`) — новый секрет = новый placeholder + ветка в awk +
ручная первичная вставка в живой конфиг.

**Добавить/убрать админа:** `caddy hash-password --plaintext '<пароль>'` → добавить/удалить строку
`<логин> <хэш>` в блоке `basic_auth` живого `/etc/caddy/Caddyfile` → `systemctl reload caddy`.
Шаблонные применения (`--apply-caddy`) переносят все строки автоматически. Пароли хранятся только
у людей; на сервере — только bcrypt.

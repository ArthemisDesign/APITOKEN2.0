# PANEL.md — админ-панель admin.apitoken.sale

Единый центр управления для владельца: спрос/предложение пула, жизненный цикл подписок,
пользователи, регистрации, деньги, безопасность, B2B и аудит коммерции. Существующий
`panel.apitoken.sale` — отдельный dashboard и не меняется вместе с admin-сайтом. Читай этот файл
ПЕРЕД тем, как развивать любую из панелей.

## Архитектура (кто что делает)

```
браузер ──basic_auth──▶ Caddy (admin.apitoken.sale)
                          ├─ /                    → движок GET /admin-panel (HTML, include_str!)
                          ├─ /overview /capacity
                          │  /metrics /subs      → движок :8787/:8788 (+ header_up x-api-key = CONTROL key)
                          └─ /admin/*            → commerce-backend :3000/:3001 /v1/admin/*
                                                   (+ header_up x-admin-key = COMMERCIAL_ADMIN_KEY)
```

- **Один вход — basic auth Caddy.** Все секреты (control-ключ движка, admin-ключ коммерции)
  инжектятся server-side; браузер их не видит. Caddy также передаёт проверенный Basic Auth login
  как `x-admin-actor`, чтобы audit отличал администраторов. Admin API нельзя вызывать напрямую из public web.
- **HTML admin-сайта живёт в `crates/server/src/admin-panel.html`** и вкомпилирован в бинарь движка
  (`GET /admin-panel`, без авторизации — данные всё равно требуют ключей). Деплой движка = деплой admin,
  статических копий НЕТ (раньше была `/srv/claude-api/panel/index.html` — упразднена, дрейфовала).
- **Существующая панель изолирована:** `panel.apitoken.sale` продолжает получать `GET /panel`,
  собранный из `crates/server/src/panel.html`, со своими прежними Caddy matchers. Новый admin host
  имеет отдельный Caddy-блок и никогда не rewrite'ит запросы в `/panel`.
- **Данные движка** (`/overview`, `/capacity`, `/subs`, `/metrics`) — `crates/server/src/http.rs`.
- **Данные коммерции** — `apps/api/src/admin.controller.ts` и
  `apps/api/src/admin-operations.controller.ts` (оба за `AdminGuard` по `x-admin-key`), агрегаты и
  admin-транзакции — `packages/db/src/admin-overview.ts`, live-деньги — только через engine Control API.
- **Регистрации** считаются по исходному способу: `auth.oauth_registered` = OAuth; отсутствие
  этого события = обычная email/password регистрация. Текущие привязанные методы показываются
  отдельно, поэтому поздний OAuth claim не переписывает происхождение аккаунта.
- **Ручное начисление** принимает целые USD, UUID idempotency key и обязательную причину. Движок
  кредитуется идемпотентно по `admin-credit:<uuid>`, audit_log дедуплицируется тем же ref. Подарок
  не считается платным пополнением и не двигает prepay-тир.
- **Отключение пользователя** сначала блокирует authoritative engine account, затем атомарно
  отключает commerce-пользователя, mapping и все сессии. Включение согласует те же два контура.

## Возможности администратора

- KPI: клиенты, новые и активные, OAuth/обычные регистрации, платящие клиенты, число и сумма
  top-up, ручные начисления, checkout/refund/error, ключи, сессии и engine-account states.
- Пользователи: поиск/фильтры, live баланс/расход, платежи, ключи, тир, 2FA, начисление баланса,
  revoke всех сессий, сброс 2FA, enable/disable.
- Деньги: последние подтверждённые платежи с состоянием engine credit и незавершённые checkout.
- B2B: одноразовые email-bound invite-ссылки и изменение индивидуальной скидки.
- Система: ёмкость, headroom, fleet health, lifecycle подписок и прокси.
- Аудит: последние operator/user/provider события и причины административных действий.

## Как добавить новую вкладку / источник данных

1. **Данные движка** → новый эндпоинт в `crates/server/src/http.rs` за `control_authed`
   (или `readonly_authed`), добавить путь в матчер `@data` в `deploy/Caddyfile`.
2. **Данные коммерции** → эндпоинт в `apps/api` за `AdminGuard`; в `deploy/Caddyfile` расширить
   матчер `@commerce_admin` уже проксирует весь path `/admin/*` на `/v1/admin/*`.
3. **UI** → вкладка в `admin-panel.html`: добавить кнопку, ветку в `refresh()`, свой
   `render…()`. На 401 данных коммерции НЕ показывать экран ключа — только инлайн-ошибку.
4. Деплой: обычный merge в master (watchdog). Изменения `deploy/Caddyfile` watchdog применяет сам
   (`--apply-caddy`), секретные строки переносит из живого конфига `install-caddy.sh`. Финальный
   watchdog-gate проверяет marker встроенного HTML и что `admin.apitoken.sale` отвечает `401` без
   Basic Auth; незащищённая или не задеплоенная панель блокирует завершение rollout.

## Партнёрская админка — вынесена на отдельный сайт

Раньше была вкладкой «Партнёры» в этой панели; теперь это **отдельный админ-сайт
`partners.panel.apitoken.sale`** (тот же sales-web `/admin` + sales-api). Вход — оператор вводит
`SALES_ADMIN_KEY` в KeyGate (ключ НЕ инжектится server-side, уходит как `x-sales-admin-key` из
sessionStorage). Caddy: `/v1/*`→sales-api :3100, `/`→редирект на `/admin`, остальное→sales-web :3200.
В этой панели маршрута `/partners-admin/*` и вкладки больше нет.

## Секреты (три, все только server-side)

| Секрет | Где живёт | Кто проверяет |
|---|---|---|
| basic auth админов (bcrypt, по строке на человека: `Q`, `R`, `M`, легаси `admin`) | `/etc/caddy/Caddyfile` | Caddy snippet `panel_admins`, imported by both isolated hosts |
| CONTROL-ключ движка (`x-api-key`) | `/etc/caddy/Caddyfile` + env движка | движок `control_authed` |
| `COMMERCIAL_ADMIN_KEY` (`x-admin-key`) | `/etc/caddy/Caddyfile` + `/etc/apitoken/api.env` | backend `AdminGuard` |

(`SALES_ADMIN_KEY` больше не инжектится Caddy — партнёрская админка на `partners.panel.apitoken.sale`
принимает его от оператора через `x-sales-admin-key`; в этой панели его нет.)

**Инвариант:** значение `x-admin-key` в Caddyfile ОБЯЗАНО совпадать с `COMMERCIAL_ADMIN_KEY` в
`api.env` (реальный инцидент 2026-07-17: ключи разошлись → 401 → панель просила «ключ»).
`install-caddy.sh` переносит строки секретов из живого Caddyfile по placeholder'ам
(`<BASIC_AUTH_USERS_PLACEHOLDER>` — ВСЕ строки `user $2y$…` разом, `<CONTROL_KEY_PLACEHOLDER>`,
`<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>` и admin-копии control/admin placeholders) — новый секрет = новый placeholder + ветка в awk +
ручная первичная вставка в живой конфиг.

**Добавить/убрать админа:** `caddy hash-password --plaintext '<пароль>'` → добавить/удалить строку
`<логин> <хэш>` в блоке `basic_auth` живого `/etc/caddy/Caddyfile` → `systemctl reload caddy`.
Шаблонные применения (`--apply-caddy`) переносят все строки автоматически. Пароли хранятся только
у людей; на сервере — только bcrypt.

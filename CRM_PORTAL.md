# CRM_PORTAL.md — внутренняя CRM crm.panel.apitoken.sale (каркас)

Отдельный внутренний продукт «CRM & Parsing». Разрабатывается в ветке **`feat/crm-portal`**;
в master вливается, когда появится реальное наполнение. Внутренности CRM пока НЕ спроектированы —
этот файл фиксирует только каркас и границы.

## Что уже есть

- **`apps/crm-web`** — Next.js (порт **:3300**), дизайн-DNA основных панелей (light, JetBrains
  Mono, тонкие рамки, 4px radius, один синий акцент; `globals.css` скопирован из sales-web).
  Пока одна страница-заглушка (сайдбар, стат-карточки, разделы «soon»).
- **Caddy vhost** `crm.panel.apitoken.sale` → `127.0.0.1:3300` в `deploy/Caddyfile` со СВОИМ
  снипетом `(crm_admins)` — НЕ `panel_admins`.
- **Учётки доступа** — отдельные от основной панели, дают доступ ТОЛЬКО к CRM:
  `Q_Sales`, `R_Sales`, `M_Sales` (bcrypt-хэши сгенерированы, пароли — у людей;
  локальная копия — `secrets/CRM.md`, git-игнорируется).
- **`install-caddy.sh`** — awk стал group-aware: bcrypt-строки внутри `(crm_admins)` переносятся
  в `<CRM_ADMIN_USERS_PLACEHOLDER>`, остальные — в панельный placeholder. Пока в живом конфиге
  нет учёток CRM, рендерится запертая заглушка (`disabled` со случайным паролем) — применение
  шаблона не падает, сайт заперт.
- **`systemd/apitoken-crm-web.service`** — юнит (:3300, релизы `/opt/apitoken/crm-releases`),
  на сервер пока не установлен.
- **Engine-ключ «CRM & Parsing»** выпущен: аккаунт `acct_1113acfc74b987e88f097283`
  (handle `crm-parsing`), key_id `key_089eb45fa269e3ec3d698bbde6aefdfc`, баланс $100.
  Сам ключ — в `secrets/CRM.md`. Аккаунт виден в панели: таблица «Аккаунты движка» (/overview).

## Первичный ввод в прод (когда каркас вольём в master)

1. Merge `feat/crm-portal` → master; дождаться зелёного watchdog.
2. В живой `/etc/caddy/Caddyfile` добавить блок из шаблона и три строки учёток в `(crm_admins)`
   (строки — в `secrets/CRM.md`), затем `systemctl reload caddy`. Дальше `--apply-caddy`
   переносит их автоматически.
3. Поднять crm-web: первый релиз руками (или расширить watchdog, см. TODO) +
   `systemctl enable --now apitoken-crm-web.service`.

## TODO (сознательно отложено)

- Деплой-конвейер: классификатор `wd_path_is_crm` в `deploy/watchdog-lib.sh` + `crm-deploy.sh`
  (по образцу sales: свой release-root `/opt/apitoken/crm-releases`, health-gate на :3300).
- Наполнение CRM (лиды/парсинг/воронка) — по постановке владельца; данные и API пока не выбраны.
- Если CRM понадобится бэкенд — отдельный `apps/crm-api` (свой порт), НЕ переиспользовать
  sales-api/commerce напрямую; связь с движком — только через Control API (`CONTROL_API.md`).

## Инварианты

- Доступ к CRM — только учётки `*_Sales`; админы `Q/R/M` основной панели сюда автоматически
  НЕ попадают (и наоборот: `*_Sales` не имеют доступа к panel/partners.panel).
- Секреты (пароли, engine-ключ CRM) не коммитим; репозиторий хранит только placeholder'ы.

# CRM_PORTAL.md — внутренняя CRM crm.panel.apitoken.sale (каркас)

Отдельный внутренний продукт «CRM & Parsing». Разрабатывается в ветке **`feat/crm-portal`**;
в master вливается, когда появится реальное наполнение. Внутренности CRM пока НЕ спроектированы —
этот файл фиксирует только каркас и границы.

## Что уже есть

- **AI-CRM собрана** (вертикальный срез, всё проверено смоуком с живыми AI-вызовами):
  устройство — **`CRM_AI.md`**, контракт для парсеров — **`CRM_PARSING_SPEC.md`**.
  - `packages/crm-db` — Postgres-схема (drizzle) + миграции (`0000_crm_init`) + фильтр-DSL
    (vitest-тесты зелёные);
  - `apps/crm-api` (порт **:3400**, NestJS/Fastify) — ингест с AI-адаптером формата,
    AI-куратор реестра признаков, AI-сегментатор (smart views), поиск по описанию (/ask);
    env — `/etc/apitoken/crm.env` (шаблон в `secrets/CRM.md`).
- **`apps/crm-web`** — Next.js (порт **:3300**), дизайн-DNA основных панелей (light, JetBrains
  Mono, тонкие рамки, 4px radius, один синий акцент; `globals.css` скопирован из sales-web).
  Живой кабинет поверх `/v1` crm-api: Дашборд, Контакты (+сегменты), Сегменты, Признаки, Спросить AI.
- **Caddy vhost** `crm.panel.apitoken.sale` в `deploy/Caddyfile` со СВОИМ снипетом
  `(crm_admins)` — НЕ `panel_admins`. Роуты: `/v1/ingest/*` → :3400 БЕЗ basic_auth (парсеры,
  ключ `x-crm-ingest-key`), `/v1/*` → :3400 за basic_auth, остальное → :3300 (crm-web).
- **Учётки доступа** — отдельные от основной панели, дают доступ ТОЛЬКО к CRM:
  `Q_Sales`, `R_Sales`, `M_Sales` (bcrypt-хэши сгенерированы, пароли — у людей;
  локальная копия — `secrets/CRM.md`, git-игнорируется).
- **`install-caddy.sh`** — awk стал group-aware: bcrypt-строки внутри `(crm_admins)` переносятся
  в `<CRM_ADMIN_USERS_PLACEHOLDER>`, остальные — в панельный placeholder. Пока в живом конфиге
  нет учёток CRM, рендерится запертая заглушка (`disabled` со случайным паролем) — применение
  шаблона не падает, сайт заперт.
- **`systemd/apitoken-crm-web.service` + `systemd/apitoken-crm-api.service`** — юниты
  (:3300/:3400, релизы `/opt/apitoken/crm-releases`), на сервер пока не установлены.
- **Engine-ключ «CRM & Parsing»** выпущен: аккаунт `acct_1113acfc74b987e88f097283`
  (handle `crm-parsing`), key_id `key_089eb45fa269e3ec3d698bbde6aefdfc`, баланс $100.
  Сам ключ — в `secrets/CRM.md`. Аккаунт виден в панели: таблица «Аккаунты движка» (/overview).

## Первичный ввод в прод (когда каркас вольём в master)

1. Merge `feat/crm-portal` → master; дождаться зелёного watchdog.
2. В живой `/etc/caddy/Caddyfile` добавить блок из шаблона и три строки учёток в `(crm_admins)`
   (строки — в `secrets/CRM.md`), затем `systemctl reload caddy`. Дальше `--apply-caddy`
   переносит их автоматически.
3. Создать базу `apitoken_crm` в Postgres и `/etc/apitoken/crm.env` (шаблон — `secrets/CRM.md`);
   прогнать `node packages/crm-db/dist/migrate.js`.
4. Поднять crm-api и crm-web: первый релиз руками (или расширить watchdog, см. TODO) +
   `systemctl enable --now apitoken-crm-api.service apitoken-crm-web.service`.

## TODO (сознательно отложено)

- Деплой-конвейер: классификатор `wd_path_is_crm` в `deploy/watchdog-lib.sh` + `crm-deploy.sh`
  (по образцу sales: свой release-root `/opt/apitoken/crm-releases`, health-gate на :3300).
- Первые реальные парсеры (по `CRM_PARSING_SPEC.md`) и обкатка AI-сегментации на живом корпусе.
- Воронка/статусы/касания (outreach) — по постановке владельца.
- crm-api НЕ переиспользует sales-api/commerce напрямую; AI ходит через клиентский `/v1` движка
  по ключу «CRM & Parsing» (Control API движка CRM не трогает).

## Инварианты

- Доступ к CRM — только учётки `*_Sales`; админы `Q/R/M` основной панели сюда автоматически
  НЕ попадают (и наоборот: `*_Sales` не имеют доступа к panel/partners.panel).
- Секреты (пароли, engine-ключ CRM) не коммитим; репозиторий хранит только placeholder'ы.

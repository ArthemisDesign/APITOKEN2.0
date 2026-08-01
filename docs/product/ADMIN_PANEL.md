# Admin panel — внутренняя админка (admin.apitoken.sale)

Next.js-приложение `apps/admin` (`@claude-api/admin`). UI извлечён из встроенной админ-панели
Rust-движка (`crates/server/src/admin-panel.html`) в отдельный bounded context с собственным
жизненным циклом релизов — как у sales (`apps/sales-web`) и OpenKeys (`apps/openkeys`).

Закрытый sales-калькулятор доступен по `https://admin.apitoken.sale/sales/calculator`. Он сравнивает
Claude Pro/Max, paid ChatGPT и paid Gemini планы по живой калибровке 5ч/7д, выводит устойчивый
30-дневный API-dollar equivalent и считает скидку, недоиспользованную квоту, экономию клиента,
упущенную выручку и валовую разницу. Денежная арифметика страницы — integer nanoUSD. Холодные
якоря и Claude priors не подставляются: до фактического движения quota значение остаётся неизвестным.

## Состав

- Приложение: `apps/admin`, Next.js, слушает `127.0.0.1:3700`.
- Собственной БД и секретов нет: ни миграций, ни env-файла (в отличие от sales/OpenKeys).
- Workspace-зависимостей, кроме Next/React, нет — в TypeScript-контекстах это отдельный
  контекст `admin` с корнем `apps/admin` (как `web` → `apps/web`).
- Health endpoint: `GET /api/health` → 200 `{"ok":true}`.

## Релизный цикл (watchdog lane `admin`)

- Классификация путей: `wd_path_is_admin` в `deploy/watchdog-lib.sh` (`apps/admin/**` плюс общие
  build-файлы — бамп зависимостей пере-собирает релиз). Backend-лейн `apps/admin` не захватывает.
- Baseline: `/var/lib/apitoken/watchdog/admin.sha`. Пока файла нет, первый запуск watchdog
  деплоит контекст безусловно (как OpenKeys).
- Релизный корень: `/opt/apitoken/admin-releases/<sha>`, атомарный симлинк `current`.
- Deploy-скрипт: `deploy/admin-deploy.sh <sha>` — промоушен протестированного кандидата,
  атомарный симлинк, `systemctl restart apitoken-admin.service`, health-gate на
  `http://127.0.0.1:3700/api/health`, откат симлинка при неуспехе. Миграций нет.
- Юнит: `systemd/apitoken-admin.service` (`User=deploy`, `next start -H 127.0.0.1 -p 3700`,
  харденинг как у `apitoken-openkeys.service`, включая `AF_NETLINK`; без `EnvironmentFile`).
- GitHub: статус-контекст `deploy/admin`, deployment environment `production-admin`.

## Первый запуск на сервере

```bash
# 1. Раскатить обновлённые юниты, sudoers и контроллеры (root)
deploy/install-watchdog.sh

# 2. Включить юнит один раз
systemctl enable apitoken-admin.service

# 3. Дальше выкат автоматический: watchdog увидит изменения в apps/admin
#    и вызовет admin-deploy.sh
```

## Домен

`admin.apitoken.sale` обслуживает `apps/admin` на `127.0.0.1:3700` и целиком закрыт
`managed_admin_auth`: логин/пароль проверяет commerce internal auth с domain grants. Caddy
same-origin проксирует обезличенные `/capacity`, `/codex-subs`, `/gemini-subs` в три
provider runtime и добавляет серверные ключи; браузер не получает control keys, полный email,
OAuth, Google project или proxy. Защита относится ко всем страницам, включая `/sales/calculator`.

# Admin panel — внутренняя админка (admin.apitoken.sale)

Next.js-приложение `apps/admin` (`@claude-api/admin`). UI извлечён из встроенной админ-панели
Rust-движка (`crates/server/src/admin-panel.html`) в отдельный bounded context с собственным
жизненным циклом релизов — как у sales (`apps/sales-web`) и OpenKeys (`apps/openkeys`).

Закрытый sales-калькулятор доступен по `https://admin.apitoken.sale/sales/calculator`. Он сравнивает
Claude Pro/Max, paid ChatGPT и paid Gemini планы по живой калибровке 5ч/7д, выводит устойчивый
30-дневный API-dollar equivalent и считает скидку, недоиспользованную квоту, экономию клиента,
упущенную выручку и валовую разницу. Денежная арифметика страницы — integer nanoUSD. Холодные
якоря и Claude priors не считаются измерением. Если сам тариф ещё не калиброван, но у провайдера
есть другой измеренный тариф, калькулятор масштабирует его API-ёмкость по официальному соотношению
квот и явно помечает результат знаком `≈` и статусом «расчёт». Собственное измерение всегда
приоритетнее и автоматически заменяет расчётное значение.

Расчётные коэффициенты и их authority:

- Claude: Pro / Max 5× / Max 20× = `1:5:20` по [Anthropic pricing](https://www.anthropic.com/pricing).
- ChatGPT: Plus / Pro 5× / Pro 20× = `1:5:20`, Business имеет ту же опубликованную 5-часовую
  квоту, что Plus, по [OpenAI pricing](https://learn.chatgpt.com/docs/pricing). Runtime-plan
  `chatgpt_pro` — покупаемая Authbot подписка за $200, то есть Pro 20×; Pro 5× пока существует
  в калькуляторе как расчётная линия `chatgpt_pro_5x` за $100.
- Google AI: в коммерческой матрице присутствуют только используемые пулом планы Pro и Ultra.
  Google публикует для Ultra до `20×` больше лимитов Gemini относительно Pro, поэтому расчётный
  коэффициент Pro / Ultra = `1:20` по [Google AI plans](https://one.google.com/about/google-ai-plans/).
  Code Assist Standard/Enterprise и Workspace Ultra в калькулятор не выводятся.

Масштабирование выполняется отдельно для 5ч, 7д и 30д только через integer BigInt. Источниками
служат исключительно прямые измерения тарифов того же провайдера; уже рассчитанные значения не
используются рекурсивно. Если прямых опор несколько, берётся среднее нормализованных значений.
Опубликованные коэффициенты описывают 5-часовые Claude/OpenAI лимиты и верхнюю границу Google AI
Ultra; дополнительные недельные ограничения и workload-зависимое потребление могут отличаться,
поэтому расчёт 7д/30д не считается калибровкой до собственного live-измерения тарифа.

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

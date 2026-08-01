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

## Pricing configurators и B2B policies

Страница `/pricing` — операторская поверхность versioned multi-discount authority:

- Global B2C редактируется полным CAS replacement-набором provider/model rules. Для B2C разрешены
  `track` и точные static overrides; exact model rule имеет приоритет над provider rule;
- provider switches показывают master, product, B2C и B2B gates. Master визуально отделён, а его
  изменение и любое выключение gate требуют отдельного browser confirmation. Сохранённые policy
  rules при выключении не удаляются;
- provider rule не включает будущие модели автоматически. Редактор предлагает только модели из
  активного product catalog; Gemini не появляется без явной catalog entry;
- service inventory охватывает все products, показывает `purpose` и `responsible` из утверждённой
  Stage 5 matrix и открывает product-aware policy editor. Service и B2B принимают только static
  discount rules, не `track`;
- каждое сохранение показывает новую source version и не объявляется применённым, пока targets не
  имеют совпадающие desired/applied versions и exact ACK. В UI видны job state, последняя ошибка,
  actor, reason и время версии.

Страница `/business` использует тот же policy editor для существующих B2B clients и активных
invitations. Новая invitation создаётся сразу с полной provider/model policy; scalar discount editor
в активном UI отсутствует. Непогашенная invitation редактируется CAS replacement-версиями, resend
получает независимую exact snapshot, а после redemption изменение invitation уже не меняет client
policy. Preview/email/registration описывают provider/model доступ и account остаётся pending до
engine ACK; usable key до подтверждения policy не выдаётся.

Админка не выполняет Stage 5 assignment/backfill сама и не выводит назначения B2B/service/OpenKeys
из имён. Пока catalog/policy foundation отсутствует, соответствующие редакторы fail closed и явно
показывают, что materialization ещё не выполнена.

## GPT capacity board на странице подписок

GPT-блок `/subscriptions` — компактная операторская сводка, полностью рассчитанная из backend
`/codex-subs` без собственной денежной authority:

- главный strip суммирует `fleet_capacity_nanocredits`/`fleet_remaining_nanocredits` всех plan cohorts самого длинного
  доступного положительного окна, показывает использованную долю, обычный Standard API-equivalent
  и максимальный тарифный API-equivalent; оба сценария подписаны моделью/tier/context/token kind.
  Если хотя бы одна cohort ещё не измерена, общий номинал остаётся неизвестным, а не превращается
  в заниженную частичную сумму;
- таблица token capacity отвечает, сколько fresh, cache-read, cache-write либо output/reasoning
  токенов можно обслужить текущим остатком всего пула для каждой модели и Standard/Fast. Cache write
  расходует native fresh-input credits; reasoning уже входит в output;
- profitability matrix выбирает самый выгодный short/long-контекст для каждой пары модель ×
  Standard/Fast, показывает exact `$ API-equivalent / native credit` по четырём token kinds и
  сортирует строки по лучшему значению убыванию. Все деньги и credits считаются только BigInt;
- home-таблица показывает только bounded masked email, runtime/integrity state, quota с progress-bar
  и reset, shared-cohort remaining credits и обычный/максимальный API-equivalent. Для разных paid
  plans каждая почта получает pooled capacity только своей cohort. Opaque UUID, raw immutable ledger,
  schedules и индивидуальная noisy capacity в основной UI не выводятся;
- provider placeholder с неположительным окном игнорируется. До появления положительного движения
  quota UI показывает короткое `ждём Δquota`, не подставляя ноль или прайор.

## Claude и Gemini capacity boards

Claude- и Gemini-блоки `/subscriptions` используют ту же компактную операторскую композицию:
одна строка ёмкости, таблицы токенов/тарифов и аккаунты с quota progress-bar и reset. Старые
StatCard-наборы, proxy/transport details и длинные calibration explanations в основном экране не
выводятся. Во всех трёх пулах аккаунт слева обозначается только bounded email hint — первые четыре
символа local-part без домена.

Claude строится из `/capacity`:

- `window_totals` и `available_nano` — decimal nanoUSD strings; 5ч/7д strip и token-only capacity
  считаются через BigInt. `conversion_models` содержит authoritative Standard/Fast ставки metering
  для input, cache read, cache write 5м/1ч и output плюс отдельный Web Search SKU;
- строки profitability сортируются по максимальному официальному `$ / 1M токенов`. Это рейтинг
  выручки на токен при продаже по API-тарифу, а не обещание разной маржи на один и тот же доллар
  откалиброванной ёмкости;
- per-sub таблица оставляет только masked email/plan, routing state, окна 5ч/7д и текущий 7д
  API-dollar remaining. Prior явно помечается и не выдаётся за завершённую калибровку.

Gemini строится из `/gemini-subs` и сохраняет provider-specific семантику:

- workload 5ч/weekly API-$ — realized blend наблюдённой смеси, не фиксированный номинал Google AI
  подписки. Fleet totals имеют canonical `*_nano` strings; float-поля остаются только display
  compatibility;
- `conversion_models` публикует paid-tier ставки для uncached/audio/cached input,
  output+thinking, image output, long context и Search. Profitability сортируется по токеновому
  тарифу; Search показывается отдельно, потому что его единица — query или grounded prompt;
- official quota join использует backend-публикуемый список private quota bucket ids каждой
  canonical модели. Если Google прислал `remaining_amount`, UI суммирует только эти целые значения.
  Если опубликована лишь fraction, token amount остаётся `—`: workload-$ никогда не делится на
  цену токена для выдумывания Gemini capacity.

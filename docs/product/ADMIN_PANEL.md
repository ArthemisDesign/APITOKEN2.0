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

Страница `/system` получает supply из `/overview`, который использует ту же exact Claude authority,
что `/capacity`. Если canonical remaining недоступен, UI показывает `—` и warning, а не `$0` и не
старый pool prior/EMA; отдельный дублирующий browser-запрос `/capacity` удалён.

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

- Global B2C редактируется полным CAS replacement-набором: default 50%, provider overrides и exact
  model overrides. Exact model rule имеет приоритет над provider, provider — над global default;
- provider switches показывают master, product, B2C и B2B gates. Master визуально отделён, а его
  изменение и любое выключение gate требуют отдельного browser confirmation. Сохранённые policy
  rules при выключении не удаляются;
- provider rule не включает будущие модели автоматически. Редактор предлагает только модели из
  активного product catalog; Gemini не появляется без явной catalog entry;
- backend admin API публикует canonical service inventory и exact-CAS mutation под
  `/admin/service-account-inventory`; он показывает `purpose`, `responsible`, last verified engine
  status, all-runtime-model access и `billing_mode=meter_only`. Текущий UI не классифицирует неизвестные
  accounts автоматически: до отдельной формы оператор использует тот же защищённый admin-контракт.
  Service не редактирует product discounts и не зависит от balance;
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
из имён. Commerce producer теперь даёт защищённый bounded snapshot prepared target/recovery,
freshness/source completeness Stage 8, durable activation jobs/receipts и отдельно timestamped
engine head через `GET /admin/pricing-release-activation-v2`; единственный mutation endpoint —
explicit `POST .../stage` с verified actor/reason и canonical evidence digest. Подключение этих
endpoint'ов к странице `/pricing` идёт отдельным consumer-коммитом после GREEN backend producer
SHA. Пока consumer не доставлен, UI эту поверхность не вызывает. Per-account canary и
maintenance-mode controls запрещены.

## GPT capacity board на странице подписок

GPT-блок `/subscriptions` — компактная операторская сводка, полностью рассчитанная из backend
`/codex-subs` без собственной денежной authority:

- главный strip суммирует `fleet_capacity_nanocredits`/`fleet_remaining_nanocredits` всех plan cohorts самого длинного
  доступного положительного окна, показывает использованную долю, обычный Standard API-equivalent
  и максимальный тарифный API-equivalent; оба сценария подписаны моделью/tier/context/token kind.
  Если хотя бы одна cohort ещё не измерена, общий номинал остаётся неизвестным, а не превращается
  в заниженную частичную сумму;
- home-таблица показывает только bounded masked email, runtime/integrity state, quota с progress-bar
  и reset, shared-cohort remaining credits и обычный/максимальный API-equivalent. Для разных paid
  plans каждая почта получает pooled capacity только своей cohort. Opaque UUID, raw immutable ledger,
  schedules и индивидуальная noisy capacity в основной UI не выводятся;
- `conversion_models` используется локально только для двух API-equivalent значений в strip и
  home-таблице. Token-capacity и profitability-матрицы в основной UI не разворачиваются; backend
  продолжает публиковать тарифный каталог как расчётный/audit-контракт;
- provider placeholder с неположительным окном игнорируется. До появления положительного движения
  quota UI показывает короткое `ждём Δquota`, не подставляя ноль или прайор.

## Claude и Gemini capacity boards

Claude-блок `/subscriptions` намеренно оставляет только одну компактную таблицу аккаунтов: bounded
email hint, routing/auth state, quota+reset и exact доступные/полные API-$ отдельно для 5ч и 7д.
Workload evidence, token-only capacity, model profitability и локальный summary strip в Claude-блоке
не выводятся: главные fleet totals уже находятся в едином control-room выше, а оператору внутри
Claude нужны только окна конкретных подписок. Gemini также оставляет только таблицу окон по
профилям; отдельный локальный summary strip, model-quota и profitability таблицы удалены.
Старые StatCard-наборы, proxy/transport details и длинные calibration explanations в основном экране
не выводятся. Во всех трёх пулах аккаунт слева обозначается только bounded email hint — первые четыре
символа local-part без домена.

Над деталями расположен единый control-room из трёх карточек Claude/GPT/Gemini. В каждой только
два одинаково читаемых rail: `5ч` и `7д`, current remaining / full-window API-$, использованная доля,
число routable identities и coverage. Это главный экран сравнения продаваемой ёмкости; подробности
по аккаунтам идут ниже без дополнительных cache/model/token-матриц. Claude-карточка вместо ложных денег немедленно показывает
`N сохраняется`, `N потеряно` или ошибку authority из `calibration_delivery`. Gemini применяет тот
же fail-closed контракт и не показывает stale API-$ при pending/degraded exact authority. Его
свежие provider quota/reset при этом остаются видны, а денежная ячейка компактно говорит
`обновляем`: сбой dollar-evidence не должен ослеплять оператора по реальному quota wall.

Claude строится из `/capacity`:

- `window_totals` и `available_nano` — decimal nanoUSD strings для общего control-room;
  `conversion_models` остаётся backend-каталогом authoritative Standard/Fast ставок metering;
- 5ч API-dollar remaining — акцентный столбец каждой подписки рядом с 5ч quota/reset; полная
  ёмкость окна выводится компактной строкой `из $…`.
  7д remaining/ёмкость остаются соседним сравнительным столбцом. Таблица также оставляет masked
  email/plan и routing state. Никакой prior не подставляется: до exact evidence выводится
  `ждём данные`. Свежая exact quota fraction из runtime даёт current remaining даже когда Anthropic
  не прислал reset — тогда UI пишет `сброс уточняется`, а не ложное `0м`. При stale/missing snapshot
  или pending/degraded FIFO строка показывает `обновляем`, не устаревший процент и не номинальную
  ёмкость. Dead/non-routable аккаунт показывает `вне ротации` и не выглядит продаваемой supply;
- `calibration_evidence` и `conversion_models` продолжают приходить с backend как audit/calculation
  contract, но основной Claude UI их не разворачивает в дополнительные таблицы.

Gemini строится из `/gemini-subs` и сохраняет provider-specific семантику:

- workload 5ч/weekly API-$ — realized blend наблюдённой смеси, не фиксированный номинал Google AI
  подписки. Fleet totals имеют canonical `*_nano` strings; float-поля остаются только display
  compatibility. В strip 5ч workload-$ стоит первым, а per-profile таблица показывает отдельные
  5ч и 7д workload-dollar remaining и полную ёмкость (`из $…`) рядом с соответствующими
  quota/reset;
- таблица профилей показывает bounded email, auth state, quota/reset для 5ч и 7д, доступные/полные
  workload-$ и число доступных моделей. Private quota bucket ids, `remaining_amount`, токеновые
  ставки, Search и profitability в основной UI не выводятся. Неавторизованный, account-cooling
  или полностью model-cooling профиль сохраняет quota для диагностики, но показывает
  `вне ротации` вместо денег и не входит во fleet API-$;
- `conversion_models`, official quotas и их integer amounts продолжают приходить с backend как
  audit/calculation contract. UI не делит workload-$ на цену токена и не выдумывает Gemini token
  capacity из одной только fraction.

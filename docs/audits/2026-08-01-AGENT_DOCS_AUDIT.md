# Аудит системы координации агентов — 2026-08-01

**Объект.** Система «живого контракта» документации, введённая коммитами `ce016f1`
(AGENTS.md, `docs/DEPENDENCIES.md`, `docs/CHANGE_CHECKLISTS.md`, `docs/README.md`) и `e1d39bd`
(`deploy/docs-check.sh` в статическом merge-gate). Вопрос аудита: гарантирует ли система
синхронную работу агентов — не может ли агент забыть обновить документацию и связанный код,
не нарушив заявленных правил.

**Снимок.** Audited tree — `origin/master @ 80b18f7` («universal chat tools, UNIFIED_ROUTER 3.2»).
Во время аудита `origin/master` ушёл вперёд минимум на 3 коммита; где находка уже исправлена
более новым коммитом вне audited tree, это отмечено явно. Аудит выполнен в изолированном
read-only worktree от `origin/master`; эмпирические тесты `docs-check.sh` — в отдельном
scratch-worktree с подсаженными коммитами (удалён после прогона). Ни один существующий файл
не изменён.

**Метод.** Каждое утверждение документов проверялось командой или чтением кода (grep по
идентификаторам, `git show --stat`, `bash -n`, прогон checker'а на подсаженных diff'ах).
Обратная проверка: поиск в коде потребителей/зеркал, НЕ перечисленных в карте.

---

## Общий вердикт

Система **в целом работоспособна, но не выполняет собственный инвариант**. По правилу самой
карты, «строка, не соответствующая коду, — дефект уровня бага» (`docs/DEPENDENCIES.md:10-11`);
аудит нашёл ~25 таких дефектов разной тяжести, включая прямые противоречия между четырьмя
входными документами (AGENTS.md, CLAUDE.md, CONTRIBUTING.md, BRANCHES.md) по критичным
правилам: кто удаляет worktree, допустимы ли прямые коммиты в master, из чего состоит
merge-gate. Агент, прочитавший разные документы, получит разные инструкции — это подрывает
цель «синхронной работы» сильнее, чем любой пропуск в карте.

Что подтверждено как работающее:

- Gate реально блокирует изменение контрактных поверхностей без документации (П.4, P1/P5).
- Культура «обнови инструкцию в том же коммите» уже живая: найти в свежей истории кодовый
  коммит вообще без `.md` не удалось с первых попыток (f4eecf1, 5c8c295 — оба с CLAUDE.md).
- Чеклисты «Изменение Control API» и «Новый алерт/метрика» полностью подтверждены эмпирикой.
- Индекс `docs/README.md` полон: 32/32 файла, без битых строк и непроиндексированных документов.
- Все 58 алертов имеют runbook-анкоры, и все 52 уникальных анкора имеют секции в MONITORING.md.
- Часть находок аудита уже исправлена на более новом `origin/master` (документ Platega —
  `407c9b3`, обратное направление sales feed — `9f47659`): система самокорректируется,
  но постфактум, а не на gate.

---

## П.1. Пути, команды и файлы, упомянутые в документах, существуют и исполняемы

**Вердикт: нарушено (7 находок), основная масса — сходится.**

Сходится:

- 60+ путей из AGENTS.md (документы, каталоги миграций, smoke-тесты, `.claude/hooks/guard-git.sh`
  — реально зарегистрирован как PreToolUse в `.claude/settings.json:3,9`) — все существуют.
- `bash -n` по всем 52 `deploy/*.sh` + `deploy/apitoken-db-dump` + 2 `tests/*.sh` — чисто.
- Все 30 markdown-ссылок `docs/README.md` ведут на существующие файлы.
- Node 24 / pnpm 9 подтверждены (`.node-version` 24.18.0, `engines >=24 <25`, `pnpm@9.7.0`).
- `docker compose up -d commerce-postgres` и `TEST_DATABASE_URL` из AGENTS.md:233 совпадают
  с `compose.yaml:2,4-9`; скрипт `test:integration` есть (`package.json:21`).
- AGENTS.md:197 обещает CLAUDE.md у 7 крейтов (registry, pool, forward, server, metering,
  authbot, router) — все 7 существуют.

Нарушено:

1. **`docs/DEPENDENCIES.md` §5** — нет `systemd/claude-api-anthropic@.service`, хотя это
   *текущий* юнит Anthropic-слотов: `deploy/engine-bluegreen.sh:87` `slot_unit() →
   claude-api-anthropic@`, а `claude-api@` — legacy (:88). Юнит появился 2026-07-29 (`cbe7984`),
   карта написана 2026-08-01 (`ce016f1`) — **карта родилась уже устаревшей**.
2. **`docs/DEPENDENCIES.md` §5** — в «исчерпывающем» списке infra-юнитов нет
   `apitoken-{sudoers,sysctl,tmpfiles}-install.service` (существуют в `systemd/`).
3. **CLAUDE.md:5** — «В каждом крейте есть свой вложенный `crates/<name>/CLAUDE.md`» — ложно
   для `crates/gemini-credential` и `crates/codex-credential` (AGENTS.md:196-199 при этом точен).
4. **BRANCHES.md:68-69 ↔ AGENTS.md** — прямое противоречие о том, кто удаляет worktree
   (подробнее в П.5, находка 2).
5. **CONTRIBUTING.md:35-43** — перечень «complete gate» не включает `deploy/docs-check.sh`,
   добавленный в gate коммитом `e1d39bd` (тот коммит обновил только AGENTS.md).
6. **AGENTS.md:238** — gate указан как голый `cargo test --locked --workspace` под лозунгом
   «ровно то, что прогоняет agent-merge.sh»; скрипт использует `deploy/sccache-cargo.sh`
   (`agent-merge.sh:116`) и path-aware lanes (подробнее в П.5, находка 1).
7. **Platega/DigiSeller рассинхрон @80b18f7** — дефолтный платёжный провайдер без
   интеграционного документа и строк в README/DEPENDENCIES; экспортированный, но не подключённый
   `digiseller.ts` (`packages/payments/src/index.ts`) описан как боевой. **Исправлено
   на текущем origin/master коммитом `407c9b3`** (вне audited tree).

Побочное наблюдение: комментарий `deploy/agent-merge.sh:135` про «agent-merge.test.sh»
устарел — файл удалён, и `deploy/agent-merge.suite.sh:522` явно требует его отсутствия.

---

## П.2. Каждая связь «производитель → контракт → потребители» из DEPENDENCIES.md подтверждена кодом

**Вердикт: нарушено (14 находок).** Таблицы раздела 1 в большинстве строк верны; границы БД
(раздел 4) подтверждены полностью. Основная масса дефектов — неполнота перечней и две
фактические лжи (pricing «dormant», B2C-зеркала).

### Раздел 1 — контракты между контекстами

Сходится:

- Строка 22 (`crates/server` → `/admin/*` под `CLAUDE_API_CONTROL_KEY`, только Combined/Anthropic;
  единственный клиент `packages/engine-client`): производитель (`http.rs:112+`, `admin.rs:1`,
  `config.rs:760`, merge admin-роутов только в Combined `http.rs:331` и Anthropic `http.rs:360`),
  клиент (`engine-client/src/index.ts:129-540`), обратная проверка (прямых обращений к `/admin/*`
  вне engine-client нет; в `apps/api` только тестовые моки), документ — всё подтверждено.
- Строка 23 (engine-client → api/worker/openkeys): `@claude-api/engine-client` ровно у трёх
  приложений; `ENGINE_BASE_URL`+`ENGINE_CONTROL_KEY` у каждого (`apps/api/src/config.ts:35-36`,
  `apps/worker/src/config.ts:6-7`, `apps/openkeys/src/lib/config.ts:99-100`).
- Строка 46 (`packages/contracts`): sweep всех package.json — импортируют ровно `apps/api`,
  `apps/worker`, `apps/openkeys`, `packages/db`, `packages/engine-client`; НЕ импортируют
  `apps/web`, `apps/sales-*`, `apps/admin` — точно как в карте.
- Строка 47 (публичный API → `apps/web`): `apps/web/src/lib/api.ts:1,171`.
- Строка 49 (OpenKeys админ API → `apps/admin`): route-handlers, `x-openkeys-control-key`
  (`internal-admin.ts:17`), Caddy (`Caddyfile:322-328`), `docs/product/OPENKEYS.md:80-81`.
- Строка 50 (sales-api → sales-web/admin): guard только `x-sales-admin-key`
  (`apps/sales-api/src/admin.guard.ts:16-23`), Caddy (`Caddyfile:329-332`).
- Примечание про локальные zod-схемы sales feed на обеих сторонах: `sync.service.ts:23-40`,
  `sales-feed.controller.ts:190-199`; в `packages/contracts` их нет.
- Раздел 4 (границы БД): `packages/db` — только api/worker; `packages/sales-db` — только
  sales-api; `packages/openkeys-db` — только openkeys; DSN движка (`CLAUDE_API_DATABASE_URL`,
  `crates/server/src/main.rs:529`) в TS не встречается ни разу.

Нарушено:

1. **DEPENDENCIES.md:24** — перечень operator-роутов неполон: через тот же домен и тот же
   ключ проксируются и потребляются `apps/admin` ещё `/codex-subs` (`Caddyfile:296-301` → 8792)
   и `/gemini-subs` (`Caddyfile:305-310` → 8794); `apps/admin/src/lib/sources.ts:9-10`.
2. **DEPENDENCIES.md:24** — у `/metrics` второй потребитель: Prometheus скрейпит его напрямую
   по loopback с Bearer-токеном (`observability/prometheus/prometheus.yml:36-45`); в разделе 1
   не отражено.
3. **DEPENDENCIES.md:36** — документ контракта `docs/sales/SALES_PORTAL.md:43-48` описывает
   только три GET-фида; POST `referral-discount` и `referral-profiles` в документе отсутствуют.
4. **DEPENDENCIES.md:37** — `GET partners/resolve` заявлен в контракте, но потребителя в коде
   нет (repo-wide grep находит только саму строку карты). Комментарий
   `apps/sales-api/src/internal.controller.ts:93` («commerce спрашивает при OAuth-регистрации»)
   коду не соответствует. Либо задокументировать как резерв, либо удалить эндпоинт из карты.
5. **DEPENDENCIES.md:37** — `SALES_PORTAL.md` @80b18f7 вообще не описывает обратное направление
   sales-api→commerce (ни `promo/redeem`, ни `partners/*`). **Исправлено на текущем
   origin/master коммитом `9f47659`** (вне audited tree).
6. **DEPENDENCIES.md:48** — неперечисленный потребитель `/v1/admin/*` под `x-admin-key`:
   `apps/content-studio` (`apps/content-studio/src/lib/api.ts:6` → `/v1/admin/content/*`;
   Caddy `Caddyfile:255-263` с тем же ключом). Content-studio отсутствует в разделе 1 карты.
7. **DEPENDENCIES.md:51** — в перечне адаптеров пропущен живой `DigiSellerProvider`
   (`packages/payments/src/index.ts:2-4`, регистрация `apps/api/src/payments.module.ts:21`),
   хотя его документ в той же строке указан — внутреннее противоречие строки. «Platega
   (дефолт)» подтверждено: `packages/contracts/src/index.ts:573` `paymentProviderSchema.default("platega")`.
8. Смежно (локальная инструкция контрактной поверхности): `apps/sales-api/README.md:54-58`
   документирует `x-admin-key` для `/v1/admin/*` — код принимает только `x-sales-admin-key`
   (`admin.guard.ts:16-19` явно отвергает `x-admin-key`). Ложная инструкция рядом с кодом —
   ровно то, что «живой контракт» обязан ловить.

### Разделы 2–3 — движок и зеркала моделей/цен

Сходится: `crates/metering` — authority цен (`lib.rs:104-142`, `codex.rs:83-90`,
`gemini.rs:40-41,123-134`); потребители metering ровно `crates/forward` и `crates/server`
(Cargo.toml); `crates/registry/src/pricing/` — durable-идентичности (doc-комментарий модуля
подтверждает); authbot на `127.0.0.1:8796` (`crates/authbot/src/main.rs:116`);
`CURRENT_*_CANONICAL_MODELS`/`B2C_PRICING_TIERS`/`CURRENT_PRODUCT_CATALOG_ENTRIES`
(`packages/contracts/src/index.ts:319,327,671,335`); шапка `apps/web/src/lib/models.ts:1-7`
требует синхронизации с metering; `assertOpenKeysCatalog` fail closed
(`openkeys-pricing.ts:111-135`, вызовы :202,:226); `PRODUCT_CATALOG` nanoUSD/bigint
(`calculation.ts:10,25,28`).

Нарушено:

9. **DEPENDENCIES.md:64 (раздел 2)** — `crates/forward/src/pricing*` назван «shadow-evaluation
   конвейер (dormant), рантайм-вызовов нет», но resolver/bridge вызываются в живом
   admission-пути: `crates/forward/src/proxy.rs:1217` (`resolve_pricing` в ветке `strict_policy`,
   влияет на допуск, :1228-1239), `crates/forward/src/codex/billing.rs:305`, bridge-префлайт
   `proxy.rs:1099-1145`; `PricingShadowRuntime` стартует в проде (`crates/server/src/main.rs:1354`).
   Даже собственный doc-комментарий `crates/forward/src/pricing.rs:1-4` («no runtime caller
   yet») устарел — карта воспроизвела устаревшую характеристику.
10. **Раздел 3** — два перечисленных зеркала одного authority противоречат друг другу:
    `apps/web/src/lib/pricing-tiers.ts:1-3` («плоская B2C-модель, единая скидка 50%, тиров нет»)
    против `packages/contracts/src/index.ts:671-677` (`B2C_PRICING_TIERS` — лестница 60–70%,
    активно используется биллингом `packages/db/src/pricing.ts:48-49,400-401,604,887-896`).
    `apps/web/src/app/changelog/page.tsx:15` заявляет «tiers retired». Это уже не
    документационный, а продуктовый рассинхрон — ровно тот дрейф, который карта обязана ловить.
11. **Раздел 3, обратная проверка** — неучтённые хардкоды цен: полная таблица «модель → цена»
    в `apps/web/src/components/marketing-pages.tsx:29-39` (дублирует `models.ts`, сегодня
    значения совпадают, но ничем не связано) и intro-ставки Sonnet 5 в
    `apps/web/src/components/cost-calculator.tsx:29-38` (локально, не из `models.ts`).

### Раздел 5 — инфраструктура

Сходится: все 11 строк таблицы доменов подтверждены Caddyfile (порты, blue-green слоты);
все перечисленные systemd-юниты существуют; оба rules-файла на месте; 58 алертов = 58
runbook-аннотаций; все 52 уникальных анкора имеют секции в `docs/ops/MONITORING.md`;
`deploy/watchdog-lib.sh` с классификаторами существует (`:506-561`, source из
`agent-merge.sh:75,207`).

Нарушено:

12. **Таблица доменов неполна**: в Caddyfile есть `mail./autodiscover./autoconfig.apitoken.sale`
    → `127.0.0.1:8080` (:249-251), `sales.apitoken.sale` (301-редирект, :380-382),
    `admin.partners.apitoken.sale` (managed auth, :428-438). По собственному правилу карты
    («новый домен = новая строка») это дефект.
13. **systemd**: `claude-api-anthropic@.service` не покрыт паттерном `claude-api{,-openai,-gemini}[@]`
    (см. П.1, находка 1).
14. **Доставка**: карта ссылается на `deploy/README.md` как на «полное описание»
    `agent-merge.sh`, но `deploy/README.md` вообще его не упоминает (grep пуст); описание
    есть только в `CONTRIBUTING.md:63`.

Оговорка (сходится по факту, слабее формулировки): `deploy/monitoring-config.test.sh`
механически проверяет runbook-анкоры только для ~15 закреплённых алертов из 58. Полную
согласованность сегодня подтверждает ручная сверка (этот аудит); гарантия AGENTS.md
«новый алерт без runbook-секции не пройдёт gate» механически не выполняется — новый алерт
вне закреплённого списка пройдёт gate без секции.

---

## П.3. Чеклисты CHANGE_CHECKLISTS.md покрывают реальные точки изменений

**Вердикт: нарушено (5 пробелов), 2 чеклиста из 7 полностью сходятся.**

Все ссылки во всех 7 чеклистах указывают на существующие файлы/символы (проверено каждое).
Эмпирическая сверка — по реальным коммитам соответствующих типов из истории до `80b18f7`
(коммиты старше введения чеклистов, поэтому «не прошёл чеклист» — не нарушение, а материал
для проверки полноты пунктов):

1. **«Новая модель» — пробел.** `apps/web/src/app/docs/integration-builder-data.ts` (а также
   `api-reference-data.ts`, `docs-portal.tsx`) затронуты в 2/2 коммитов типа (`cc47cd6` —
   Opus 5 + Fable 5, `af0b6a9` — Gemini в integration builder). Пункта про docs-портал/
   integration builder в чеклисте нет.
2. **«Изменение цены/мультипликатора» — пробел.** Engine-машинерия цен вне пунктов:
   `crates/forward/src/**/billing.rs` (2/3 коммитов типа: `1914307`, `746e489`),
   `packages/db/src/pricing-control-jobs.ts` (+970 строк в `1e1b622`),
   `apps/worker/src/pricing-worker.service.ts`, `packages/engine-client`. Чеклист называет
   инвариант «только durable jobs», но не указывает ни одного файла этой машинерии.
   Витринные числа на практике размазаны по ~20 файлам `apps/web` (`d66747c`: `learn-*.ts`,
   `messages.json`, `llms.ts`, `md-pages.ts`, `cost-calculator.tsx`, `pricing-overview.tsx`,
   `plans/page.tsx`) против двух указанных в чеклисте.
3. **«Новый провайдер подписки» — системный пробел.** В 2/2 коммитов типа (`f2407ef` —
   Gemini provider, 45 файлов; `2f37358` — OAuth provisioning) не покрыты: `crates/forward`
   (основной runtime провайдера — в пункте «Runtime» перечислены только Caddyfile/systemd/
   `crates/server`), `config.env.example`, deploy-скрипты (`watchdog.sh`, `watchdog-lib*.sh`,
   `engine-bluegreen.sh`, `sudoers.d`). `crates/authbot` (gemini_oauth.rs +1367 строк) и
   observability-стек (rules, grafana, blackbox, MONITORING.md) отсутствуют в чеклисте вовсе.
4. **«Изменение sales feed» — пробел + расхождение с протоколом.** `apps/sales-web`
   (`referrals/page.tsx`, `partner-analytics.tsx`, `lib/api.ts`) затронут в 2/2 коммитов типа
   (`d0142e9`, `cf9e774`) — пункта нет. Оба коммита меняли производителя и потребителя одним
   коммитом вопреки стадингу «производитель первым» (история старше протокола, зафиксировано
   как факт). `SALES_PORTAL.md` не обновлялся ни в одном из них — историческое подтверждение
   ценности пункта про документ.
5. **«Новый способ оплаты» — подтверждённое историей нарушение собственного пункта.**
   `docs/commerce/PLATEGA_INTEGRATION.md` отсутствовал @80b18f7 (`8375d1b` добавил Platega без
   документа; исправлено лишь в `407c9b3`, не-предке audited SHA). Живой дефолтный провайдер
   остался без обязательного по чеклисту документа — ровно тот класс забвения, который
   система должна ловить, и docs-gate его не поймал (см. П.4, находка P3). Контрпример:
   `a4b40e5` (Cryptomus) добавил оба интеграционных документа в том же коммите.

Полностью сходятся (и ссылки, и эмпирика): **«Изменение Control API»** (`2032d95`, `1914307` —
код + `CONTROL_API.md` в одном коммите, стадинг соблюдён) и **«Новый алерт/метрика»**
(`66bc7df`, `66efa83` — rules + MONITORING.md + источник метрики в одном коммите).

Не проверяемо: намеренность отсутствия `claude-opus-5`/`fable-5` в
`CURRENT_ANTHROPIC_CANONICAL_MODELS` при наличии в metering (`crates/metering/src/lib.rs:328-336`)
и web-каталоге — согласуется с §7 MULTI-DISCOUNT («не включается автоматически»), но из
истории намерение не подтвердить.

---

## П.4. deploy/docs-check.sh — эмпирическая проверка

**Вердикт: сходится по основной функции, нарушено по покрытию (4 находки).**

Прогоны (scratch-worktree @ `80b18f7`, подсаженные коммиты удалены после тестов):

| Тест | Diff | Ожидание | Факт |
|---|---|---|---|
| T1 | `ce016f1` (docs-only) | exit 0 | exit 0 — **сходится** |
| T2 | вызов `origin/master HEAD` из шапки скрипта | работает | exit 2 — **нарушено** |
| T4 | `2032d95` (http.rs + admin.rs + CONTROL_API.md) | exit 0 | exit 0 — **сходится** |
| P1 | подсажено: `crates/server/src/http.rs` без docs | exit 1 | exit 1, внятное сообщение — **сходится** |
| P2 | подсажено: `http.rs` + несвязанный `research/*.md` | — | exit 0 — **любой .md удовлетворяет gate** |
| P3 | подсажено: `packages/payments/src/platega.ts` без docs | — | exit 0, только warning — **нарушено** |
| P4 | подсажено: `crates/metering/src/lib.rs` без docs | — | exit 0, только warning — **нарушено** |
| P5 | подсажено: новый файл в `packages/db/migrations/` без docs | exit 1 | exit 1 — **сходится** |

Сходится:

- Синтаксис чист (`bash -n`); вызов из `am_gate_static` передаёт resolved 40-char SHA
  (`agent-merge.sh:151`, base/target из `rev-parse` :572,:584) — прод-вызов корректен.
- Блокирует главные классы: Control API-роуты, миграции, `packages/contracts`, оба
  internal-контроллера sales feed.
- Текст ошибки внятный и отсылает к чеклистам и карте.

Нарушено:

1. **Шапка скрипта противоречит собственной валидации**: пример `bash deploy/docs-check.sh
   origin/master HEAD` (`docs-check.sh:11`) отклоняется проверкой полных SHA (:21) с exit 2.
2. **Любой `.md` снимает блокировку** (P2): контрактное изменение + правка несвязанного
   markdown проходит. Gate эвристический — но это стоит зафиксировать как известное
   ограничение, а не как гарантию.
3. **`packages/payments` не входит в контрактные поверхности** (P3), хотя чеклист «Новый
   способ оплаты» требует документ + строку в карте, а сам текст ошибки checker'а называет
   «способ оплаты» среди причин блока. Историческое подтверждение дыры: Platega прошла без
   документа (П.3, находка 5).
4. **`crates/metering` — authority цен по карте — не входит в контрактные поверхности** (P4):
   изменение цен без документации проходит с warning.

Покрытие против обещаний: AGENTS.md («Проверка») заявляет «контрактные поверхности без
изменений в документации не пройдут». Фактические поверхности — contracts, 4 каталога
миграций, `http.rs`/`admin.rs`, 2 sales-контроллера. Платежи, цены, алерты, новые провайдеры —
только warning. Либо список расширяется, либо формулировка смягчается.

---

## П.5. Противоречия между документами; полнота индекса docs/README.md

**Вердикт: нарушено (7 противоречий). Индекс — сходится полностью. Карта репозитория в
AGENTS.md — сходится с реальным layout.**

Сходится:

- Порядок слоёв `registry ← pool ← forward ← server` един во всех документах; роли
  metering/authbot/router/credential-крейтов «вне слоёв» согласованы.
- Порты/URL: router 8798, admin 3700, openkeys 3410, plane origins 8790/8792/8794 — едины
  между AGENTS.md, crates/*/CLAUDE.md, Caddyfile, systemd-юнитами.
- Правила миграций (AGENTS.md:243-251 = `packages/db/MIGRATIONS.md`:6-11,30-35 =
  CONTRIBUTING.md:27-32); все 4 каталога миграций существуют.
- Инвариант денег (integer/nanoUSD) един: CLAUDE.md:47, AGENTS.md:219,
  `crates/metering/CLAUDE.md`:11, `crates/router/CLAUDE.md`:32.
- **Индекс docs/README.md полон**: 32 файла в `docs/` — 32 строки индекса, 1:1, без битых
  ссылок; секция «Рядом с кодом» тоже валидна. На текущем origin/master полнота сохраняется
  (PLATEGA_INTEGRATION.md проиндексирован).
- Карта репозитория AGENTS.md: все 8 apps, 9 crates, 6 packages покрыты без умолчаний
  и неверных описаний.

Нарушено (противоречия — агент получит разные инструкции в зависимости от прочитанного):

1. **Состав merge-gate расходится в трёх документах.** AGENTS.md:236-241 — линейный список
   «ровно то, что прогоняет agent-merge.sh»; реальный скрипт path-aware: build через
   `deploy/typescript-build-contexts.sh` (не `pnpm build`), тесты через
   `deploy/typescript-test-groups.sh` (не `pnpm test`), cargo под `deploy/sccache-cargo.sh`,
   плюс 9 deploy-сьютов в deployment-lane, о которых AGENTS.md умалчивает; lanes выбираются
   по diff (`agent-merge.sh:65-227`). CONTRIBUTING.md:34-43 точнее, но не включает
   `deploy/docs-check.sh`, который прогоняется всегда.
2. **Кто удаляет worktree.** BRANCHES.md:68 — «worktree убирает человек, не агент»;
   AGENTS.md:286-306 и CLAUDE.md:135-136 — «агент обязан удалить свой worktree и ветку».
3. **Прямые коммиты в master.** CLAUDE.md:105-106 и BRANCHES.md:11,44 разрешают для доков/
   кросс-компонентной работы; CONTRIBUTING.md:64 («only supported way to reach master»)
   и AGENTS.md:268-284 — только через `agent-merge.sh`.
4. **CLAUDE.md:5** — «в каждом крейте есть CLAUDE.md» — ложно (см. П.1, находка 3).
5. **База task-ветки.** AGENTS.md:82 — всегда от `origin/master`; BRANCHES.md:58 — типовой
   цикл от `origin/comp/forward`; CONTRIBUTING.md:12-21 допускает оба. Сами `origin/comp/*`
   ветки реальны (с BRANCH.md на каждой) — расходятся только предписания.
6. **BRANCHES.md, мелочи**: пример однострочного `git commit -m "forward: …"` (:63) при
   запрете AGENTS.md:118-120; сниппет создания comp-веток (:74-76) без authbot.
7. **CLAUDE.md:124** — ссылка на smoke-пример «в истории/README.md» мертва: примера нет ни в
   текущем README, ни в его истории (`git log -S` пуст). Реальные примеры —
   `tests/rotation_fanout_smoke.sh`, `tests/universal_chat_smoke.sh`.

---

## Сводная таблица находок

| # | Сeverity | Находка | Где |
|---|---|---|---|
| 1 | critical | Прямые коммиты в master: CLAUDE/BRANCHES разрешают, CONTRIBUTING/AGENTS запрещают | П.5.3 |
| 2 | critical | Удаление worktree: человек vs агент | П.5.2 |
| 3 | critical | `crates/forward/src/pricing*` назван dormant, но в живом admission-пути | П.2.9 |
| 4 | critical | B2C-зеркала противоречат друг другу (flat 50% vs тиры 60–70%, используемые биллингом) | П.2.10 |
| 5 | critical | Gate не покрывает платежи: Platega прошла без обязательного документа (прецедент) | П.3.5 + П.4.3 |
| 6 | high | Состав merge-gate расходится в трёх документах; CONTRIBUTING без docs-check.sh | П.5.1 |
| 7 | high | Карта родилась устаревшей: нет `claude-api-anthropic@` (текущий slot-unit) | П.1.1 |
| 8 | high | content-studio — неперечисленный потребитель `/v1/admin/*` под `x-admin-key` | П.2.6 |
| 9 | high | Operator-роуты неполны (`/codex-subs`, `/gemini-subs`); `/metrics` — второй потребитель Prometheus | П.2.1-2 |
| 10 | high | `apps/sales-api/README.md` документирует неверный заголовок (`x-admin-key`) | П.2.8 |
| 11 | high | Чеклист «Новый провайдер» не покрывает forward/authbot/deploy/observability (2/2 коммитов) | П.3.3 |
| 12 | high | Чеклист «Цена» не покрывает engine-машинерию и размазанную витрину (3/3 коммитов) | П.3.2 |
| 13 | high | `crates/metering` (authority цен) не в контрактных поверхностях gate | П.4.4 |
| 14 | high | Гарантия «алерт без runbook не пройдёт gate» механически не выполняется (~15 из 58) | П.2 (оговорка) |
| 15 | medium | `partners/resolve` заявлен в контракте без потребителя; комментарий в коде врёт | П.2.4 |
| 16 | medium | Таблица доменов карты неполна (mail/*, sales redirect, admin.partners) | П.2.12 |
| 17 | medium | `deploy/README.md` не описывает agent-merge.sh («полное описание» мертво) | П.2.14 |
| 18 | medium | Чеклисты «Новая модель» и «Sales feed» без docs-портала и sales-web (2/2 коммитов) | П.3.1, П.3.4 |
| 19 | medium | Шапка docs-check.sh противоречит собственной валидации (usage-example → exit 2) | П.4.1 |
| 20 | medium | DigiSeller: живой адаптер пропущен в строке карты, документ указан | П.2.7 |
| 21 | medium | Любой `.md` снимает блокировку gate (известное ограничение эвристики) | П.4.2 |
| 22 | low | CLAUDE.md:5 «в каждом крейте CLAUDE.md» — ложно для credential-крейтов | П.1.3 |
| 23 | low | База task-ветки: origin/master vs origin/comp/* — предписания расходятся | П.5.5 |
| 24 | low | BRANCHES.md: однострочный commit-пример; сниппет без authbot | П.5.6 |
| 25 | low | CLAUDE.md:124 — мёртвая ссылка на smoke-пример | П.5.7 |
| 26 | low | infra-юниты sudoers/sysctl/tmpfiles не в списке карты | П.1.2 |
| — | fixed upstream | Platega без документа; SALES_PORTAL без обратного направления; POST-эндпоинты фида без документа | П.1.7, П.2.5 (407c9b3, 9f47659) |

## Рекомендации (в порядке отдачи)

1. **Снять противоречия входных документов** (находки 1, 2, 6): выбрать единый источник
   истины для пути в master, уборки worktree и состава gate; привести CLAUDE.md, BRANCHES.md,
   CONTRIBUTING.md к нему. Это дешевле всего и бьёт прямо в цель «синхронной работы».
2. **Расширить контрактные поверхности docs-check.sh** (находки 5, 13): добавить
   `packages/payments/*` и `crates/metering/*`; исправить usage-пример в шапке (находка 19);
   зафиксировать ограничение «любой .md» в шапке скрипта.
3. **Обновить DEPENDENCIES.md по находкам П.2** (7, 8, 9, 15, 16, 17, 20) — включая
   исправление строки про dormant pricing (находка 3) и дополнение operator-роутов.
4. **Разрешить B2C-рассинхрон** (находка 4): это продуктовое решение (flat 50% vs тиры),
   не документационное — зафиксировать одно и привести другое.
5. **Дополнить чеклисты по эмпирике** (находки 11, 12, 18): forward/authbot/deploy/
   observability в «Новый провайдер»; engine-машинерию в «Цена»; docs-портал в «Новая модель»;
   sales-web в «Sales feed».
6. **Monitoring-gate**: либо расширить `monitoring-config.test.sh` на все 58 алертов
   (механический перебор анкоров вместо закреплённого списка), либо смягчить формулировки
   в AGENTS.md/карте (находка 14).
7. **`partners/resolve`** (находка 15): удалить из карты и кода или задокументировать как
   зарезервированный — по протоколу «удаление последним шагом».

## Не проверяемо (зафиксировано по стоп-правилу)

1. Совпадение прод-значения `ADMIN_CONTROL_KEY` (живёт только в `/etc/caddy/Caddyfile` на
   хосте, `deploy/CADDY.md:7`) с engine-ключом — из репозитория не проверить; механизм
   наследования `render-caddy.awk` корректен.
2. Живость GitHub status contexts (`deploy/watchdog`, `deploy/migration`) — не файлы
   репозитория; документированы согласованно (CONTRIBUTING.md:131-139).
3. Намеренность отсутствия `claude-opus-5`/`fable-5` в `CURRENT_ANTHROPIC_CANONICAL_MODELS`
   (см. П.3).
4. Содержимое `BRANCH.md` на `origin/comp/*` — существование подтверждено, содержимое вне
   дерева master.

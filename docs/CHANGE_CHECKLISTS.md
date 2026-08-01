# CHANGE_CHECKLISTS.md — чеклисты кросс-функциональных изменений

Типовые изменения, которые невозможно сделать в одном месте: у каждого есть зеркала и
зависимые участки в других контекстах. Карта самих связей — `docs/DEPENDENCIES.md`; правила
изменения контрактов — раздел «Контракты между контекстами» в корневом `AGENTS.md`.

**Как пользоваться.** Если твоё изменение подпадает под один из типов ниже — пройди чеклист
ЦЕЛИКОМ до коммита и укажи в body коммита, какой чеклист применён (например: «Чеклист: новая
модель — все пункты пройдены» или «пункт OpenKeys неприменим, модель Anthropic-only»).
Пункт нельзя молча пропустить: либо выполнен, либо явно помечен неприменимым с причиной.
Чеклист — минимум, а не потолок: если по `docs/DEPENDENCIES.md` у изменения есть ещё
потребители, они тоже входят в diff или в отчёт.

## Новая модель (в существующем провайдере)

- [ ] `crates/metering/src/{lib,codex,gemini}.rs` — тарифная таблица (authority цен, nanoUSD).
- [ ] `packages/contracts` — `CURRENT_*_CANONICAL_MODELS` и/или pricing-схемы.
- [ ] `apps/web/src/lib/models.ts` — SEO-каталог (шапка файла требует синхронизации с
      `crates/metering`).
- [ ] `apps/web/src/app/docs/` — docs-портал: `integration-builder-data.ts` и, если модель
      видна в справке, `api-reference-data.ts` / `docs-portal.tsx`.
- [ ] `docs/engine/<провайдер>.md` — список моделей провайдера.
- [ ] `docs/commerce/MULTI-DISCOUNT.md` §7 — новая модель НЕ включается автоматически:
      нужна явная catalog generation (каталоги/свитчи/политики в `crates/registry/src/pricing/`
      через versioned pricing-протокол Control API).
- [ ] `apps/openkeys` — `assertOpenKeysCatalog()` сверяется с `CURRENT_PRODUCT_CATALOG_ENTRIES`
      из `packages/contracts`: без обновления каталога OpenKeys fail closed.
- [ ] `docs/commerce/PRICING.md` — если модель меняет клиентский прайсинг.
- [ ] `apps/admin` — если модель видна в квотах/калькуляторе (`subscriptions`,
      `sales/calculator/calculation.ts`).

## Изменение цены или мультипликатора

- [ ] `crates/metering` — authority-таблица (официальные цены провайдера), ревьюимый коммит.
- [ ] Engine multiplier меняется только durable jobs — не разовыми вызовами (инвариант из
      `CLAUDE.md`); протокол — versioned pricing в `docs/engine/CONTROL_API.md`. Машинерия:
      `packages/db/src/pricing-control-jobs.ts`, `apps/worker/src/pricing-worker.service.ts`,
      `packages/engine-client` (ledger/ack), исполнение в `crates/forward/src/**/billing.rs`.
- [ ] `packages/contracts` — `B2C_PRICING_TIERS` / pricing-схемы; `packages/db/src/pricing.ts`.
- [ ] `apps/web` — витринные числа: `src/lib/pricing-tiers.ts` + `src/lib/models.ts` и вся
      витринная копия (`src/components/marketing-pages.tsx`, `src/components/cost-calculator.tsx`,
      `src/lib/md-pages.ts`, `src/lib/messages.json`, `src/lib/llms.ts`, `src/lib/learn-*.ts`).
      Радиус проверять grep'ом по старому числу, а не по памяти.
- [ ] `docs/commerce/PRICING.md` — клиентский прайсинг и тиры.
- [ ] Партнёрские расчёты: `docs/sales/SALES_PAYOUT_PERIODS.md`, логика `apps/sales-api` —
      если цена входит в базу выплат.
- [ ] `apps/admin/src/app/sales/calculator/calculation.ts` — `PRODUCT_CATALOG`.

## Новый провайдер подписки

Полный порядок research → credential/Auth Bot → runtime → money/calibration → admin → blue-green →
live GA: `docs/engine/PROVIDER_ONBOARDING.md`. Чеклист ниже — индекс обязательного радиуса, а не
замена phase gates и Definition of GA из playbook.

- [ ] Credential-крейт `crates/<provider>-credential` (шифрованные OAuth-конверты, без сети)
      + `crates/<name>/CLAUDE.md` по правилам «живого контракта».
- [ ] `crates/metering/src/<provider>.rs` — тарифная таблица.
- [ ] Runtime движка: транспорт/пул/биллинг провайдера в `crates/forward` (основной объём кода),
      режим в `crates/server`, слоты/порты в `deploy/Caddyfile`, systemd-юниты.
- [ ] Пополнение пула: OAuth-provisioning в `crates/authbot` (если провайдер подписочный).
- [ ] Sticky/unlimited-parallel runtime: нет локальной очереди/semaphore/reject; retry только до
      первого public byte; disconnect drain сохраняет terminal usage и settlement.
- [ ] Durable reserve/delivering/settlement + exact immutable turn evidence; official API nanoUSD и
      native subscription credits ведутся раздельно.
- [ ] Калибровка каждого native window по Codex fixed-point/raw-evidence контракту; exact plan +
      duration cohorts, null до evidence, без nominal/prior/EMA.
- [ ] Exact turn delivery имеет bounded FIFO, idempotent replay, conflict quarantine, pending/drop
      diagnostics и shutdown drain; quota snapshot не может обогнать failed spend event.
- [ ] Доставка: `config.env.example`, deploy-скрипты (`watchdog.sh`, `watchdog-lib*.sh`,
      `engine-bluegreen.sh`, `sudoers.d`) — новые порты, юниты и секреты провайдера.
- [ ] Observability: метрики и алерты в `observability/prometheus/rules/*`, дашборды Grafana,
      runbook-секции в `docs/ops/MONITORING.md` (см. чеклист «Новый алерт или метрика»).
- [ ] `docs/engine/<PROVIDER>_PROVIDER.md` — новый документ + строка в `docs/README.md`.
- [ ] `docs/commerce/MULTI-DISCOUNT.md` — каталоги, рубильник провайдера (§8), политики.
- [ ] `packages/contracts` — canonical models, продуктовые каталоги.
- [ ] `apps/web` (витрина), `apps/openkeys`, `apps/admin` — отображение и продажа.
- [ ] `apps/admin` повторяет актуальную семантику GPT capacity board (credits/capacity/used,
      token-capacity, два API-$ сценария, profitability, consumed-quota bar, masked identity),
      адаптируя units/tiers/windows провайдера.
- [ ] Controlled live matrix каждого опубликованного plan/model/tier + public post-deploy smoke;
      exact landed SHA имеет `deploy/watchdog` GREEN.
- [ ] Строка в `docs/DEPENDENCIES.md`.

## Изменение Control API (движок ↔ коммерция/OpenKeys)

Контракт expand-only — см. протокол в `AGENTS.md`. Порядок: сначала производитель (движок),
потребители — после зелёного `deploy/watchdog` на SHA производителя.

- [ ] `crates/server/src/http.rs` + `src/admin.rs` — роуты/хендлеры.
- [ ] `docs/engine/CONTROL_API.md` — В ТОМ ЖЕ коммите, что и код движка.
- [ ] `packages/contracts` — zod-схемы новых/расширенных сообщений.
- [ ] `packages/engine-client` — методы клиента (отдельным шагом после деплоя движка).
- [ ] Потребители по `docs/DEPENDENCIES.md`: `apps/api`, `apps/worker`, `apps/openkeys`
      (после деплоя движка). `apps/admin` идёт через Caddy-прокси — отдельно проверить
      operator-роуты.

## Изменение sales feed (коммерция ↔ партнёрка)

Контракт expand-only; типы продублированы локальными zod-схемами на ОБЕИХ сторонах — правятся
обе.

- [ ] Производитель `apps/api/src/sales-feed.controller.ts` (или `apps/sales-api/src/internal.controller.ts`
      для обратного направления) — первым, по протоколу контрактов.
- [ ] Потребитель `apps/sales-api` (`sync.service.ts`, `commerce.service.ts`) или `apps/api`
      (`promo.service.ts`, `auth.service.ts`) — после деплоя производителя.
- [ ] `apps/sales-web` — партнёрский фронтенд (`referrals`, `partner-analytics`, `lib/api.ts`),
      если изменение фида видно партнёру.
- [ ] `docs/sales/SALES_PORTAL.md` — раздел «Граница sales ↔ commerce» в том же коммите.
- [ ] Строка sales feed в `docs/DEPENDENCIES.md` — если меняется состав эндпоинтов.

## Новый способ оплаты

- [ ] `packages/payments/src/<provider>.ts` — адаптер + регистрация в реестре
      (`apps/api/src/payments.module.ts`, env-фабрика).
- [ ] `PaymentProviderCode` и чекаут-схемы: `apps/api/src/checkout.service.ts`,
      `packages/contracts`.
- [ ] Вебхук в `apps/api/src/payments.controller.ts` (+ исключение из origin-гарда) и/или
      reconcile-поллинг в `apps/worker`.
- [ ] `docs/commerce/<PROVIDER>_INTEGRATION.md` — новый документ + строка в `docs/README.md`.
- [ ] `apps/web` — выбор провайдера на чекауте; `apps/admin` — финансовые витрины.
- [ ] Строка в `docs/DEPENDENCIES.md`.

## Новый алерт или метрика

- [ ] `observability/prometheus/rules/{application,operations}.yml` — алерт с аннотацией
      `runbook: 'docs/ops/MONITORING.md#<alert>'`.
- [ ] `docs/ops/MONITORING.md` — секция `## <Alert>` В ТОМ ЖЕ коммите (без неё не пройдёт
      `deploy/monitoring-config.test.sh`).
- [ ] Если метрика новая — коллектор должен её экспортировать (проверяет тот же скрипт).

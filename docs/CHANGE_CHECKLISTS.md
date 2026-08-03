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

Публикация двухэтапная. Implementation/research мёржится первым и остаётся dormant: на этом шаге
запрещено добавлять модель в production defaults/systemd, публичный model catalog, router presets,
сайт и публичные docs. После GREEN exact implementation SHA выполняется controlled production live
на owned credential. Бесплатный `countTokens` идёт первым, затем минимальный generation; aggregate
admission budget по умолчанию не больше `$0.0001` (0,01 цента). Quota/catalog row и `countTokens`
не доказывают generation. Публикационный gate требует одновременно generation 2xx, реальный output,
terminal authoritative usage, incremental SSE и все заявленные controls. Только после этого
отдельный publication-коммит проходит публичную половину чеклиста. Любой failed generation означает
withdrawal: публичные поверхности снимаются, а immutable/dormant artifacts не переписываются.

- [ ] Research/implementation commit: официальный model/price/control contract, точный private wire
      mapping и controlled canary path; runtime implementation по умолчанию dormant.
- [ ] `crates/metering/src/{lib,codex,gemini}.rs` — тарифная таблица (authority цен, nanoUSD).
- [ ] `packages/contracts` — `CURRENT_*_CANONICAL_MODELS` и/или pricing-схемы.
- [ ] GREEN exact implementation SHA + live gate: generation/output/usage/SSE/controls подтверждены
      на каждой заявленной subscription plan/model tier; sanitized evidence записано в provider doc.
- [ ] Publication commit не смешан с implementation commit; при failed live вместо него выполнен
      withdrawal из всех ошибочно затронутых public/default поверхностей.
- [ ] `apps/web/src/lib/models.ts` — SEO-каталог (шапка файла требует синхронизации с
      `crates/metering`); выполняется только в publication commit.
- [ ] `apps/web/src/app/docs/` — docs-портал: `integration-builder-data.ts` и, если модель
      видна в справке, `api-reference-data.ts` / `docs-portal.tsx`; только после live gate.
- [ ] Production defaults/systemd и `crates/router/routing-presets.json` — только после live gate.
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
- [ ] `packages/contracts` — global/provider/model discount и pricing release schemas;
      `packages/db/src/pricing.ts`. `B2C_PRICING_TIERS` не сохранять как target authority.
- [ ] `apps/web` — витринные числа: удалить зависимости от `src/lib/pricing-tiers.ts`, проверить
      `src/lib/models.ts` и всю
      витринная копия (`src/components/marketing-pages.tsx`, `src/components/cost-calculator.tsx`,
      `src/lib/md-pages.ts`, `src/lib/messages.json`, `src/lib/llms.ts`, `src/lib/learn-*.ts`).
      Радиус проверять grep'ом по старому числу, а не по памяти.
- [ ] `docs/commerce/PRICING.md` — global/provider/model pricing, B2B/OpenKeys/service и bonus.
- [ ] `docs/commerce/MULTI-DISCOUNT.md` + Stage 5/6/8/9 — target/recovery release, full inventory,
      100% shadow и one-head activation; per-account canary/maintenance rollout не добавлять.
- [ ] Production Stage 5/6 запускается только защищённым AdminGuard producer API: verified actor,
      fresh exact plan digest, meaningful mutation reason, attributed audit и strict status;
      package CLI/ручной SSH не считать operator surface. UI consumer подключать отдельным
      коммитом только после GREEN producer SHA.
- [ ] Terminal pre-cutover `strict + legacy_single` delivery восстанавливается только exact-CAS
      `/v1/admin/pricing-policy-delivery-repairs`: старый payload не переписывать, generic dead job
      не ретраить и commerce rows вручную не исправлять.
- [ ] B2B current discount остаётся independent Anthropic rule; OpenKeys остаётся 1:1; service
      остаётся `meter_only` и all-model. Явно отметить неприменимые классы.
- [ ] Партнёрские расчёты: `docs/sales/SALES_PAYOUT_PERIODS.md`, логика `apps/sales-api` —
      если цена входит в базу выплат.
- [ ] Sales feed/commission: exact `paid_funded_nano` не должен зависеть от pricing mode; welcome
      bonus исключается. При изменении wire schema пройти отдельный чеклист sales feed.
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
- [ ] `apps/admin` добавляет провайдера в единый компактный fleet control-room и одну account-table:
      реальные provider windows, exact remaining/full API-$, used rail, readiness/coverage и masked
      identity. Raw token/profitability/cache/quota-bucket matrices остаются backend/report evidence,
      а pending/degraded authority скрывает saleable money вместо показа stale `$`.
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

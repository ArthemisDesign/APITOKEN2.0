# Журнал внедрения практик DeepSeek Harness

> **Статус:** активный рабочий журнал.
>
> **Исходное предложение:** [DEEPSEEK_HARNESS_DEVELOPMENT_PRACTICES.md](DEEPSEEK_HARNESS_DEVELOPMENT_PRACTICES.md).
>
> **Цель:** внедрить первый рекомендуемый пакет, сохранив действующие managed worktrees, path-aware merge gate, exact-SHA validation, expand-only контракты и production watchdog как источники истины.

## Definition of Done

Пакет завершён, когда все пункты ниже находятся в `master`, прошли локальные релевантные проверки и получили GREEN `deploy/watchdog` на точных SHA:

1. Существует assembled acceptance-сценарий, который запускает собранный `claude-api`, подключает реальный `packages/engine-client` и disposable PostgreSQL, затем проверяет основные операции Control API и отрицательный случай контракта.
2. Существует keyless router → engine → mock-upstream replay для non-stream и SSE поведения. Merge gate работает read-only, проверяет стабильный transcript и отдельные семантические условия.
3. Developer-facing change-plan команда переиспользует production path classifiers, показывает base/head, выбранные bounded contexts, validation lanes, специальные проверки и документационных владельцев. Неизвестные пути включают полный fail-closed план.
4. Always-on static lane проверяет выбранные архитектурные инварианты: запрещённые зависимости нижних Rust-слоёв, production env ownership и запрещённые обходы TypeScript bounded-context контрактов.
5. Документационный gate проверяет относительные Markdown links/anchors, полноту `docs/README.md` и конкретных документационных владельцев для Control API, metering/pricing, payments, sales feed и alerts. Несвязанный `.md` не удовлетворяет этим требованиям.
6. В репозитории есть лёгкий шаблон incident → guardrail. Он отличает incident postmortem от широкого audit и требует ссылку на исполняемую регрессию для закрытого инцидента.
7. Настоящий журнал содержит точные landed SHA, выполненные проверки, открытые ограничения и один следующий шаг.

## Границы реализации

- Не добавлять вторую независимую систему классификации путей. Новый change plan и test selection используют функции из `deploy/watchdog-lib.sh` и envelope из `deploy/validation-plan.sh`.
- Не менять production deployment semantics ради developer UX.
- Не вводить глобальный 100% coverage gate.
- Не переносить Cordis/plugin архитектуру, ACP/GUI corpus, bilingual pairing или платформенную матрицу DeepSeek Harness.
- Не выполнять paid/live provider calls: replay и assembled acceptance должны быть keyless.
- Изменения схемы БД не планируются. Если необходимость появится, миграция доставляется отдельно по migration-first правилу.
- Изменение межконтекстного wire-контракта не планируется. Acceptance должен проверить действующий контракт, а не расширить его.

## План доставки

| Этап | Содержание | Состояние | Landed SHA | Проверки / доказательство |
|---|---|---|---|---|
| 0 | Исходное предложение и этот журнал | Завершён | `e2eab16b9a9541c3777fb635cbbfa00579c3d2ad` | UTF-8/newline, whitespace, `deploy/docs-check.sh`; exact SHA GREEN `deploy/watchdog` |
| 1 | Change-plan интерфейс и repository invariants | Завершён | `973f21a3c3df790dcf71559c9ee50d5010c97984` | Полный TypeScript/Rust/deployment/static gate; exact SHA GREEN `deploy/watchdog` |
| 2 | Targeted docs ownership, links/anchors/index | Завершён | `15643546ee710cc5a38f1a1873fcba4690cce6e6` | Полный TypeScript/Rust/deployment/static gate; exact SHA GREEN `deploy/watchdog` |
| 3 | Assembled Control API ↔ EngineClient acceptance | Готов к trusted-host merge | — | Built package export and orchestration syntax verified locally; real disposable-PostgreSQL execution requires trusted host because local Docker is unavailable |
| 4 | Keyless router → engine replay | Не начат | — | Non-stream + SSE transcript, semantic guards, repeatability |
| 5 | Incident → guardrail template и финальная документация | Не начат | — | Documentation checks, ссылка из process contract |
| 6 | Финальная совместная проверка и closeout | Не начат | — | Полный выбранный gate, точные GREEN SHA |

## Журнал решений

### 2026-08-22 — первый пакет остаётся набором малых merge

Один большой коммит смешал бы merge-policy tooling, документационный gate, Rust/TypeScript integration harness и test fixtures. Это затруднило бы review и сделало причину возможного production RED неоднозначной. Пакет доставляется последовательными небольшими merge. Каждый следующий этап стартует от свежего `origin/master` после GREEN предыдущего SHA.

### 2026-08-22 — существующий production selector остаётся authority

DeepSeek Harness использует отдельные `change-scope.ts` и gate DAG, но наш production pipeline уже имеет `deploy/validation-plan.sh` и `deploy/watchdog-lib.sh`. Новый developer-facing интерфейс будет адаптером над ними. Дублирование path rules запрещено.

### 2026-08-22 — replay дополняет, а не заменяет семантические assertions

Golden transcript может стабильно зафиксировать неправильный ответ. Поэтому replay считается успешным только вместе с отдельными проверками terminal usage, обязательных SSE событий, непустого результата и ожидаемого error contract.

## Текущее состояние

- Завершено: предложение/журнал — GREEN `e2eab16b9a9541c3777fb635cbbfa00579c3d2ad`; change plan/invariants — GREEN `973f21a3c3df790dcf71559c9ee50d5010c97984`; targeted docs gate — GREEN `15643546ee710cc5a38f1a1873fcba4690cce6e6`; worktree каждого этапа удалён штатно.
- В работе: этап 3 на свежем `origin/master` `15643546ee710cc5a38f1a1873fcba4690cce6e6`; built binary/package-export acceptance подключён после Rust artifact lane и до остановки disposable PostgreSQL.
- Проверено локально: `pnpm install --frozen-lockfile`; сборка `@claude-api/contracts` и `@claude-api/engine-client`; `node --check` acceptance client; shell syntax; classifier/ordering assertions. Локальный Docker daemon недоступен, поэтому реальное PostgreSQL/HTTP исполнение остаётся обязательной частью trusted-host exact-SHA validation, а не заявляется как локально пройденное.
- Блокеры: отсутствуют; trusted host предоставляет Docker-backed PostgreSQL, release artifact и CI account.
- Следующее действие: прогнать deployment/static regression suites, слить этап 3 и использовать trusted-host verdict как первое полное assembled доказательство.

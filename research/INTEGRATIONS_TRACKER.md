# Журнал внешних интеграций apiToken.sale

Этот файл — постоянная операционная история работы по `AGENT_TASK_INTEGRATIONS.md`. Он нужен,
чтобы не повторять уже сделанную разведку, возвращаться к отложенным ревью и регулярно проверять,
появилась ли ссылка на `apitoken.sale` в upstream-репозитории или опубликованной документации.

## Как обновлять журнал

Для каждой цели фиксировать:

- upstream, default branch, дату и SHA разведки;
- правила контрибьюции и 1–2 принятых аналога;
- выбранный минимальный механизм интеграции и затронутые пути;
- локальные тесты, живые проверки и известные baseline-сбои;
- fork/branch/commit/PR, CLA, CI и все замечания ревью;
- URL ожидаемого брендового упоминания;
- дату последней проверки upstream и опубликованных страниц;
- результат backlink-проверки: `pending`, `present`, `removed` или `not-applicable`.

После мержа проверять ссылку минимум в четырёх местах: merged diff PR, default branch upstream,
публичная документация/релиз и GitHub code search по `apitoken.sale`. Если публикация зависит от
релиза, записывать отдельно дату мержа и дату появления на публичном сайте.

## Сводка

| Цель | Состояние | PR | Упоминание в upstream | Публичная ссылка | Последняя проверка |
|---|---|---|---|---|---|
| `BerriAI/litellm` | open, safe to merge | [#34915](https://github.com/BerriAI/litellm/pull/34915) | pending merge | pending | 2026-07-28 |
| `musistudio/claude-code-router` | PR open; review pending | [#1598](https://github.com/musistudio/claude-code-router/pull/1598) | pending merge | absent | 2026-07-28 |
| `LibreChat-AI/librechat.ai` | queued | — | — | — | — |
| `QuantumNous/new-api` | queued | — | — | — | — |
| `Aider-AI/aider` | queued | — | — | — | — |
| `cline/cline` | queued | — | — | — | — |
| `RooCodeInc/Roo-Code` | queued | — | — | — | — |
| `continuedev/continue` | queued | — | — | — | — |

## BerriAI/litellm

- Эталонный PR: <https://github.com/BerriAI/litellm/pull/34915>.
- Текущий итог: 5/5 `safe to merge`, открытых замечаний нет.
- Ожидаемое упоминание: регистрация провайдера и связанные тесты/документация из PR.
- Backlink status: `pending`, пока PR не смержен в default branch.
- Подробная хронология: `research/LITELLM_PR.md`.

## musistudio/claude-code-router

### Разведка

- Upstream: <https://github.com/musistudio/claude-code-router>, 36,247 звёзд на момент проверки.
- Default branch: `main`.
- Исходный SHA разведки: `d2867bd4a4e128d1291f24e00f1ff010d1206f92`.
- Повторная проверка и тесты выполнены на актуальном upstream SHA
  `3b99fa239b581a787034cc4e3caf35640e32b35b` (`v3.0.17`).
- Форк создан: <https://github.com/apitokensale-admin/claude-code-router>.
- Опубликованная ветка: <https://github.com/apitokensale-admin/claude-code-router/tree/feat/apitoken-provider>.
- Опубликованный commit: `b8179b2d05064d83b37c6ae4a5e2a8a598889d9c`.
- Тот же patch чисто применён и повторно проверен поверх `v3.0.17`; локальный контрольный commit:
  `e1de0d53cf66c3eb7fbaec612e815d28eeb16e78`.
- Remote branch пока сохраняет исходного родителя `d2867bd`: обновление ref на историю `v3.0.17`
  GitHub отклонил, потому что промежуточный upstream release меняет `.github/workflows/release.yml`,
  а PAT не имеет scope `workflow`. Для PR это не создаёт conflict: patch применился на `3b99fa2`
  автоматически и именно в таком виде прошёл повторные проверки.
- `CONTRIBUTING.md` и repo-local `AGENTS.md` отсутствуют. Проверены GitHub workflows и npm scripts.
- Актуальный механизм — встроенный `ProviderPreset`, а не старый отдельный transformer/config.
- Принятые аналоги: `claudeapi` и `Fenno.ai`; оба регистрируются отдельным preset-модулем через
  `packages/core/src/providers/presets/index.ts`. Для Anthropic-совместимого endpoint используется
  протокол `anthropic_messages`.

### Реализация

Ветка опубликована. Добавлены:

- новый preset `apiToken.sale` с `https://api.apitoken.sale`;
- aliases для поиска в UI;
- Anthropic Messages capability;
- актуальные Claude model IDs;
- шаблон ключей `sk-pool-…` и ссылка на сайт;
- unit test, доказывающий регистрацию preset и совпадение URL без слеша, со слешем и с `/v1`;
- фирменная иконка в UI и документации;
- EN/ZH one-click import с публичной ссылкой на `https://apitoken.sale`;
- упоминание `apiToken.sale` в списках поддерживаемых провайдеров README/README_zh;
- UI test, проверяющий выбор иконки для нормализованного endpoint.

Затронутые upstream-пути:

- `README.md`;
- `README_zh.md`;
- `docs/public/provider-icons/apitoken.svg`;
- `docs/src/content/docs/en/configuration/provider-deeplink.md`;
- `docs/src/content/docs/zh/configuration/provider-deeplink.md`;
- `docs/src/styles/global.css`;
- `packages/core/src/providers/presets/apitoken/index.ts`;
- `packages/core/src/providers/presets/index.ts`;
- `packages/core/test/unit/providers/provider-preset-utils.test.mjs`;
- `packages/ui/src/assets/provider-icons/apitoken.svg`;
- `packages/ui/src/pages/home/shared/options.ts`;
- `packages/ui/test/integration/providers.test.ts`.

### Проверки

- `npm ci`: выполнен; root сообщает 7 известных dependency vulnerabilities, docs — 5.
- `npm run typecheck`: passed.
- `npm run test:ui`: 134 passed, 0 failed.
- `npm run build:assets`: passed.
- `npm run build` в `docs/`: 72 страницы собраны.
- `npm run test:core` с patch: 655 tests, 646 passed, 5 skipped, 4 failed.
- `npm run test:core` на чистом upstream `3b99fa2`: 654 tests, 645 passed, 5 skipped, 4 failed.
- Четыре ошибки полностью совпадают на patch и baseline: bounded-heap regression request log и три
  bundled `claude-design` path/permission теста. Интеграция добавляет один проходящий core test.

Живой прогон выполнен через локально собранный CCR `v3.0.17`, а не прямым запросом в обход него:

- basic Messages: HTTP 200, получен ожидаемый текст;
- Anthropic SSE streaming: HTTP 200, полный поток и ожидаемый текст;
- forced tool use: HTTP 200, вызван `integration_check` с `{ "status": "ok" }`.

Ключ использовался только через временную env-переменную. Временный CCR home с SQLite и ответы
удалены после прогона; в branch diff секретов нет.

### PR и backlink

- PR открыт: <https://github.com/musistudio/claude-code-router/pull/1598>.
- Статус сразу после открытия: `OPEN`, `MERGEABLE`, `mergeStateStatus=CLEAN`, draft=false.
- Base SHA: `3b99fa239b581a787034cc4e3caf35640e32b35b`; head SHA:
  `b8179b2d05064d83b37c6ae4a5e2a8a598889d9c`.
- В upstream настроены workflows только на push в `main` и release tag; pull-request checks не
  объявлены, поэтому отсутствие CI checks у PR ожидаемо и не является зависшим запуском.
- CLA-бот, запросы review и замечания на момент проверки не появились.
- Greptile запрошен комментарием `@greptileai please review`:
  <https://github.com/musistudio/claude-code-router/pull/1598#issuecomment-5104585428>.
- Для создания PR использован временный classic PAT только через process env; он не сохранён в
  keyring, файлах или git config. Предыдущие GraphQL/REST 403 fine-grained PAT закрыты.
- Ожидаемое упоминание после мержа: preset-модуль содержит `apitoken.sale`, endpoint и website URL;
  провайдер появляется в UI CCR, README и EN/ZH one-click docs.
- Ожидаемые публичные страницы:
  <https://ccrdesk.top/en/configuration/provider-deeplink/> и
  <https://ccrdesk.top/configuration/provider-deeplink/>.
- Проверка 2026-07-28: upstream `main` не содержит `apitoken.sale`; GitHub code search = 0;
  обе публичные страницы ссылки не содержат. Backlink status: `pending`.
- Следующая проверка: ответ Greptile/maintainer, merge и первый upstream release.

## Очередь и повторные проверки

После завершения каждой цели добавлять датированную запись ниже, даже если состояние не изменилось.
Это отличает «не проверяли» от «проверили, всё ещё pending».

| Дата | Цель | Проверено | Результат |
|---|---|---|---|
| 2026-07-28 | `BerriAI/litellm` | PR status | open, safe to merge; upstream backlink pending |
| 2026-07-28 | `musistudio/claude-code-router` | upstream/fork/rules/preset architecture | implementation in progress |
| 2026-07-28 | `musistudio/claude-code-router` | branch, current upstream, tests, live CCR run | branch ready; all integration checks passed; four core failures confirmed as pristine baseline |
| 2026-07-28 | `musistudio/claude-code-router` | GraphQL/REST PR creation, credentials | blocked only by fine-grained PAT; classic `public_repo` PAT required |
| 2026-07-28 | `musistudio/claude-code-router` | upstream/code search/public docs | no existing `apitoken.sale` reference; backlink pending |
| 2026-07-28 | `musistudio/claude-code-router` | classic PAT, PR creation, initial status | PR [#1598](https://github.com/musistudio/claude-code-router/pull/1598) open and cleanly mergeable; review pending; Greptile requested |

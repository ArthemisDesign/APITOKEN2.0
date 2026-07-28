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
| `musistudio/claude-code-router` | implementation/testing | pending | pending | pending | 2026-07-28 |
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
- Разведанный SHA: `d2867bd4a4e128d1291f24e00f1ff010d1206f92`.
- Форк создан: <https://github.com/apitokensale-admin/claude-code-router>.
- Рабочая ветка: `feat/apitoken-provider`.
- `CONTRIBUTING.md` и repo-local `AGENTS.md` отсутствуют. Проверены GitHub workflows и npm scripts.
- Актуальный механизм — встроенный `ProviderPreset`, а не старый отдельный transformer/config.
- Принятые аналоги: `claudeapi` и `Fenno.ai`; оба регистрируются отдельным preset-модулем через
  `packages/core/src/providers/presets/index.ts`. Для Anthropic-совместимого endpoint используется
  протокол `anthropic_messages`.

### Реализация

Пока не опубликована. Подготовлены:

- новый preset `apiToken.sale` с `https://api.apitoken.sale`;
- aliases для поиска в UI;
- Anthropic Messages capability;
- актуальные Claude model IDs;
- шаблон ключей `sk-pool-…` и ссылка на сайт;
- unit test, доказывающий регистрацию preset и совпадение URL без слеша, со слешем и с `/v1`.

Планируемые upstream-пути:

- `packages/core/src/providers/presets/apitoken/index.ts`;
- `packages/core/src/providers/presets/index.ts`;
- `packages/core/test/unit/providers/provider-preset-utils.test.mjs`.

### Проверки

- `npm ci`: выполнен; upstream сообщает 7 известных dependency vulnerabilities.
- Новый unit test проходит.
- Полный `test:core` дошёл до 624 тестов: 615 passed, 5 skipped, 4 failed.
- Все четыре сбоя находятся вне изменённых provider-файлов: один нестабильный bounded-heap тест
  request log и три теста bundled `claude-design` plugin paths/permissions. Перед PR их нужно
  перепроверить на чистом upstream SHA и указать как baseline, если воспроизводятся без патча.
- `typecheck` и оставшиеся workspace-тесты ещё не зафиксированы: цепочка остановилась на core fail.
- Живой basic/stream/tool-use прогон через CCR ещё предстоит.

### PR и backlink

- PR: `pending`.
- Ожидаемое упоминание после мержа: preset-модуль содержит `apitoken.sale`, endpoint и website URL;
  провайдер появляется в UI CCR.
- Backlink status: `pending`.
- Следующая проверка: после push, затем после PR review, merge и первого upstream release.

## Очередь и повторные проверки

После завершения каждой цели добавлять датированную запись ниже, даже если состояние не изменилось.
Это отличает «не проверяли» от «проверили, всё ещё pending».

| Дата | Цель | Проверено | Результат |
|---|---|---|---|
| 2026-07-28 | `BerriAI/litellm` | PR status | open, safe to merge; upstream backlink pending |
| 2026-07-28 | `musistudio/claude-code-router` | upstream/fork/rules/preset architecture | implementation in progress |

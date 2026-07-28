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

## Доступ к GitHub

- Основная fine-grained сессия `gh` не изменена.
- Classic PAT для внешних PR хранится только в macOS Keychain: service
  `apitoken-sale/github-classic-pat`, account `apitokensale-admin`.
- `~/.config/apitoken-sale/github.env` имеет права `600` и содержит только loader из Keychain;
  самого токена нет ни в файле, ни в репозитории.
- Проверка 2026-07-28: GitHub API HTTP 200, аккаунт `apitokensale-admin`; scope `repo` включает
  требуемый `public_repo`. Токен избыточно привилегирован и истекает 2026-08-27 — при ротации
  заменить на минимальный classic PAT.
- Полные правила использования и ротации: `research/GEO_GITHUB.md`.

## Сводка

| Цель | Состояние | PR | Упоминание в upstream | Публичная ссылка | Последняя проверка |
|---|---|---|---|---|---|
| `BerriAI/litellm` | open, safe to merge | [#34915](https://github.com/BerriAI/litellm/pull/34915) | pending merge | pending | 2026-07-28 |
| `musistudio/claude-code-router` | PR open; review pending | [#1598](https://github.com/musistudio/claude-code-router/pull/1598) | pending merge | absent | 2026-07-28 |
| `LibreChat-AI/librechat.ai` | PR open; review pending | [#713](https://github.com/LibreChat-AI/librechat.ai/pull/713) | pending merge | absent | 2026-07-28 |
| `QuantumNous/new-api` | PR open; automated review clean, human review pending | [docs #196](https://github.com/QuantumNous/new-api-docs-v1/pull/196) | pending merge | absent | 2026-07-28 |
| `Aider-AI/aider` | PR open; CLA and review pending | [#5504](https://github.com/Aider-AI/aider/pull/5504) | pending merge | absent | 2026-07-28 |
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
  а использованный тогда PAT не имел scope `workflow`. Для PR это не создаёт conflict: patch
  применился на `3b99fa2` автоматически и именно в таком виде прошёл повторные проверки.
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
- Classic PAT для создания и сопровождения внешних PR сохранён в отдельной записи macOS Keychain;
  process env загружается локальным loader и не меняет основную fine-grained сессию `gh`.
  Предыдущие GraphQL/REST 403 fine-grained PAT закрыты.
- Ожидаемое упоминание после мержа: preset-модуль содержит `apitoken.sale`, endpoint и website URL;
  провайдер появляется в UI CCR, README и EN/ZH one-click docs.
- Ожидаемые публичные страницы:
  <https://ccrdesk.top/en/configuration/provider-deeplink/> и
  <https://ccrdesk.top/configuration/provider-deeplink/>.
- Проверка 2026-07-28: upstream `main` не содержит `apitoken.sale`; GitHub code search = 0;
  обе публичные страницы ссылки не содержат. Backlink status: `pending`.
- Следующая проверка: ответ Greptile/maintainer, merge и первый upstream release.

## LibreChat-AI/librechat.ai

### Разведка

- Upstream документации: <https://github.com/LibreChat-AI/librechat.ai>, 608 звёзд на момент
  проверки; default branch `main`, исходный SHA
  `31e9fad7982552980d0ee1e2a94ed20080c89ed3`.
- Основное приложение: <https://github.com/danny-avila/LibreChat>, 41,384 звезды, версия `v0.8.7`,
  проверенный SHA `f7bc50ae5b752e50fab7f97ee00531ca1264ea05`.
- В docs-репозитории нет `CONTRIBUTING.md`, `AGENTS.md` или `CLAUDE.md`; обязательный workflow из
  README: английский MDX как source of truth, sidebar через `meta.json`, затем Prettier, lint,
  typecheck и production build. Переводы генерирует upstream workflow, вручную их не добавлять.
- Принятый аналог: [NEAR AI Cloud #668](https://github.com/LibreChat-AI/librechat.ai/pull/668) —
  отдельная provider-страница, регистрация в `meta.json`, локальный build и живое подтверждение
  доступности endpoint. PR смержен после maintainer review; Vercel preview для внешнего автора так
  же требовал authorization.
- Старый путь через `ANTHROPIC_REVERSE_PROXY` больше не является лучшим вариантом. В LibreChat
  [#13748](https://github.com/danny-avila/LibreChat/pull/13748) добавлен `provider: anthropic` для
  custom endpoints: он выбирает native Messages API client и использует заданные `baseURL` и
  `apiKey`.

### Реализация

- Форк: <https://github.com/apitokensale-admin/librechat.ai>.
- Ветка: <https://github.com/apitokensale-admin/librechat.ai/tree/feat/apitoken-provider>.
- Опубликованный commit: `0aacd9c9773bef876314b094a49675ee2868f080`.
- Добавлена provider-страница `apiToken.sale` и sidebar entry; затронуты только:
  - `content/docs/configuration/librechat_yaml/ai_endpoints/apitoken.mdx`;
  - `content/docs/configuration/librechat_yaml/ai_endpoints/meta.json`.
- Конфигурация использует наш `sk-pool-...` key через env, `https://api.apitoken.sale` как base URL,
  native `provider: anthropic`, явный список актуальных моделей и `models.fetch: false`.
- В тексте отдельно зафиксировано: `provider: anthropic` обозначает wire protocol LibreChat, а не
  официальный ключ или host Anthropic. Секретов в branch diff нет.

### Проверки

- `pnpm install --frozen-lockfile`: passed.
- `pnpm lint:prettier`: passed.
- `pnpm lint`: passed, 0 warnings.
- `pnpm typecheck`: passed, включая MDX generation.
- `pnpm test`: 22 test files, 294 tests passed.
- Production build с `NODE_OPTIONS=--max-old-space-size=8192`: passed; сгенерировано 2,879 static
  pages. Первый запуск со стандартным 4 GB heap завершился Node OOM; после увеличения только heap
  тот же build прошёл без изменений кода.
- Публичные ссылки `apitoken.sale`, `/register`, `/docs` и `/models`: HTTP 200; API root корректно
  отвечает 401 без ключа.

Живой прогон сделан через реальный код LibreChat `v0.8.7`: документированная конфигурация прошла
через `initializeCustom`, затем через native Anthropic model client LibreChat.

- basic message: получен точный ответ `LIBRECHAT_APITOKEN_OK`;
- streaming: получен полный поток `LIBRECHAT_STREAM_OK`;
- tool use: вызван `integration_check` с `{ "status": "ok" }`.

Ключ передавался только через неэхируемый stdin в process env, после процесса удалён; ни в одном
файле приложения, docs-ветки или отчёта его значения нет.

### PR и backlink

- PR открыт: <https://github.com/LibreChat-AI/librechat.ai/pull/713>.
- Статус после открытия: `OPEN`, `MERGEABLE`, draft=false; head SHA `0aacd9c`, base SHA `31e9fad`.
- `mergeStateStatus=UNSTABLE` вызван только Vercel preview со статусом `Authorization required to
  deploy`. Это внешний team permission, а не падение кода; тот же комментарий был в уже смерженном
  аналоге #668. От владельца apiToken.sale действий не требуется.
- CLA, maintainer review и замечания пока не появились.
- Greptile запрошен комментарием `@greptileai please review`:
  <https://github.com/LibreChat-AI/librechat.ai/pull/713#issuecomment-5105063543>.
- Ожидаемое упоминание после мержа: отдельная sidebar-страница с брендом, website URL, API host и
  готовой LibreChat-конфигурацией.
- Ожидаемая публичная страница:
  <https://www.librechat.ai/docs/configuration/librechat_yaml/ai_endpoints/apitoken>.
- Проверка 2026-07-28: upstream code search = 0, public page возвращает 404. Backlink status:
  `pending`.
- Следующая проверка: Greptile/maintainer review, Vercel authorization при необходимости, merge и
  публикация страницы.

## QuantumNous/new-api

### Разведка

- Основной upstream: <https://github.com/QuantumNous/new-api>, 43,663 звезды на момент проверки;
  default branch `main`, SHA разведки `afe16c64cd73853da1eda3bf236f15d69637b4bf`.
- В основном репозитории уже есть нативный Anthropic channel (`ChannelTypeAnthropic = 14`). Его
  adaptor добавляет `/v1/messages` к `ChannelBaseUrl`, передаёт ключ в `x-api-key` и выставляет
  `anthropic-version: 2023-06-01`; отдельный provider type для apiToken.sale не нужен.
- UI отдельно предупреждает не добавлять `/v1` к Base URL. Поэтому корректное значение для нашей
  интеграции — ровно `https://api.apitoken.sale`, без `/v1` и завершающего слеша.
- Старый <https://github.com/QuantumNous/new-api-docs> помечен архивным. Актуальная документация
  находится в <https://github.com/QuantumNous/new-api-docs-v1>; default branch `main`, SHA
  разведки `cb6ee07375e03ac7556b29ece10f165807497a48`.
- В `new-api-docs-v1` отсутствуют repo-local `AGENTS.md`, `CLAUDE.md` и `CONTRIBUTING.md`.
  Проверены README, workflow автоперевода, структура Fumadocs и package scripts.
- Принятые аналоги: [DeepChat #143](https://github.com/QuantumNous/new-api-docs-v1/pull/143)
  добавил полноценный трёхъязычный интеграционный гайд; внешняя документационная правка
  [#163](https://github.com/QuantumNous/new-api-docs-v1/pull/163) была принята с тем же ожидаемым
  Vercel `Authorization required`. Source-язык автоперевода — китайский; английский вариант можно
  включать в исходный PR, японский upstream способен сгенерировать после мержа.
- Поиск в issues, PR и upstream-коде `new-api`, `new-api-docs` и `new-api-docs-v1` не нашёл
  существующего упоминания `apitoken.sale`.

### Реализация

- Форк документации: <https://github.com/apitokensale-admin/new-api-docs-v1>.
- Ветка: <https://github.com/apitokensale-admin/new-api-docs-v1/tree/docs/apitoken-anthropic-channel>.
- Опубликованный commit: `e9e0a51c9c50e1ab87b37ba9b620d57d713edb5b`.
- В существующий admin channel guide добавлен короткий EN/ZH пример без нового типа канала.
- Гайд фиксирует: `Anthropic` как wire protocol, Base URL без `/v1`, provider-issued ключ формата
  `sk-pool-...`, выбор моделей, channel test и ссылки на список моделей/документацию.
- Отдельно указано, что apiToken.sale — сторонний Anthropic-compatible endpoint, а ключ не выдан
  Anthropic. Перед production читателю предложено независимо проверить авторизацию, условия и
  data-handling policy провайдера в соответствии с compliance policy New API.
- Затронуты только два upstream-пути:
  - `content/docs/en/guide/feature-guide/admin/channel.mdx`;
  - `content/docs/zh/guide/feature-guide/admin/channel.mdx`.
- В PR body явно раскрыто, что изменение AI-assisted. Секретов в commit и PR нет.

### Проверки

- `bun install --frozen-lockfile`: passed.
- Prettier check двух изменённых MDX: passed.
- `bun run build`: passed; сгенерировано 2,065 static pages. Prebuild обновляет changelog из
  GitHub Releases; эти несвязанные локальные артефакты после успешной проверки не включены в diff.
- В `QuantumNous/new-api`: `go test ./relay/channel/claude` passed.
- Живой прогон через текущий Claude adaptor New API подтвердил:
  `https://api.apitoken.sale/v1/messages`, непустой `x-api-key`,
  `anthropic-version: 2023-06-01`, HTTP 200 и модель `claude-sonnet-4-6`.
- Отдельные live smoke проверки: basic Messages — HTTP 200; Anthropic SSE stream — HTTP 200 и
  полный набор событий до `message_stop`; forced tool use — HTTP 200, `stop_reason=tool_use` и
  вызов ожидаемого инструмента.
- Ключ передавался только через неэхируемый stdin в process env и не сохранялся в исходниках,
  branch diff, выводе или PR. Публичные homepage, model list и docs URL отвечают HTTP 200.

### PR и backlink

- PR открыт: <https://github.com/QuantumNous/new-api-docs-v1/pull/196>.
- Статус: `OPEN`, `MERGEABLE`, draft=false; head SHA `e9e0a51`, base SHA `cb6ee07`.
- CodeRabbit завершил review успешно и не оставил actionable comments; все 5 pre-merge checks
  CodeRabbit зелёные. Его promotional/slop detector пометил PR как потенциально рекламный, поэтому
  человеческий maintainer review остаётся важным риском принятия, но технической правки бот не
  запросил.
- Единственный красный check — Vercel `Authorization required to deploy`: внешнему автору нельзя
  запускать preview в team QuantumNous. Это тот же permission-state, что в принятых аналогах, а не
  падение сборки.
- Greptile запрошен комментарием `@greptileai`:
  <https://github.com/QuantumNous/new-api-docs-v1/pull/196#issuecomment-5105548789>.
- CLA и human review на момент проверки не появились.
- Ожидаемое упоминание после мержа: EN/ZH channel guide содержит бренд, API host, формат ключа,
  ссылки на model list/docs и точную конфигурацию Anthropic channel.
- Ожидаемые публичные страницы:
  <https://docs.newapi.pro/en/docs/guide/feature-guide/admin/channel> и
  <https://docs.newapi.pro/zh/docs/guide/feature-guide/admin/channel>.
- Проверка 2026-07-28: обе текущие страницы доступны с browser User-Agent, но ещё не содержат
  `apitoken.sale`; upstream code search = 0. Backlink status: `pending`.
- Следующая проверка: Greptile/human review, решение maintainer по promotional warning, merge,
  автоперевод JA и публикация EN/ZH страниц.

## Aider-AI/aider

### Разведка

- Upstream: <https://github.com/Aider-AI/aider>, 47,755 звёзд на момент проверки; default branch
  `main`, SHA разведки `5dc9490bb35f9729ef2c95d00a19ccd30c26339c`.
- Последний upstream merge датирован 2026-05-22: внешний PR
  [#5173](https://github.com/Aider-AI/aider/pull/5173) принят. Репозиторий замедлился после релиза
  `v0.86.0`, но продолжает принимать точечные внешние изменения и не архивирован.
- `CONTRIBUTING.md` разрешает небольшие PR без предварительного issue, требует Individual CLA и
  описывает Jekyll-сборку документации. Repo-local `AGENTS.md` и `CLAUDE.md` отсутствуют.
- Принятый аналог: [#3043](https://github.com/Aider-AI/aider/pull/3043) добавил инструкции API key
  в существующую provider-страницу DeepSeek и был смержен maintainer без дополнительных правок.
  Текущие LLM-доки группируют провайдеров в `aider/website/docs/llms/`.
- Aider не реализует Anthropic transport самостоятельно: текущий `main` закрепляет LiteLLM
  `1.82.3`. Его Anthropic path читает `ANTHROPIC_API_BASE` и `ANTHROPIC_API_KEY`, а модель с
  префиксом `anthropic/` отправляется через Messages API.
- Исходники LiteLLM `1.82.3` и живой прогон подтвердили корректное значение
  `ANTHROPIC_API_BASE=https://api.apitoken.sale`: корневой URL без `/v1/messages`.

### Реализация

- Форк: <https://github.com/apitokensale-admin/aider>.
- Ветка: <https://github.com/apitokensale-admin/aider/tree/docs/apitoken-anthropic-endpoint>.
- Опубликованный commit: `cb15e1dd997fd40c5756208f819d09fc6839ca57`.
- Изменён только `aider/website/docs/llms/anthropic.md`: добавлен нейтральный раздел про любые
  Anthropic-compatible endpoints и apiToken.sale как проверенный пример.
- Документированы Mac/Linux и Windows env, корневой Base URL, provider-issued key и явный запуск
  `anthropic/claude-sonnet-4-6`.
- Отдельный note сообщает, что apiToken.sale — сторонний сервис, не Anthropic, а его ключи не
  Anthropic-issued; читателю предложено проверить условия и data-handling policy.
- В diff одна публичная ссылка на homepage, нет цен, сравнительных обещаний, секретов или изменений
  кода Aider. PR body раскрывает AI assistance.

### Проверки

- `git diff --check`: passed.
- `codespell aider/website/docs/llms/anthropic.md`: passed.
- Production Jekyll build на Ruby 3.3: passed; сгенерировано 132 HTML-файла.
- Новый heading, callout, env-блоки и ссылка проверены в собранном HTML.
- Полный `html-proofer` обработал 132 файла и обнаружил только 5 существующих проблем вне
  изменённой страницы: два старых HTTP URL, пустой `a` на share page и два старых broken anchors
  в blog post. Новая ссылка отвечает HTTP 200.

Живой прогон выполнен на актуальном Aider `0.86.3.dev53+g5dc9490bb` с его закреплённым LiteLLM
`1.82.3`, а не прямым SDK в обход клиентского стека:

- Aider с `ANTHROPIC_API_BASE=https://api.apitoken.sale` и моделью
  `anthropic/claude-sonnet-4-6` получил streaming response, применил ожидаемый search/replace и
  изменил только запрошенную строку во временном файле;
- basic completion через тот же установленный LiteLLM: passed;
- SSE streaming через тот же LiteLLM: passed;
- forced tool use вызвал `integration_check` с `{ "status": "ok" }`: passed.

Ключ передавался через неэхируемый stdin только в process env. Временный git-репозиторий и
неподходящий Python 3.9 venv перемещены в Trash; в ветке, логах и PR секрета нет.

### PR и backlink

- PR открыт: <https://github.com/Aider-AI/aider/pull/5504>.
- Статус: `OPEN`, `MERGEABLE`, draft=false; base SHA `5dc9490b`, head SHA `cb15e1dd`.
- Первый GitHub Actions run имеет ожидаемый `action_required`: maintainer должен разрешить workflow
  нового внешнего автора. Это permission gate, а не падение pre-commit.
- Individual CLA пока не подписан; CLA Assistant показывает `license/cla=PENDING`. Подписание —
  юридическое действие владельца аккаунта по ссылке
  <https://cla-assistant.io/Aider-AI/aider?pullRequest=5504>, агент его не принимает самостоятельно.
- Greptile запрошен комментарием `@greptileai`:
  <https://github.com/Aider-AI/aider/pull/5504#issuecomment-5106107037>; ответ пока не появился.
- Human review и замечания отсутствуют.
- Ожидаемая публичная ссылка после мержа:
  <https://aider.chat/docs/llms/anthropic.html#anthropic-compatible-endpoints>.
- Проверка 2026-07-28: upstream code search = 0; текущая публичная Anthropic-страница отвечает
  HTTP 200, но `apitoken.sale` ещё не содержит. Backlink status: `pending`.
- Следующая проверка: CLA, разрешение pre-commit workflow, Greptile/human review, merge и
  публикация GitHub Pages.

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
| 2026-07-28 | `musistudio/claude-code-router` | Keychain credential, PR/review/checks | classic PAT validated and persisted outside repo; PR remains open, cleanly mergeable, with no CI, review comments or actionable blockers; next target may start |
| 2026-07-28 | `LibreChat-AI/librechat.ai` | upstream rules, native Anthropic path, accepted analog | docs target confirmed; `provider: anthropic` routes custom base URL through native Messages API |
| 2026-07-28 | `LibreChat-AI/librechat.ai` | implementation, full docs gate, live LibreChat run | branch ready; formatting, lint, typecheck, 294 tests and 2,879-page build passed; basic, streaming and tool use passed through LibreChat v0.8.7 |
| 2026-07-28 | `LibreChat-AI/librechat.ai` | PR, reviews, checks, upstream/public backlink | PR [#713](https://github.com/LibreChat-AI/librechat.ai/pull/713) open and mergeable; Vercel requires maintainer authorization; review and backlink pending; Greptile requested |
| 2026-07-28 | `QuantumNous/new-api` | code/docs repositories, channel adaptor, contribution analogs, duplicate search | native Anthropic channel confirmed; current target is `new-api-docs-v1`; no existing `apitoken.sale` reference |
| 2026-07-28 | `QuantumNous/new-api` | EN/ZH guide, docs build, Go test, direct and adaptor live runs | branch ready; 2,065-page build passed; basic, SSE, tool use and actual New API adaptor request passed |
| 2026-07-28 | `QuantumNous/new-api` | PR, automated review, checks, upstream/public backlink | docs PR [#196](https://github.com/QuantumNous/new-api-docs-v1/pull/196) open and mergeable; CodeRabbit clean with promotional warning; Vercel authorization and human review pending; Greptile requested |
| 2026-07-28 | `Aider-AI/aider` | activity, contribution rules, LiteLLM transport, accepted docs analog | repo still accepts focused PRs; `ANTHROPIC_API_BASE` on pinned LiteLLM 1.82.3 is the supported path |
| 2026-07-28 | `Aider-AI/aider` | docs change, Jekyll build, rendered HTML, live Aider and LiteLLM smoke | one-file branch ready; codespell/build passed; Aider edit plus basic, SSE and tool use passed |
| 2026-07-28 | `Aider-AI/aider` | PR, CLA, workflow, review and backlink | PR [#5504](https://github.com/Aider-AI/aider/pull/5504) open and mergeable; CLA and first-time workflow approval pending; Greptile requested; backlink absent |

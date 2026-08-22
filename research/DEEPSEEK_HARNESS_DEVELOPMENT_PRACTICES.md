# Практики DeepSeek Harness, которые стоит адаптировать в нашем репозитории

> **Статус:** предложение для обсуждения, не действующий регламент.
>
> **Базовые ревизии сравнения:** наш репозиторий — `c25a5adc7eef3b6498c8842cbed90377c64c67f4`; DeepSeek Harness — `909dc8146a743ab4b1aa0a51c1a4159e0dec00dd`. Checkout DeepSeek Harness содержал посторонние незакоммиченные изменения; ни один из опорных process/testing файлов, на которых основаны предложения, не входил в их список.
>
> **Обозначение путей:** в разделах «Идея из DeepSeek Harness» пути относятся к checkout DeepSeek Harness; в разделах «Текущее положение у нас» и «Как адаптировать» — к нашему репозиторию, если не сказано обратное.

## Краткий вывод

Наш процесс уже сильнее DeepSeek Harness в нескольких важных местах: изоляция через управляемые worktree, сериализованное попадание в `master`, path-aware merge gate, проверка и доставка точного SHA, expand-only миграции и межконтекстные контракты, producer-first rollout, frontend preview и production watchdog. Эти механизмы не нужно заменять.

Наибольшую пользу даст перенос не архитектуры «всё является плагином» и не конкретных инструментов DeepSeek Harness, а восьми процессных идей. Ниже они перечислены в рекомендуемом порядке отдачи:

1. проверять продукт через реальный shipped entry path и собранные артефакты;
2. воспроизводить стабильные внешние сценарии keyless через детерминированный replay с семантическими guards;
3. использовать один явный change scope и детерминированный план доказательств для локальной работы и merge gate;
4. превращать production/near-miss инциденты в причинный postmortem, короткое defensive rule и исполняемую регрессию;
5. превращать ключевые архитектурные правила в исполняемые проверки;
6. делать документацию проверяемой проекцией исходного кода, а не только обязательным соседним `.md`;
7. выбирать глубину тестов по риску и добавить единый hygiene-контур для Rust и TypeScript;
8. хранить инженерные решения с явным жизненным циклом и альтернативами.

Рекомендуемый первый пакет — shipped-path acceptance с keyless replay, developer-facing change plan и один repository-invariant/docs-hardening slice. Он закрывает реальные классы дрейфа с умеренным объёмом изменений и не требует перестройки delivery pipeline. Стандарт incident → guardrail можно добавить параллельно как лёгкий документный процесс без нового merge gate.

## Что у нас уже есть и копировать повторно не нужно

| Концепция | Наш механизм |
|---|---|
| Изоляция параллельной работы | Управляемые worktree через `deploy/agent-worktree.sh`, запрет raw `git worktree` и переключения веток в общей директории. |
| Локальные инструкции рядом с кодом | Корневые `AGENTS.md`/`CLAUDE.md` и локальные `crates/<name>/CLAUDE.md`. |
| Живая документация | Обновление контрактных документов в том же коммите, `docs/DEPENDENCIES.md`, `docs/CHANGE_CHECKLISTS.md`, `deploy/docs-check.sh`. |
| Дешёвые проверки по области diff | Классификаторы в `deploy/watchdog-lib.sh` и чистый планировщик `deploy/validation-plan.sh`. |
| Проверка неизменяемого кандидата | Локальный gate и trusted-host validation привязаны к точному SHA; merge сериализован. |
| Безопасные изменения схем и контрактов | Expand-only, migration-first и producer-first с ожиданием production GREEN. |
| Проверка frontend человеком | Уникальные `preview/*` ветки и запрет merge до одобрения preview. |
| Доменные playbook/skills | Например, `.claude/skills/provider-onboarding/SKILL.md` и полный provider onboarding contract. |

DeepSeek Harness использует другие ветки, PR stack, GitHub Actions и release-процесс. Они решают другие ограничения и не превосходят наш точный production watchdog автоматически.

## Критерии отбора предложений

Предложение попало в список, если оно:

- закрывает класс ошибок, который не устраняется текущим merge/watchdog-контуром;
- может быть введено постепенно и fail-closed;
- переиспользует существующие классификаторы, тестовые базы и документацию;
- даёт измеримый сигнал, а не только новое правило для чтения;
- не переносит в наш репозиторий продуктовую архитектуру DeepSeek Harness.

## Кандидат 5 (P1). Исполняемые архитектурные инварианты

### Идея из DeepSeek Harness

DeepSeek Harness не ограничивается текстом в `AGENTS.md`. Каждый пакет обязан публиковать собственный invariant companion, а `scripts/verify-package-invariants.ts` и `scripts/package-invariants.ts` проверяют полноту, регистрацию, сборку и допустимую форму этих проверок. Дополнительные workspace-инварианты закреплены в `scripts/check-workspace-constraints.ts`.

### Текущее положение у нас

Наши архитектурные ограничения подробно описаны в `CLAUDE.md`, crate-level инструкциях и `docs/DEPENDENCIES.md`. Cargo уже механически запрещает циклическое нарушение направления зависимостей, но не все отрицательные правила следуют из графа зависимостей. Например, запреты на HTTP/network в нижних слоях, чтение env вне composition layer и некоторые правила integer-only money требуют отдельной проверки.

Исторический `docs/audits/TESTS_AUDIT.md` отдельно отмечал этот класс разрыва между документированным и механически проверяемым состоянием. Перед реализацией нужен новый baseline по текущему SHA: аудит является снимком, а не доказательством актуального нарушения.

### Как адаптировать

Добавить один верхнеуровневый проверяющий контур, условно `deploy/repository-invariants.sh` или `tools/repository-invariants/`, и включить его в always-on static lane. Не нужно сразу требовать invariant-файл от каждого crate/package.

Первый пилот должен проверять три правила с высокой ценностью и низкой неоднозначностью:

1. `crates/pool` не получает HTTP/network-зависимости, а `crates/registry` — HTTP/external-network зависимости;
2. production-чтение environment остаётся в разрешённых composition-файлах; тестовые обращения и стандартные временные каталоги учитываются явным allowlist;
3. новые межworkspace-зависимости TypeScript следуют разрешённым bounded-context направлениям и не обходят `packages/engine-client`/HTTP-контракты.

Проверка должна работать по структуре manifest/AST там, где простой grep даёт ложные срабатывания. Исключения должны быть узкими, именованными и проверяемыми.

### Польза

- Архитектурный запрет ломается в коммите, где появилась запрещённая зависимость, а не при следующем аудите.
- Ревьюер видит точное нарушенное правило и путь.
- `AGENTS.md` становится короче: в нём остаётся правило и ссылка на исполняемую проверку.

### Риск слепого копирования

Обязательный invariant companion для каждого нашего приложения и crate создаст много пустых файлов и формальное выполнение процесса. Начинать нужно с нескольких глобальных инвариантов, которые действительно можно доказать автоматически.

### Пилот и критерий успеха

Пилот: три правила выше плюс regression fixtures для разрешённого и запрещённого изменения.

Критерий успеха: каждая намеренная мутация нарушает gate с конкретной диагностикой; текущий `master` проходит без широких исключений; добавка к static lane укладывается в 10 секунд на тёплом checkout.

## Кандидат 1 (P0). Проверка реального пути запуска и собранного артефакта

### Идея из DeepSeek Harness

`docs/testing.md` разделяет unit, coverage, real-API e2e, keyless snapshots и browser snapshots. Главный принцип: тест должен проходить тем же путём, которым запускается продукт. DeepSeek Harness отдельно запускает built `lib/` под обычным Node, реальный Loader, subprocess и wire protocol. Постмортем `docs/postmortem/0001-acp-default-export-drops-inject.md` показывает, почему 100% line coverage не обнаружил сломанный реальный load path.

### Текущее положение у нас

У нас уже есть сильные mock-upstream и production-path проверки: `tests/rotation_fanout_smoke.sh`, `tests/universal_chat_smoke.sh`, `deploy/test-stage2-e2e.sh`, disposable PostgreSQL/Redis и exact-SHA candidate validation. Это хорошая база.

Недостающая концепция — явное правило, по которому изменение пользовательского или межконтекстного поведения обязано иметь один сценарий через реальную композицию и, где применимо, через собранный артефакт. Unit-тесты производителя и потребителя по отдельности не доказывают, что они совместимы в реальном entry path.

### Как адаптировать

Ввести небольшой каталог «assembled contract scenarios». Сценарий:

- запускает реальный binary/app из того артефакта, который попадёт в release;
- подменяет только внешнюю дорогую или недетерминированную границу;
- проверяет внешний результат: HTTP/wire, запись в БД, файл, SSE transcript или состояние очереди;
- keyless по умолчанию;
- сохраняет компактный golden transcript только для стабильного внешнего контракта.

Первый кандидат — engine Control API ↔ `packages/engine-client`: реальный `claude-api` с disposable PostgreSQL, реальный `EngineClient`, основные операции account/key/ledger/pricing и strict parse ответа. Второй кандидат — один SSE-сценарий router → plane → mock upstream с byte/event-level ожиданием.

### Польза

- Ловит несовместимость сериализации, сборки, маршрутизации, env wiring и package exports.
- Проверяет не только обе стороны контракта, но и путь между ними.
- Уменьшает зависимость от production canary как первого места, где проявляется ошибка композиции.

### Риск слепого копирования

Snapshot всего JSON/HTML создаёт шум и хрупкие обновления. Golden нужен только для стабильной внешней семантики; динамические идентификаторы и время нормализуются узко, а не глобальным «очистителем всего».

### Пилот и критерий успеха

Пилот: один Control API scenario с 5–8 ключевыми операциями и одним отрицательным случаем несовместимой схемы.

Критерий успеха: сценарий краснеет при намеренном переименовании поля на producer или consumer стороне, при запуске не того артефакта и при пропуске обязательной миграции; время выполнения — не более двух минут поверх существующей DB lane.

## Кандидат 2 (P0). Keyless deterministic replay внешнего поведения

### Идея из DeepSeek Harness

DeepSeek Harness записывает реальный assembled transcript один раз, затем воспроизводит его без API-ключа через ту же композицию. `docs/testing.md` и snapshot infrastructure разделяют запись и read-only replay; merge gate сравнивает стабильный внешний результат, а не вызывает модель или сеть.

### Как адаптировать

Начать с одного router → engine → mock upstream сценария. Зафиксировать вход, HTTP/SSE transcript и необходимые durable side effects. Динамические UUID, порты и время нормализовать только в конкретных полях.

Replay обязан иметь семантические guards: пустой результат, один error-only ответ, отсутствие terminal usage или обязательного SSE event не считаются успехом только потому, что golden совпал. Record/update остаётся явной локальной операцией с review diff; merge gate работает read-only и никогда не перезаписывает ожидания.

### Польза

- Проверяет assembled внешний контракт детерминированно и без секретов.
- Отделяет намеренное изменение transcript от случайного дрейфа.
- Даёт дешёвый regression signal после того, как shipped-path scenario уже доказал реальную композицию.

### Риск слепого копирования

Полный ACP/GUI snapshot corpus DeepSeek Harness нам не нужен. Большой golden превращается в шум, а автоматическое обновление ожиданий может закрепить неправильное поведение. Snapshot дополняет семантические assertions, а не заменяет их.

### Пилот и критерий успеха

Пилот: один сценарий с non-stream и SSE веткой, terminal usage и одним отрицательным ответом. Критерий успеха: 20 последовательных replay дают байтово одинаковый нормализованный результат; намеренное удаление обязательного event и подмена результата на error-only ломают gate; CI не требует provider key.

## Кандидат 6 (P1). Документация как проверяемая проекция исходного кода

### Идея из DeepSeek Harness

DeepSeek Harness имеет отдельный `doc-sync` DAG. Он проверяет Markdown links и anchors, generated catalogs, code/document type equivalence, package paths, Mermaid, Agent Note format, documentation budgets и сборку сайта. `docs/AGENTS.md` задаёт принцип «один факт — один владелец»: в других местах должна быть ссылка, а не копия.

### Текущее положение у нас

Наш `deploy/docs-check.sh` правильно делает документацию частью merge gate, но его намеренно грубая эвристика считает достаточным любой `.md` в diff. Сам скрипт честно документирует это ограничение. `docs/DEPENDENCIES.md` и `docs/CHANGE_CHECKLISTS.md` содержат богатую семантику, которую нельзя полностью сгенерировать, но часть их фактов можно проверять автоматически.

Исторический `docs/audits/2026-08-01-AGENT_DOCS_AUDIT.md` показывает типичные последствия: несвязанный Markdown может удовлетворить gate, списки потребителей и команд могут дрейфовать, а несколько входных документов могут повторить правило по-разному. Часть конкретных находок уже исправлена; переносить нужно метод предотвращения, а не старый список дефектов.

### Как адаптировать

Разделить документационный gate на три уровня:

1. **Связность Markdown.** Проверка относительных ссылок, anchors и полноты `docs/README.md`.
2. **Targeted ownership.** Для критичных code surfaces требовать конкретное семейство документов, а не любой `.md`. Например, изменения `crates/metering/**` требуют pricing/provider contract; `packages/payments/**` — payment integration/DEPENDENCIES; Control API producer — `docs/engine/CONTROL_API.md`.
3. **Generated/validated projections.** Генерировать или сверять только факты, которые имеют машинный источник: package dependency graph, перечень публичных routes, alert → runbook anchor, workspace consumers, migration inventory. Человеческое объяснение остаётся ручным.

`docs/DEPENDENCIES.md` не нужно заменять generated-файлом. Лучше добавить рядом компактный machine-readable manifest или generated appendix, с которым ручная карта обязана согласовываться по идентификаторам producer/consumer.

Для входных документов полезно применить ограниченный принцип DeepSeek Harness «один дом для факта»: `AGENTS.md` содержит приказ и ссылку; `CONTRIBUTING.md` — contributor flow; `deploy/README.md` — точную механику gate; остальные документы не копируют полный список команд.

### Польза

- Несвязанная документационная правка больше не маскирует изменение контракта.
- Битые ссылки, исчезнувшие anchors и забытые потребители ловятся до merge.
- Снижается вероятность противоречий между `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` и `BRANCHES.md`.

### Риск слепого копирования

Полная генерация `docs/DEPENDENCIES.md` уничтожит важные семантические детали: порядок rollout, privacy, fail-closed поведение и экономические инварианты. Генерировать нужно инвентарь, а не объяснение.

Жёсткие word budgets нельзя применять ко всем provider/runbook документам: длинная таблица фактов может быть корректной. Бюджет уместен только для входных и часто разрастающихся документов.

### Пилот и критерий успеха

Пилот:

- проверка Markdown links/anchors;
- проверка полноты `docs/README.md`;
- targeted mapping для Control API, metering/pricing, payments, sales feed и alerts;
- regression cases: «несвязанный `.md`», «битый anchor», «новый alert без runbook», «новый consumer без DEPENDENCIES».

Критерий успеха: все четыре мутации блокируются; docs-only diff остаётся дешёвым; ложное срабатывание имеет явный путь исправления или узкое исключение.

## Кандидат 3 (P0). Один детерминированный change-scope и план проверок

### Идея из DeepSeek Harness

`scripts/change-scope.ts` строит versioned JSON по committed/staged/unstaged/untracked scope. `.agents/skills/dsh-pre-push-checks/SKILL.md` использует этот отчёт для выбора минимального доказательства. `scripts/run-gates.ts` описывает проверки как DAG с зависимостями, bounded concurrency, временем выполнения и точной причиной skip/failure.

### Текущее положение у нас

У нас уже есть более важная часть: `deploy/validation-plan.sh` чисто вычисляет fail-closed validation envelope по точным SHA, а `deploy/watchdog-lib.sh` содержит path classifiers. Но этот план ориентирован на merge/watchdog и не является удобным единым интерфейсом локальной проверки для человека или агента.

### Как адаптировать

Не писать второй selector. Добавить developer-facing команду, которая переиспользует существующие classifiers и выводит:

- точный base/head и merge-base;
- изменённые bounded contexts;
- выбранные static/TypeScript/Rust/deployment lanes;
- обязательные специальные сценарии и документационные owners;
- причины выбора и ожидаемые команды;
- machine-readable JSON для агентов и human-readable summary.

Следующим шагом команда может запускать выбранный DAG с кэшем и bounded concurrency. Источником истины остаются те же функции, которые использует production gate.

### Польза

- Локальная проверка совпадает с будущим merge plan.
- Агент не запускает полный workspace без причины и не забывает специальный тест.
- Диагностика «почему запустилась эта lane» становится явной.
- Можно измерять стоимость каждой проверки и оптимизировать самый дорогой критический путь.

### Риск слепого копирования

Второй независимый TypeScript-runner поверх shell pipeline создаст расхождение двух политик. Первая версия должна быть только адаптером над текущим `validation-plan.sh`/classifiers.

### Пилот и критерий успеха

Пилот: `deploy/check-change.sh --base <sha> --head <sha> --plan-only`, который печатает JSON и summary и имеет fixture matrix на существующие классификаторы.

Критерий успеха: для всех regression fixtures локальный plan и merge validation envelope совпадают; неизвестный путь включает полный fail-closed набор; median локального feedback time уменьшается без пропуска обязательной lane.

## Кандидат 8 (P1). Инженерные решения с явным жизненным циклом

### Идея из DeepSeek Harness

`.agents/notes/README.md` задаёт единый формат Agent Notes и состояния `proposed`, `implemented`, `rejected`, затем frozen archive. Каждая запись содержит проблему, решение или предложение, рассмотренные альтернативы и последствия. Текущие контракты остаются в docs/code, а note хранит то, что код не объясняет: почему выбран этот вариант и что было сознательно отвергнуто.

### Текущее положение у нас

В нашем репозитории есть сильные design documents, execution journals, audits и статусы внутри больших контрактов. Однако единого формата для решения, предложения, исторического снимка и действующего контракта нет. Из-за этого план может остаться рядом с реализованной системой, а rationale может быть смешан с текущей инструкцией.

### Как адаптировать

Ввести узкий Decision Record, а не обязательную заметку для каждого нетривиального коммита. Запись нужна, если меняется:

- архитектурная граница или dependency direction;
- межконтекстный, wire, durable или configuration contract;
- delivery/testing/process policy;
- security/privacy/money invariant;
- решение, которое будущий разработчик с высокой вероятностью попытается отменить.

Предлагаемая структура: `docs/decisions/{proposed,implemented,rejected}/<class>/YYYY-MM-DD-<topic>.md`. При принятии нужно одновременно обновить правила организации документации и `docs/README.md`. Минимальные секции: `Problem`, `Decision`/`Proposal`, `Alternatives considered`, `Consequences`/`Risks`, `Verification`.

Действующий contract не дублируется в record: он остаётся в доменном документе, а record ссылается на него. Реализованная запись поддерживает только факты, необходимые для понимания решения; исторические execution details архивируются или удаляются по отдельной политике.

### Польза

- Снижает повторное обсуждение уже отвергнутых вариантов.
- Отделяет «что система обязана делать» от «почему мы выбрали именно это».
- Делает статус незавершённых планов однозначным.

### Риск слепого копирования

Требование записи для каждого изменения породит бюрократию и дубли. Нужен порог по типу решения, а не по размеру diff. Не нужно копировать обязательную двуязычность и сложный frozen-manifest DeepSeek Harness на первом этапе.

### Пилот и критерий успеха

Пилот: оформить по новому шаблону следующие три реальные cross-cutting решения и перенести только один существующий документ, статус которого сейчас трудно определить.

Критерий успеха: reviewer может по одному пути определить статус, выбранный вариант, отвергнутые альтернативы и ссылку на текущий contract; ни один факт действующего API не поддерживается в двух местах.

## Кандидат 7a (P1). Матрица тестов по риску

### Идея из DeepSeek Harness

DeepSeek Harness планирует unit, real composition, snapshot, built-artifact и real-API tiers по типу изменения. `docs/testing.md` отдельно требует проверять внешний мир, а не self-report системы, и мокать только дорогую или недетерминированную границу. Per-file coverage используется как один сигнал, но не заменяет assembled behavior.

### Как адаптировать

Вместо общего требования «добавить тесты» завести небольшой risk manifest для критичных поверхностей:

| Класс | Минимальное доказательство |
|---|---|
| Деньги/settlement/pricing | Unit/property + реальная PostgreSQL transaction/concurrency + assembled entry scenario. |
| Auth/security/privacy | Положительный и отрицательный тест через реальный guard/perimeter; прямой вызов service недостаточен. |
| Межконтекстный contract | Producer serialization + consumer parse + один совместный scenario. |
| Streaming/retry/cancellation | Реальный stream/subprocess/socket, первая публичная граница и teardown/quiescence. |
| UI с денежными или operator решениями | Component behavior + deterministic browser/preview review для критичного journey. |
| Deployment controller | Поведенческий regression test, который использует реальную функцию/секцию, а не только grep-count. |

Manifest может связать path glob с обязательным test group в существующем path-aware gate. Это расширяет текущую классификацию, а не заменяет её.

### Польза

- Тестовая глубина соответствует blast radius.
- Устраняется ложное чувство безопасности от большого числа unit-тестов вокруг чистых helper-функций.
- Критичная ветка получает тест на том уровне, где реально принимается решение.

### Риск слепого копирования

Глобальный порог 100% per-file coverage для зрелого смешанного Rust/TypeScript монорепозитория даст дорогой и легко «играемый» сигнал. Лучше сначала измерить changed-file coverage и закрыть risk surfaces, а не объявлять 100% целью.

### Пилот и критерий успеха

Пилот: две поверхности — engine Control API money operations и один worker money loop. Для каждой зафиксировать tiers, добавить отсутствующее доказательство и связать path с test group.

Критерий успеха: намеренное удаление guard/transaction fence/consumer field ломает соответствующий tier; gate явно сообщает, какое risk rule выбрало тест.

## Кандидат 7b (P1). Единый hygiene-контур

### Идея из DeepSeek Harness

DeepSeek Harness запускает не только build/test/typecheck, но и lint, clone detection (`jscpd`), dead-code/export анализ (`knip`), package publication checks (`publint`) и workspace constraints. Быстрые staged checks отделены от исчерпывающего CI.

### Текущее положение у нас

В корневом `package.json` есть build/typecheck/test, а lint определён только в отдельных приложениях. Rust build/test является обязательным, но единый `cargo fmt`/`clippy` policy не описан в корневом gate. Публикационный `publint` нам в полном виде не нужен, поскольку основные workspace packages внутренние.

### Как адаптировать

Ввести path-aware hygiene lane:

- Rust: `cargo fmt --check`; затем `cargo clippy` для изменённых crates/all targets с контролируемым ratchet по существующим warnings;
- TypeScript: единая lint-конфигурация или общий runner для изменённого package closure;
- dead exports/dependencies: `knip` сначала report-only, затем blocking для новых нарушений;
- duplication: `jscpd` только для нового/изменённого кода с baseline, без требования немедленно очистить весь репозиторий;
- package constraints: workspace protocol, допустимые зависимости bounded contexts, обязательные scripts для deployable packages.

Быстрый pre-commit hook может проверять только staged whitespace/format/lint. Полные tests остаются в merge gate, чтобы hook не дублировал дорогую работу. Как в DeepSeek Harness, hook-path нужно устанавливать через worktree-local Git config: настройка одного агента не должна менять hooks соседних worktree. Первая версия должна только отклонять нарушение или исправлять строго staged-файлы с повторной проверкой diff; скрытые repository-wide auto-fix недопустимы.

### Польза

- Ловит мёртвый код, потерянные test attributes, новые warnings и копирование логики до production.
- Унифицирует качество между приложениями.
- Уменьшает стоимость будущих refactor и аудитов.

### Риск слепого копирования

Включение `-D warnings`, полного `knip` и clone threshold одним коммитом почти гарантирует большой несвязанный cleanup. Нужен ratchet: baseline фиксирует существующий долг, gate запрещает новый.

### Пилот и критерий успеха

Пилот: `fmt`, changed-crate `clippy`, общий TypeScript lint и report-only `knip`/duplication. Через 2–4 недели стабильного сигнала перевести новые нарушения в blocking.

Критерий успеха: менее 5% ложных срабатываний/ручных bypass; нет роста baseline; hygiene lane не увеличивает p95 merge gate больше чем на 15% благодаря параллельному запуску.

## Кандидат 4 (P0). Инцидент → постмортем → defensive rule → regression gate

### Идея из DeepSeek Harness

`docs/postmortem/` хранит причинную цепочку конкретного инцидента, а `docs/defensive-patterns.md` — короткие правила классов ошибок: teardown должен дождаться quiescence, callback exception локализуется в dispatcher, независимые outcomes сообщаются отдельно, untrusted output не получает ambient env. Каждое правило возникло из реального дефекта и связано с тестовой политикой.

### Текущее положение у нас

У нас есть подробные audits, provider journals, deployment runbooks и множество сильных защитных комментариев. Но audit — широкий снимок, а не incident-specific causal record. Hard-won lessons могут остаться в одном длинном документе или комментарии и не стать общим правилом для следующего компонента.

### Как адаптировать

Добавить короткий шаблон постмортема для production/near-miss инцидентов:

1. impact и detection;
2. точная причинная цепочка;
3. почему существующие проверки пропустили дефект;
4. исправление;
5. постоянный regression test/gate;
6. одно обобщённое defensive rule, только если оно применимо более чем к одному месту.

Общий документ defensive patterns должен содержать только доказанные классы ошибок. Не нужно превращать каждую ошибку в универсальное правило.

### Польза

- Процесс учится на дефекте механически, а не только текстом.
- Новые агенты получают компактный список опасных lifecycle/concurrency/deploy паттернов.
- Postmortem объясняет происхождение правила, не раздувая `AGENTS.md`.

### Риск слепого копирования

Постмортем на каждый мелкий bug создаст шум. Порог: production impact, security/money risk, повторяемый near miss или дефект, который прошёл существующий gate.

### Пилот и критерий успеха

Пилот: выбрать два недавних инцидента разных классов, оформить causal records и добавить по одной доказанной regression-проверке.

Критерий успеха: постмортем указывает конкретный старый пробел gate; новая проверка краснеет на воспроизведении причины; обобщённое правило имеет минимум два применимых места либо остаётся только в постмортеме.

## Предлагаемый порядок внедрения

### Этап 0 — baseline и дизайн сценариев, 3–5 дней

- Зафиксировать p50/p95 времени текущих static, Rust, TypeScript и deployment lanes.
- Выбрать один Control API assembled scenario и стабильные поля keyless transcript.
- Зафиксировать текущие docs link/index ошибки, архитектурные инварианты и hygiene debt без блокировки merge.
- Добавить лёгкий postmortem template без обязательного gate; применить его к следующему подходящему инциденту.

### Этап 1 — доказательство shipped path, 1–2 недели

- Запустить реальный Control API ↔ EngineClient scenario через собранный release-кандидат и disposable PostgreSQL.
- Добавить один router → engine → mock upstream keyless replay с non-stream/SSE semantic guards.
- Привязать оба сценария к существующему path classifier.

### Этап 2 — единый scope и быстрые механические проверки, 1–2 недели

- Вывести developer-facing change plan из существующих classifiers.
- Добавить repository invariant gate с тремя правилами.
- Добавить Markdown link/anchor/index checks и targeted ownership для пяти критичных документных поверхностей.

Каждая проверка сначала получает regression fixtures. Изменение selector/gate должно fail-safe включать полный набор, как и сейчас.

### Этап 3 — качество и память решений, 2–4 недели

- Hygiene lane в report-only режиме, затем ratchet.
- Узкий Decision Record lifecycle.
- Компактный defensive-patterns owner на основе уже оформленных postmortems.
- Ограниченный no-growth budget для входных документов, но не для runbook/provider reference.

## Метрики результата

Через 6–8 недель после пилота стоит оценить:

| Метрика | Желаемое направление |
|---|---|
| Изменения критичной поверхности, прошедшие с несвязанным `.md` | 0 |
| Broken links/anchors и непроиндексированные документы в `master` | 0 |
| Архитектурные нарушения, найденные только ручным аудитом | Снижение; каждый повторяемый класс превращается в gate. |
| Дефекты producer/consumer composition после merge | Снижение; каждый критичный contract имеет assembled scenario. |
| p50 локального feedback для обычного diff | Снижение за счёт единого change plan и узких проверок. |
| p95 merge gate | Не растёт более чем на 15%; независимые проверки выполняются параллельно. |
| Новые lint/dead-code/duplication нарушения | 0 после включения ratchet. |
| Исключения и bypass в новых gates | Низкое и убывающее число; каждое исключение именовано и обосновано. |

## Что из DeepSeek Harness копировать не стоит

### Архитектуру «всё является плагином»

Наши bounded contexts, Rust dependency direction и HTTP-границы соответствуют продукту. Переход к plugin-first архитектуре будет переписыванием runtime, а не улучшением процесса разработки.

### 100% per-file coverage как немедленный глобальный gate

Для нового TypeScript harness это может быть экономически оправдано. Для нашего зрелого Rust/TypeScript production-монорепозитория сначала важнее risk-based tiers, assembled paths и changed-scope ratchet. Процент покрытия не доказывает правильную композицию.

### Обязательную Agent Note для каждого нетривиального изменения

Нам нужен decision record для решений, которые будут пересматриваться, а не дополнительный документ к каждой локальной правке. Иначе living-contract discipline превратится в дублирование.

### Двуязычную документационную машину

DeepSeek Harness поддерживает English/Chinese pairs, pairing sidecars и отдельные merge rules. У нас другой набор продуктовых языков и нет требования переводить инженерную документацию попарно. Такой контур дорог и не закрывает текущий главный риск.

### PR stacks, label taxonomy и их GitHub workflow

Наш `master` является production trigger, merge сериализован, а exact-SHA watchdog является финальным доказательством. Перенос PR-stack процесса добавит ещё одну модель координации без устранения существующего bottleneck.

### Полный набор package-publication проверок

`publint`, NodeNext consumer matrix и npm payload policy полезны для публичной библиотеки. Их нужно применять только к реально публикуемым package/artifact surfaces, а не ко всему внутреннему workspace.

### Полную runtime/platform matrix DeepSeek Harness

Node 22.19/24/26, Python и Windows/Wine отражают заявленную совместимость DeepSeek Harness. Наш текущий runtime-профиль — Rust и Node 24. Добавлять соседние версии или ОС нужно только вместе с явным продуктовым обязательством их поддерживать, а не ради симметрии с источником.

### Self-modification и экспериментальную агентную координацию

Cordis self-modification demo, experimental Agent Teams, private maintainer buses и конкретная GitHub runner failover topology решают задачи самого harness и его организации. Они не улучшают наш обычный coding/delivery process без отдельной подтверждённой потребности.

### Полностью auto-fixing hooks

Полезны worktree-local конфигурация и быстрые staged checks. Не нужно переносить скрытые repository-wide auto-fix действия: hook не должен менять несвязанные файлы или маскировать итоговый staged diff.

### Pre-release позицию «ломать совместимость свободно»

DeepSeek Harness находится в developer preview. У нас production-данные, деньги и внешние клиенты; expand-only миграции и producer-first контракты остаются обязательными независимо от удобства refactor.

## Итоговая рекомендация

Начать с одного ограниченного process-hardening изменения, которое объединит:

1. один assembled Control API scenario через собранный release-кандидат;
2. один keyless router → engine → mock replay с семантическими guards;
3. developer-facing `change plan`, переиспользующий текущие classifiers;
4. `repository-invariants` с тремя проверками и targeted docs ownership/link checks;
5. лёгкий шаблон incident → guardrail для следующего escaped production/near-miss дефекта.

Этот пакет переносит главный принцип DeepSeek Harness — «важное правило должно иметь исполняемое доказательство» — и сохраняет сильные стороны нашего production delivery. Decision records и hygiene ratchet стоит добавлять после того, как первый пакет даст стабильный сигнал и измеренное время выполнения.

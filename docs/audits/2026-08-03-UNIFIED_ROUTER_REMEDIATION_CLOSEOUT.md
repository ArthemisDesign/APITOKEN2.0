# Unified router remediation closeout — 2026-08-03

## Вывод

Все одиннадцать внутренних дефектов, зафиксированных в
`docs/audits/2026-08-03-UNIFIED_ROUTER_PRODUCTION_READINESS.md`, устранены отдельными
production-пакетами и подтверждены на точных SHA. Повторный статический, contract, negative-live и
real-harness аудит новых внутренних дефектов не выявил.

Три исходных пункта не являются незакрытыми runtime-дефектами router:

- UR-09 — ограничение cost schema OpenCode/AI SDK; серверный settlement и key-scoped pricing
  остаются точными, а изменение цен или отдельный pricing rollout не входили в remediation;
- UR-13 — реализованный fallback намеренно остаётся default-off до отдельного canary/GA решения;
- UR-14 — Roo Code не публикует официальную headless surface, поэтому live-case остаётся честным
  `SKIP`, а не подменяется другим клиентом.

Fallback GA не включался. Тарифы, multipliers, скидки и pricing releases этим remediation не
менялись.

## База и доставка

- Исходный аудит: `aca74981d00b2f8c883188043e2a0bff4b96473e`.
- Последний remediation SHA: `7da055ac8db1d23ddc3ee77981cb6c8759c03401`.
- Каждый пакет прошёл `deploy/agent-merge.sh`, exact-SHA gate и зелёный production
  `deploy/watchdog` до начала следующего зависимого пакета.

| Пункт | Статус | Production SHA | Результат |
|---|---|---|---|
| UR-01 | закрыт | `1c7288a`, `ce63cc1` | ранняя key/account preflight до body и bounded universal-body admission |
| UR-02 | закрыт | `0a274b9` | единая strict positive-integer matrix для translated output limits |
| UR-03 | закрыт | `7da055a` | локальный router blue-green 8800/8801, stable 8802, exact binary, cutover до drain |
| UR-04 | закрыт | `8f093f7` | fail-closed Code Assist JSON Schema subset translator с точным schema pointer |
| UR-05 | закрыт | `78558cd` | GPT `reasoning_content` replay-safe, reasoning-only assistant turn не ломает replay |
| UR-06 | закрыт | `0a274b9` | неправильный JSON-тип `stream` больше не превращается в `false` |
| UR-07 | закрыт | `78558cd` | translated streams завершаются успешно только при доказанном provider terminal state |
| UR-08 | закрыт | `fee441a`, `982fde1` | authoritative limits/capabilities проходят plane → router → OpenCode без model tables |
| UR-09 | внешнее ограничение | — | OpenCode 1.18.11 не выражает cache-write и произвольный long-context threshold |
| UR-10 | закрыт | `f772036`, `710cda7`, `cc7d9d4` | key/base-bound encrypted capability-only last-good cache без stale pricing |
| UR-11 | закрыт | `ce63cc1` | bounded timeout ожидания response headers без total timeout длинного SSE |
| UR-12 | закрыт | `982fde1` | глобальная alias-уникальность; ambiguous alias не получает order-dependent routing |
| UR-13 | release gap | — | fallback остаётся default-off; GA требует отдельного canary и явного enable |
| UR-14 | внешний coverage gap | — | Roo transport/config совместим, но официального headless CLI нет |

Дополнительный packaging-дефект OpenCode, при котором несколько ESM exports воспринимались как
несколько plugins, закрыт SHA `4eade697`: entrypoint экспортирует только default factory.

## Повторная проверка

### Exact-SHA gates и deployment

Последний пакет UR-03 прошёл полный forced gate после rebase: весь Rust workspace, весь TypeScript
workspace, shell/docs/static lanes и deployment regression suites. Trusted host validation и
production watchdog завершились GREEN на одном и том же SHA `7da055a`. Blue-green regression
matrix отклоняет dual-active, wrong-binary, legacy steady-state и недоступный stable origin.

### Production harness matrix

На уже развёрнутом router повторно выполнен `tests/router_harness_live_matrix.sh`. Зелёные все 21
исполняемый Standard/Fast case:

- Cline, Continue, OpenCode, Kilo, Codex, Claude Code, Gemini CLI, Hermes и Aider;
- чистый OpenCode → Gemini multi-turn bash tool replay без sanitizer/request rewrite;
- чистый OpenCode Claude main/title flow;
- Opus 5 `xhigh` и `max`;
- GPT Fast через camelCase body, canonical body и header selectors;
- native Responses, Messages и Gemini Developer API surfaces.

Roo Code 3.54.0 обнаружен установленным и снова отмечен `SKIP`: расширение имеет совместимые
OpenAI base/model/tier settings, но не имеет официального headless CLI.

### Negative live matrix

Bounded запросы к production подтвердили fail-closed поведение до billable execution:

- `stream:"false"` на Anthropic Chat, Gemini Chat и Gemini Messages → 400;
- нулевые, отрицательные и строковые output limits на Chat/Responses → 400 с точным `param`;
- Gemini `patternProperties` → локальный 400 с
  `tools.0.function.parameters/patternProperties`;
- внутренний `x-apitoken-execution-state` не вышел ни в одном router response;
- GPT reasoning-only assistant replay → 200;
- неавторизованный медленный 32 MiB upload получил 401 после 512 KiB/926 ms, не дожидаясь body.

### Catalog и штатный OpenCode

Key-scoped production `/v1/models` вернул 27 записей: authoritative limits, reasoning efforts,
service tiers и aliases присутствуют; межпровайдерных alias collisions нет. Канонический plugin
зарегистрировал 28 исполнимых OpenCode model entries с учётом синтетических GPT Fast entries.

Проверка текущего OpenCode выявила не серверный дефект, а некорректный локальный default ID
`google/gemini-3.6-flash#high`: OpenCode 1.18.11 трактует `#high` как часть model ID. После
возврата к каноническому `google/gemini-3.6-flash` запуск с установленным plugin прошёл с exit 0 и
нулём ошибок; effort-вариант штатно задаётся отдельно (`--variant high`) и также прошёл live-run.

## Остаточный риск и критерий повторного открытия

Remediation можно считать закрытым для текущего default single-model контракта unified router.
Повторное открытие требуется только если появится одно из следующих новых оснований:

1. поддерживаемая OpenCode cost extension, способная точно выразить cache-write и произвольный
   long-context threshold — тогда UR-09 закрывается отдельным client/provider пакетом без изменения
   тарифных значений;
2. отдельное продуктовое решение включить fallback GA — тогда выполняются canary, money/header
   evidence и config-only rollout из UR-13;
3. официальная Roo automation/headless surface — тогда `SKIP` заменяется воспроизводимым bounded
   live-case.

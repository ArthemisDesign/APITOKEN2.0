# Unified router resilience audit — 2026-08-03

Статус: **append-only snapshot до production landing**. Аудит выполнен на базе
`c9f50bfb6570154257a1602b54b5998f3a701538`; remediation находится в сопровождающем изменении.
Exact master SHA и watchdog verdict фиксируются в отчёте доставки, а не подменяются локальным
mock/build evidence.

## Вывод

`router.apitoken.sale` сохраняет правильную базовую архитектуру: stateless HTTP bounded context
агрегирует три независимые provider-plane, а auth, billing, reserve/settlement, retry внутри
подписки и PostgreSQL authority остаются в движке. По сравнению с прямыми per-provider доменами он
даёт один endpoint, namespaced каталог, одинаковые universal surfaces, персональные цены и
fail-closed explicit fallback. Прямые домены при этом остаются рабочими и не зависят от router.

До remediation новая точка входа имела десять существенных resilience/scale gaps: медленные
chunked bodies могли несправедливо забрать memory budget, metadata producers не имели полных
hostile bounds/singleflight, pricing не масштабировался дальше 256 моделей, auth и `/balance`
последовательно зависели от первого origin, blue-green проверял только process-local readiness,
Caddy не подписывал доказанный `no healthy upstream`, а telemetry не позволяла различить эти
классы отказов. Отдельно OpenCode рекламировал generated-image output, который его фактический
OpenAI-compatible decoder не умеет принять.

Remediation закрывает найденные классы без переноса billing в router, без provider execution
semaphore/очереди и без включения default-off fallback. Во время повторного ревью найден и закрыт
дополнительный memory-lifetime дефект: parsed advanced-routing template теперь остаётся под тем же
admission permit до terminal response headers, а single-model tree удаляется до outbound upload.

## Как работает итоговая схема

1. Universal POST сначала делает bodyless auth race к fixed origins. Первый exact schema-v1 200
   или 401 завершает race; transport/404/5xx/malformed inconclusive. Provider-plane всё равно
   повторно проверяет credential до reserve.
2. Известный `Content-Length` сразу получает округлённый MiB-вес. Unknown/chunked начинает с
   одного unit и fail-fast растёт по фактическим байтам; общий budget — 64 units, request limit —
   32 MiB, body deadline — 15 секунд без прогресса и 5 минут абсолютного времени. Очереди
   ожидания нет.
3. Namespaced single-model request выбирает plane без каталога; alias/fallback получает aggregate
   snapshot. Каталог ограничен 4 MiB, 1 024 моделями на plane и 256 байтами на ID/name, обновляется
   per-plane singleflight с общим success/failure in-flight результатом, TTL 27/30/33 секунд и
   last-good.
4. Key-scoped pricing остаётся uncached. Большой каталог режется на чанки по 256; любой failed
   chunk закрывает весь overlay, поэтому частичная/нулевая ставка не публикуется.
5. Billable fallback по-прежнему возможен только по exact `not_started` либо TCP
   `ConnectionRefused`. Stable Caddy origin синтезирует `not_started` только на собственном
   `no healthy upstream`; runtime 503 не получает его. Публичные provider vhost'ы header снимают.
6. `/balance` — исключение как bodyless read-only path: transport/header-timeout/5xx продолжаются
   Anthropic → OpenAI → Gemini, а 401 и любой non-5xx терминальны.
7. Candidate router допускается только после exact binary, direct `/ready` и loopback-only
   `/startup`, подтверждающего точный unauthenticated provider auth contract. После promotion
   `/startup` повторяется через stable 8802 до drain старого slot.

## Реестр находок и исправлений

| ID | Severity | Дефект | Исправление / acceptance |
|---|---|---|---|
| RA-01 | P0 | Unknown body заранее резервировал 32 MiB, а общий 15-секундный deadline отклонял допустимый медленный upload | Динамический 1 MiB admission, 15 секунд idle + 5 минут max, overload без queue; два реальных 32 MiB body всё ещё честно исчерпывают 64 units |
| RA-02 | P0 | После раннего release parsed fallback body мог остаться в heap вне admission accounting | Single-model tree удаляется до upload; advanced template удерживает permit до terminal headers; SSE body permit не держит |
| RA-03 | P1 | Stable origin без healthy slot не давал безопасного fallback proof | Caddy handler-only `not_started`; runtime 503 остаётся unsigned; public provider hop снимает internal header |
| RA-04 | P1 | Каталог мог выделить до 4 MiB без model-count/string bounds; failed refresh создавал последовательный convoy ожидающих callers | 4 MiB/1 024/256-byte bounds, control-char rejection, success/failure singleflight без negative cache, skewed TTL, last-good |
| RA-05 | P1 | Aggregate catalog >256 моделей целиком ломал pricing projection | Детерминированные chunks по 256, ordered merge, whole-overlay fail-closed |
| RA-06 | P1 | Sequential early auth добавлял до трёх двухсекундных задержек | Concurrent first-conclusive auth; healthy later plane не ждёт зависший первый origin |
| RA-07 | P1 | `/ready` доказывал только слушающий router process | Exact `/startup` provider data-path probe до и после promotion |
| RA-08 | P1 | `/balance` был жёстко привязан к Anthropic runtime | Bodyless read-only 5xx/transport failover, terminal 401/non-5xx, response passthrough |
| RA-09 | P1 | Admission/authority/timeout failures почти не наблюдались | Fixed-cardinality metrics, три alerts и matching runbooks без key/model/request labels |
| RA-10 | P1 | OpenCode обещал image output, который его Chat decoder отбрасывает | Plugin публикует только text output; native Gemini image route остаётся рабочим и billable |

## Что стало лучше прямой схемы

- Один base URL и единый key-scoped catalog вместо ручного выбора provider hostname.
- Namespaced IDs исключают alias hijack; presets/preferences/policy применяются до attempt 1.
- Partial provider outage не делает router liveness конъюнкцией всех plane; catalog использует
  last-good, auth и read-only balance умеют обходить зависший origin.
- Explicit fallback имеет два проверяемых pre-execution доказательства и durable single-winner
  fencing в engine authority; router не угадывает billable outcome.
- Blue-green deployment проверяет не только PID/HTTP listener, но и минимальный provider contract.
- Ресурсные и authority failures теперь имеют bounded telemetry и runbook.

## Что остаётся хуже или дороже

- Unified hostname добавляет один локальный hop и singleton router process. Blue-green убирает
  release outage, но multi-host HA не заявлен; прямые provider domains имеют меньший blast radius.
- Universal dispatch обязан materialize JSON до 32 MiB ради model/planner rewrite. Native routes
  продолжают стримить body напрямую и предпочтительнее для больших media payloads.
- Alias/fallback вызывает каталог/policy, а `/v1/models` — uncached personalized pricing; при
  холодном или большом каталоге это дороже прямого provider discovery.
- First-conclusive auth предполагает согласованную shared authority. Одновременные contradictory
  200/401 являются producer incident; реальная plane admission остаётся последней защитой.
- Generated images работают через native Gemini API, но не через текущий OpenCode
  `@ai-sdk/openai-compatible` transport. Рекламировать несовместимую capability запрещено.
- Cross-provider fallback остаётся default-off до отдельного exact-SHA live canary; этот аудит его
  не включает побочным эффектом.

## Проверки

До коммита зелёны:

- `cargo test -p claude-router`: 116/116, включая dynamic admission, idle/max deadline,
  success/failure singleflight, outbound EOF, open SSE, terminal balance 401 и fencing matrix;
- `cargo build`, `rotation_fanout_smoke.sh`, `universal_chat_smoke.sh` и
  `router_fallback_smoke.sh`;
- `pnpm install --frozen-lockfile`, полный `pnpm build`, `pnpm typecheck`, `pnpm test`, включая
  6/6 тестов canonical OpenCode plugin;
- `cargo test --locked --workspace` через repository sccache wrapper: все исполняемые тесты
  зелёные (в том числе forward 782/782, один штатный Redis test ignored; router 116/116);
- shell syntax, `watchdog-lib.test.sh`, `monitoring-config.test.sh` (67 alert/runbook anchors на
  audit-base; 72 после композиции с текущим master) и `git diff --check`;
- официальный Caddy 2.11 runtime fixture: `handle_errors 503` подписывает только Caddy
  `no healthy upstream`, public outer proxy снимает internal header, а runtime-produced 503
  остаётся unsigned.

После единственного коммита ещё обязательны exact-range `docs-check`, штатный merge gate,
`deploy/watchdog GREEN` на landed SHA и post-deploy stable/public verification. Незапущенная
проверка не считается доказательством.

Destructive memory/load probe на production и платная image generation в рамках remediation не
запускаются: resource edges доказываются barriered loopback mocks, а нативный image wire/money
contract уже принадлежит provider-plane и не менялся. Post-deploy verdict должен отдельно
подтвердить stable origin и отсутствие internal header на публичных provider vhost'ах.

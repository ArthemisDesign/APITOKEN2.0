# План масштабирования больших API payloads

Статус: **предложение к реализации; текущий production-контракт не изменён**.

Область: публичные model API endpoints `router.apitoken.sale`, `api.apitoken.sale`,
`openai.api.apitoken.sale`, `gemini.api.apitoken.sale` и их внутренний путь
Caddy → unified router → provider plane → provider transport.

Не входит в этот план: увеличение контекстного окна модели, тарифов, quota подписки,
provider-owned media limits и внутренних admin/catalog/error bounds, не ограничивающих customer
model payload.

## 1. Решение

Цель обновления — не механически заменить `32 MiB` на большое число, а безопасно открыть:

- до **256 MiB одного текстового JSON request body** там, где upstream/provider реально принимает
  такой запрос;
- до **256 MiB одного materialized non-stream response body** в переводящих adapters;
- высокую параллельность маленьких и средних запросов без блокировки несколькими максимальными;
- fail-fast защиту каждого процесса от OOM;
- disk-backed buffering для больших request bodies;
- бинарный Rust↔Node IPC для Gemini без base64 amplification;
- последующее горизонтальное шардирование provider workers.

Целевой максимум 256 MiB — локальный transport envelope, а не обещание, что любая модель примет
256 MiB содержимого. Effective limit равен минимуму всех звеньев, включая context window и
provider-owned upstream contract.

**Не допускается** сначала поднять константы и `MemoryMax`, а защиту памяти добавить потом. Новый
публичный предел включается только после weighted admission, bounded spooling, transport framing,
метрик и нагрузочного доказательства на том же exact SHA.

## 2. Классы лимитов

Перед реализацией каждый найденный limit относится к одному из трёх классов.

1. **Локальный capacity/safety limit** — можно поднимать после соответствующей защиты ресурсов.
2. **Provider/product contract** — нельзя поднимать локально: это создаст ложное обещание и лишь
   перенесёт отказ дальше по пути.
3. **Control-plane/diagnostic bound** — намеренно мал и не связан с размером model payload.

Это предотвращает ошибку «поднять всё», при которой 64 KiB error parser или 4 MiB catalog body
превращаются в новую DoS-поверхность без пользы клиенту.

## 3. Текущая карта и целевые значения

### 3.1 Публичный ingress и unified router

| Граница | Сейчас | Цель GA | Действие |
|---|---:|---:|---|
| Caddy model-vhost request body | явного cap нет | 256 MiB | Явно поставить streaming cap на четырёх model vhosts; не включать buffering |
| Universal request body | 32 MiB | 256 MiB | Поднимать после spooling и weighted admission |
| Router body admission | 128 MiB raw, шаг 1 MiB | 4 GiB aggregate raw/spool + отдельный 4 GiB estimated-RSS budget | Разделить disk/storage и memory budgets; оба fail-fast, без очереди |
| Body idle timeout | 60 s | 120 s | Timeout только при отсутствии byte progress |
| Body absolute upload timeout | 5 min | 30 min | Позволяет 256 MiB на медленном uplink; не является generation timeout |
| In-memory threshold | всё materialized | 8 MiB | Более крупные тела немедленно spill в anonymous 0600 file |
| Router process memory | `MemoryMax=512M` | `MemoryHigh=6G`, `MemoryMax=8G` | Только вместе с admission и cgroup alerts |
| Router disk-spool budget | отсутствует | 16 GiB на active slot, 256 MiB/request | Atomic reservation до записи, cleanup на drop/startup; отдельный filesystem/project quota является authority, не только in-process counter |
| Router response body | stream | stream | Не вводить aggregate cap и не буферизовать SSE |

Router memory budget учитывает не только raw bytes. Каждый materialized JSON получает
консервативный estimated-RSS weight; коэффициенты калибруются нагрузочным тестом для ASCII,
escaped Unicode, base64 и глубоко вложенных tool schemas. До калибровки действует верхний
коэффициент, доказанный worst-case тестом, а не оптимистичный `raw_bytes == RSS`.

### 3.2 Provider planes

| Plane/surface | Сейчас | Цель | Примечание |
|---|---:|---:|---|
| Anthropic native Messages request | 32 MiB | **32 MiB, оставить** | Provider-owned public Anthropic contract |
| Anthropic-compatible Chat/Responses request | 32 MiB | 32 MiB effective | Локальное увеличение не создаёт upstream capacity |
| Anthropic translated non-stream response | 32 MiB | 256 MiB local envelope | Только weighted response admission; output-token contract не меняется |
| OpenAI/Codex Chat/Responses/Messages request | 8 MiB | 256 MiB local envelope, staged provider proof | Нынешние 8 MiB — локальный parsing bound, но upstream acceptance надо доказать |
| Codex combined instructions | 1 MiB | 16 MiB | Отдельный structural cap внутри 256 MiB body |
| Codex custom tool grammar | 256 KiB | 4 MiB | Оставить bounded независимо от body max |
| Codex app-server JSONL frame | 32 MiB | 384 MiB | 256 MiB payload + JSON/framing overhead; incremental cap до allocation |
| Codex stored history entry | 16 MiB | 256 MiB | Одновременно увеличить Redis history capacity и eviction monitoring |
| Gemini text native/universal request | 32 MiB | 256 MiB | После binary IPC и Gemini weighted admission |
| Gemini inline-media/image request | 20 MiB | **20 MiB, оставить** | Документированный provider-owned media ceiling |
| Gemini non-stream/generated-image response | 64 MiB | 256 MiB | Отдельный response admission; streaming остаётся streaming |
| Gemini Rust↔Node decoded request | 32 MiB | 256 MiB | Binary framing, без base64 JSON line |
| Gemini Rust↔Node metadata frame | часть 48 MiB NDJSON | 1 MiB | Headers/URL/control только; body идёт отдельным binary frame |
| Gemini IPC binary body frame | отсутствует | 256 MiB | Length-prefixed, exact request ID, bounded before allocation |
| Provider process memory | `MemoryMax=2G` | Anthropic/OpenAI: `High=6G Max=8G`; Gemini: `High=12G Max=16G` | Нужны parent-slice caps для blue-green overlap |
| Provider request spool | отсутствует | 16 GiB/active plane | Quota и cleanup аналогичны router |

### 3.3 Намеренно не увеличивать

Следующие значения не являются customer text-payload bottleneck и остаются прежними, если
отдельная продуктовая задача не докажет обратное:

- router auth response 1 KiB;
- router policy request/response 64 KiB и 32 candidates;
- router pricing response 1 MiB, producer request 256 KiB и 256 candidates;
- catalog response 4 MiB, 1,024 models, 256-byte IDs/names;
- diagnostic/error bodies 64 KiB;
- Gemini pre-public stream prelude 1 MiB и chunk-count bounds;
- OpenAI image generation JSON 256 KiB;
- OpenAI image edit/reference limits и decoded PNG bounds;
- Gemini inline media 20 MiB;
- model input/output token limits из authoritative catalog;
- quota/RPM/concurrency Google, Anthropic и OpenAI;
- generation deadline: для Gemini customer generation production default остаётся `0` (без
  эвристического timeout), lifetime определяется disconnect/keepalive/upstream close.

Suno/Tripo3D upload/artifact bounds принадлежат отдельным media products и не меняются этим
text/model API rollout.

## 4. Обязательная архитектура до увеличения

### 4.1 Единый typed limit contract

Добавить leaf crate `crates/api-limits` без HTTP, env, network и provider dependencies. Он хранит:

- размерные newtypes (`ByteLimit`, `AdmissionUnits`) с checked arithmetic;
- production defaults и hard compile ceilings;
- route classes: universal text, Anthropic text, OpenAI text, Gemini text, Gemini media,
  image generation/edit, control-plane;
- human-readable formatter для стабильных error messages;
- проверку связей: per-request ≤ spool budget ≤ hard ceiling, response ≤ process budget,
  binary-frame envelope ≥ request body + framing overhead.

`crates/router` может импортировать этот независимый leaf crate, не нарушая запрет на импорт
`forward`/`registry`. `forward` и `server` используют те же типы/defaults. Env по-прежнему читается
только в `crates/router/src/config.rs` для router и `crates/server/src/config.rs` для engine.

Настройки должны быть strict decimal MiB/seconds с fail-closed startup validation; malformed,
zero или несогласованные значения не откатываются молча к default.

Предлагаемые operator keys:

```text
CLAUDE_ROUTER_MAX_BODY_MIB=256
CLAUDE_ROUTER_BODY_MEMORY_BUDGET_MIB=4096
CLAUDE_ROUTER_BODY_SPOOL_BUDGET_MIB=16384
CLAUDE_ROUTER_BODY_MEMORY_THRESHOLD_MIB=8
CLAUDE_ROUTER_BODY_IDLE_SECS=120
CLAUDE_ROUTER_BODY_MAX_SECS=1800

CLAUDE_API_TEXT_BODY_MAX_MIB=256
CLAUDE_API_BODY_MEMORY_BUDGET_MIB=4096
CLAUDE_API_BODY_SPOOL_BUDGET_MIB=16384
CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=8
CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=256
```

Общий provider default 4 GiB рассчитан для Anthropic/OpenAI slot с `MemoryHigh=6G`; Gemini unit
может reviewed argv-pin отдельные 8 GiB только вместе с `MemoryHigh=12G`/`MemoryMax=16G`. Startup
проверяет, что estimated-RSS budget не превышает 70% `MemoryHigh`, оставляя baseline/runtime запас;
процесс не пытается сам читать cgroup как источник конфигурации — systemd values и app values
проверяются deployment regression test как одна пара.

Provider-owned narrower caps всегда имеют приоритет над общим envelope. Production values
фиксируются в reviewed systemd argv assignments, чтобы shared env не мог незаметно расширить
публичную поверхность одного plane.

### 4.2 Bounded body storage

Добавить общий utility в `crates/forward` и отдельный аналог в router либо независимый storage
модуль leaf crate без HTTP:

- auth завершается до чтения body;
- честный `Content-Length` резервирует spool bytes сразу;
- unknown/chunked начинает с малого unit и расширяет reservation по фактическим bytes;
- request `Content-Encoding` запрещён на materializing text routes; предел считается по фактическому
  несжатому JSON и не допускает gzip/br decompression bomb до admission;
- первые 8 MiB хранятся в memory, затем body переводится в anonymous/unlinked 0600 file;
- declared и observed overflow возвращают lane-native error до provider dispatch;
- idle и absolute deadlines работают по byte progress;
- disconnect, parse failure, cancellation и panic-safe RAII освобождают memory/disk units и файл;
- spool root имеет OS-level filesystem/project quota; in-process reservation улучшает admission, но
  не считается защитой от crash leftovers или второго процесса;
- startup удаляет только принадлежащие конкретному slot stale files после проверки owner/mode/path;
- symlinks, path traversal и reusable public filenames запрещены;
- disk full и reservation exhaustion дают fail-fast 503 без billable attempt;
- advanced fallback может перечитать spool, но не создаёт N полных in-memory copies.

Для systemd slot units добавить `StateDirectory`/`ReadWritePaths` для отдельных slot spool roots.
Blue-green candidate не делит writable files с active slot.

### 4.3 Два admission budget вместо одного semaphore

Каждый materializing процесс получает:

1. **storage budget** — фактические raw bytes в memory + spool;
2. **estimated-RSS budget** — оценка одновременно живых buffers/trees/encoded frames.

Оба budget:

- weighted;
- fail-fast (`try_acquire_many`), без ожидания и request queue;
- увеличиваются по мере чтения chunked body;
- могут менять weight между фазами без окна без reservation;
- освобождаются после фактического уничтожения соответствующего allocation, не после отправки
  клиенту headers;
- публикуют current/max/rejections по fixed route class, без model/key/request labels.

В Gemini weight должен покрывать одновременно Rust raw/native/wrapped representations и Node
binary buffer. В OpenAI — raw/parsed/prepared JSON, JSON-RPC frame и stored history. Коэффициенты
не hardcode навсегда: defaults compile-fixed, но GA values выбираются по profiler evidence.

### 4.4 Router fast path без полного дерева

Для обычного namespaced single-model request router не должен строить полное
`serde_json::Value` на сотни MiB только ради `model`:

- incremental top-level parser извлекает только routing selectors и валидирует JSON;
- неизвестные payload fields пропускаются через bounded parser без allocation (`IgnoredAny`/raw
  ranges), оригинальные bytes остаются в memory/spool;
- если model уже namespaced и body не требует alias/service-tier/fallback rewrite, оригинал
  стримится в plane;
- rewrite path использует bounded streaming transformer или materialization под estimated-RSS
  admission;
- advanced `models`/`provider` сохраняет точную fallback semantics, но template хранится в spool и
  на каждую попытку создаётся не более одного transformed stream;
- недопустимо ослабить strict JSON, duplicate routing-field detection или spoofed internal-header
  stripping.

### 4.5 Убрать повторный Gemini adapter parse

Сейчас Chat/Responses/Messages adapter сериализует внутренний HTTP request, после чего
`gemini_api()` снова читает и парсит его. Вынести typed internal executor:

```text
validated public request
  → NativeGeminiRequest { operation, model, value/body, stream, typed context }
  → shared admission/reserve/affinity/rotation/wrapper/settlement executor
```

Native `/v1beta` handler и три universal adapters вызывают один executor. Синтетический внутренний
HTTP request удаляется. Typed logical ID, lifecycle clock, client attribution, auth, billing и
error envelope должны сохраниться без обходного пути.

### 4.6 Gemini binary IPC v2

Полностью заменить base64 NDJSON request-body transport:

- transport меняется с line-oriented `readline` на binary-safe framed stdin/stdout state machine;
  `readline` не может сосуществовать с raw body bytes на том же pipe;
- control frame: маленький length-prefixed JSON или fixed binary header;
- затем exact-length raw body frame;
- maximum metadata 1 MiB, maximum body 256 MiB, checked `u64 → usize`;
- reader отвергает oversize по header до allocation/read;
- request ID связывает metadata/body/cancel/response;
- один writer mutex сохраняет frame atomicity; разные active requests продолжают multiplexing;
- response headers/control остаются bounded;
- response data передаётся binary chunks (целевой max chunk 1 MiB), а не base64 strings;
- backpressure сохраняется end-to-end, stdout queue не может бесконечно накапливать chunks;
- cancel во время body upload/response уничтожает только соответствующий Node socket;
- secrets и short-lived buffers zeroize там, где это применимо;
- helper startup по-прежнему проверяет SHA-pinned Node version/platform/arch/Undici;
- `env_clear`, proxy isolation, HTTP CONNECT, TLS/JA3/JA4 и exact actual-send attestation не меняются;
- dual IPC v1/v2 не сохраняется: Rust и embedded helper поставляются одним immutable binary и
  переключаются атомарно blue-green.

### 4.7 Codex large-body path

До поднятия `OPENAI_BODY_LIMIT`:

- поднять incremental JSON-RPC reader/writer bounds до envelope, достаточного для 256 MiB request;
- доказать cap-before-allocation на 384 MiB + 1 без реального giant fixture;
- избежать `Vec` clone на retry/preparation;
- расширить stored history entry и Redis history capacity согласованно;
- обновить `deploy/affinity-redis.compose.yaml`: history maxmemory с 512 MiB до **8 GiB** на первом
  rollout, после production cardinality/size evidence — до **16 GiB**, если eviction остаётся;
- affinity Redis 128 MiB не увеличивать автоматически: он не хранит conversation bodies;
- подтвердить private Codex app-server acceptance отдельным controlled canary. До него production
  OpenAI public cap остаётся 8 MiB, даже если transport готов к 256 MiB.

### 4.8 Process и host resource envelope

Добавить systemd parent slices, чтобы одновременные active+candidate slots не могли сложить
индивидуальные maxima и вытеснить хост:

| Component | Slot `MemoryHigh` / `MemoryMax` | Parent slice `MemoryHigh` / `MemoryMax` |
|---|---:|---:|
| Router | 6 / 8 GiB | 9 / 12 GiB |
| Anthropic | 6 / 8 GiB | 9 / 12 GiB |
| OpenAI | 6 / 8 GiB | 9 / 12 GiB |
| Gemini | 12 / 16 GiB | 18 / 24 GiB |

Дополнительно для четырёх data-plane classes:

```text
LimitNOFILE=262144
TasksMax=8192
OOMPolicy=stop
```

Перед запуском inactive candidate controller проверяет host `MemAvailable`, parent-slice current и
spool free space. При недостаточном headroom deploy остаётся на старом slot и не начинает candidate.
После cutover старый slot drain ограничен существующим settlement-safe `TimeoutStopSec`; увеличение
upload body не даёт права сокращать stream drain.

Сумма parent maxima должна повторно проверяться при каждом изменении RAM/сервисов. Эти числа
рассчитаны для текущего 96 GiB host и не являются переносимыми defaults.

## 5. Порядок изменений по файлам

### 5.1 Общий contract/config

Создать/изменить:

- `crates/api-limits/{Cargo.toml,src/lib.rs}` — checked units, defaults, ceilings, route classes;
- workspace `Cargo.toml`/`Cargo.lock` — новый leaf member/dependency;
- `crates/router/Cargo.toml`, `crates/forward/Cargo.toml`, `crates/server/Cargo.toml`;
- `crates/router/src/config.rs` — strict router settings;
- `crates/server/src/config.rs` — strict provider settings;
- `config.env.example` — документированные knobs и safe-current defaults;
- `crates/router/CLAUDE.md`, `crates/forward/CLAUDE.md`, `crates/server/CLAUDE.md`;
- `docs/engine/UNIFIED_ROUTER.md`, `docs/engine/CODEX_PROVIDER.md`,
  `docs/engine/GEMINI_PROVIDER.md`, `docs/engine/ROUTING_FENCING.md`.

### 5.2 Router

- `crates/router/src/routing.rs` — bounded storage, dual admission, deadlines, incremental route
  extraction, spool-backed upload/template;
- `crates/router/src/main.rs` — state budgets and startup validation;
- `crates/router/src/metrics.rs` — byte/RSS/spool/admission metrics;
- `crates/router/src/error.rs` — стабильные lane-shaped oversized/storage/overload errors;
- `crates/router/src/{chat,responses,messages}.rs` — route-class plumbing;
- `crates/router/src/bounded.rs` — shared bounded response reads не используют public text cap;
- `crates/router/src/proxy.rs` — streamed file body и cancellation lifetime без response buffering;
- `crates/router/src/tests.rs` — boundary, slow/chunked/disconnect/fallback/memory cases;
- `tests/universal_chat_smoke.sh`, `tests/router_fallback_smoke.sh` — end-to-end mirrors;
- новый `tests/large_payload_smoke.sh` + generator, который генерирует payload на лету.

Catalog/auth/policy/pricing bounds не должны случайно использовать новый public text limit.

### 5.3 Provider adapters

- `crates/forward/src/proxy.rs` — shared bounded body storage и Anthropic 32 MiB provider cap;
- `crates/forward/src/anthropic.rs`, `anthropic_responses.rs` — разделить request provider cap и
  non-stream response envelope;
- `crates/forward/src/codex/{api,chat,skin,process,runner,transport,history}.rs` — OpenAI body,
  structural bounds, JSONL envelope и history;
- `crates/forward/src/gemini/{api,chat,responses,skin}.rs` — 256 MiB text envelope, typed executor,
  20 MiB media exception, 256 MiB response envelope;
- `crates/forward/src/state.rs` — provider admission/spool state;
- `crates/forward/src/gemini/config.rs` — typed limits in `GeminiConfig`, без env reads в lower layer;
- `crates/forward/src/metrics.rs` — fixed-cardinality admission/bytes/IPC metrics;
- `crates/server/src/http.rs` — route middleware использует named constants, а не рассыпанные
  `DefaultBodyLimit::max(...)`; internal control limits остаются отдельными;
- `crates/server/src/main.rs` — budgets, spool startup cleanup и shutdown drain.

### 5.4 Gemini transport

- `crates/forward/src/gemini/transport.rs` — IPC v2 framing, binary streams, cancellation,
  backpressure, bounds and tests;
- `crates/forward/src/gemini/node_transport.cjs` — binary parser/writer, no base64 body/data,
  unchanged TLS/proxy identity;
- `crates/forward/src/gemini/pool.rs` — phase-aware admission permit lifetime per profile request;
- Gemini API/pool/transport tests — multiplex, partial frame, hostile length, disconnect, helper
  restart/no-replay and exact-send attestation.

### 5.5 Deploy и systemd

- `deploy/Caddyfile` и `deploy/CADDY.md` — explicit 256 MiB model-vhost cap, no buffering, internal
  headers unchanged;
- `systemd/claude-router{,@}.service` — memory, files/tasks, StateDirectory/spool;
- host spool mount/quota definition и installer validation — hard 16 GiB per active slot, free-space alert;
- `systemd/claude-api-{anthropic,openai,gemini}{,@}.service`, а также retained
  `systemd/claude-api{,@}.service`, `claude-api-openai.service` и `claude-api-gemini.service`
  rollback units — те же mirrors без расхождения;
- новые `systemd/claude-{router,anthropic,openai,gemini}.slice` — aggregate blue-green caps;
- `deploy/router-bluegreen.sh`, `deploy/engine-bluegreen.sh` — pre-start headroom/spool checks;
- `deploy/deploy.sh`, `deploy/watchdog.sh`, `deploy/watchdog-lib.sh`,
  `deploy/watchdog-lib.test.sh`, `deploy/shutdown-ladder.test.sh` — install/classification scope,
  exact unit/slice values, rollback and drain invariants;
- `deploy/affinity-redis.compose.yaml`, Redis alert/dashboard docs — Codex history capacity.

### 5.6 Observability

Добавить fixed-cardinality metrics:

```text
claude_router_request_body_bytes_bucket{surface}
claude_router_body_storage_bytes{kind="memory|spool"}
claude_router_body_memory_cost_bytes
claude_router_body_spool_files
claude_router_body_admission_rejections_total{reason}
claude_api_body_storage_bytes{provider,kind}
claude_api_body_memory_cost_bytes{provider}
claude_api_body_admission_rejections_total{provider,reason}
claude_api_gemini_ipc_bytes_total{direction,kind}
claude_api_gemini_ipc_active_requests
claude_api_gemini_ipc_protocol_failures_total{reason}
```

Buckets доходят до 256 MiB, но labels не содержат key/account/model/request ID. Изменить:

- `observability/prometheus/rules/application.yml` — saturation, rejection, spool leak, memory-high,
  IPC failure alerts;
- `observability/grafana/dashboards/production-overview.json` — body p50/p95/p99, active weighted
  bytes, cgroup current/high/max, rejections и helper IPC;
- `observability/prometheus/prometheus.yml`/node-exporter textfile collector — cgroup memory events и
  spool filesystem, если текущих series недостаточно;
- `docs/ops/MONITORING.md` — отдельный runbook section на каждый новый alert;
- `docs/DEPENDENCIES.md` — обновить observability producer→consumer lines, если добавляются новые
  metrics/alerts.

Чеклист `docs/CHANGE_CHECKLISTS.md#new-alert-or-metric` выполняется полностью в каждом commit,
который добавляет alert/metric.

## 6. Тестовая стратегия

### 6.1 Unit/property tests

Не выделять сотни MiB в каждом unit test. Все readers/admissions получают test config с малыми
границами, затем проверяется одинаковая арифметика production constants.

Обязательные cases:

- exact `limit-1`, `limit`, `limit+1` для declared и chunked bodies;
- ложный меньший/больший `Content-Length`;
- integer overflow и hostile frame length до allocation;
- 1-byte chunks, stalled upload, absolute timeout, disconnect;
- spill threshold boundary и disk-full/reservation exhaustion;
- malformed/deep/escaped/base64 JSON;
- permit release после parse/translation/upstream/connect/cancel errors;
- advanced fallback перечитывает template без N retained copies;
- native/SSE response остаётся unbuffered;
- IPC partial metadata/body, interleaved IDs, cancel race, blocked stdout, helper death;
- exact-target request не replay после ambiguous IPC flush;
- startup cleanup не следует symlink и не удаляет чужие files;
- public cap никогда не переопределяет narrower Anthropic/Gemini-media cap.

### 6.2 Mock integration/load harness

`tests/large_payload_mock_load.py` потоково генерирует declared/chunked тела для loopback
mock/candidate без giant fixture или retained full body. `deploy/large-payload-evidence.sh` снимает
content-free cgroup memory/events, FD и leaked-spool evidence для указанного slot unit. Raised-default
commit обязан хранить exact-SHA output этих инструментов во внешнем deployment evidence journal.

Harness генерирует, не сохраняя в git:

- body 8/32/64/128/256 MiB и отдельный compressed-request rejection matrix;
- ASCII text, escaped Unicode, base64 and large tool schemas;
- Content-Length и chunked variants;
- Chat, Responses, Messages/count_tokens, Gemini native;
- stream/non-stream response 1/64/128/256 MiB;
- concurrency 1/4/8/16/32/64/128/256;
- mixed workload: 95% 64 KiB, 4% 8 MiB, 0.9% 64 MiB, 0.1% 256 MiB;
- disconnect at upload 25/75%, before headers and mid-SSE;
- blue-green candidate start/drain while load continues.

Снимать cgroup `memory.current`, `memory.peak`, `memory.events`, CPU, disk bytes/latency, open FDs,
helper RSS, request latency и admission counters.

### 6.3 GA acceptance

Exact candidate SHA допускается к повышенным defaults только если:

1. `oom`, `oom_kill`, `memory.max` events и leaked spool files равны нулю.
2. Peak steady workload ниже `MemoryHigh`; worst-case burst остаётся ниже `MemoryMax` с запасом ≥20%.
3. Ни один 256 MiB request не создаёт неучтённый allocation больше принятого profiler coefficient.
4. Маленький-request p99 ухудшается не более чем на 10% под mixed load.
5. Admission overload возвращается fail-fast и не создаёт queue convoy.
6. Client disconnect освобождает router, plane и Node socket/resources.
7. SSE first-byte/framing и terminal usage/settlement остаются точными.
8. Billing reserve/settle queue и PostgreSQL latency остаются внутри текущих alerts.
9. Caddy, router, plane и helper сообщают один согласованный effective cap/error.
10. Full workspace, deployment, docs и monitoring gates зелёные.

## 7. Последовательность production delivery

Это один feature train, но не один огромный commit. Каждый шаг достигает production GREEN до
следующего.

### Commit 1 — checked contract и dormant config

- Добавить dependency-free `api-limits`, strict settings и current-cap parity tests.
- Production defaults оставить текущими 32/128/2G и OpenAI 8 MiB; hard ceilings не являются
  enablement и выше-current значения fail-closed.
- Не публиковать фиктивные spool/RSS/IPC series до появления соответствующих runtime-механизмов.

### Commit 1b — честная baseline observability

- Добавить fixed-cardinality histograms фактически materialized request bodies и реальные
  oversize/read/admission rejection counters на текущих путях.
- Обновить dashboards, alerts/runbooks и dependency map; spool/RSS/IPC gauges остаются за
  commits, которые создают соответствующие authorities.
- Собрать минимум 7 дней body-size/concurrency evidence; RSS coefficients отдельно доказываются
  profiler/cgroup load harness перед raised defaults.

### Commit 2a — leaf-примитивы bounded storage и weighted admission

- Добавить dependency-light `crates/bounded-body`: fail-fast atomic weighted budgets, single-owner
  RAII reservations и private memory→anonymous-file storage с checked growth/rollback.
- Runtime call sites, config, metrics, systemd и public errors не менять; публичные пределы прежние.
- Малые unit tests доказывают threshold/limit boundaries, one-byte chunks, replay, exhaustion,
  panic/drop cleanup, private root/mode и отсутствие content/path в Debug.

### Commit 2b — интеграция router bounded storage

- Universal bodies получают два независимых fail-fast budget: raw storage и estimated memory;
  auth/deadlines/errors/permit lifetime остаются прежними.
- Каждый slot получает private mode-0700 RuntimeDirectory и обязательный абсолютный spool root;
  `/tmp` fallback запрещён. Threshold пока равен текущему 32 MiB request cap.
- Публичные пределы и `MemoryMax=512M` прежние; доказать отсутствие protocol/money/SSE изменений.

### Commit 2c — интеграция provider bounded storage

- Native Anthropic Messages идёт первым: после auth/allowlist и до parse/reserve он получает два
  независимых 2 GiB fail-fast budget и slot-private spool capability; cap/threshold остаются 32 MiB.
- Anthropic universal Chat/Responses следующими удерживают те же reservations через parse,
  translation и внутренний native `forward`, сохраняя OpenAI-shaped errors и текущий 32 MiB cap.
- Codex native Responses следующим получает shared bounded reader под текущим 8 MiB cap; остальные
  Chat/Messages/count paths мигрируют отдельно с собственными prepared/history weights.
- Затем Gemini materializers получают spooling и caller-supplied estimated RSS после typed IPC,
  сохраняя отдельные text/media caps.
- Публичные пределы пока прежние; доказать отсутствие protocol/money изменений до raised default.

### Commit 3a — Gemini binary IPC v2

- Единый binary framed stdin/stdout protocol заменяет NDJSON/readline и base64 request/response
  data: bounded JSON control, raw exact-length request frame и raw response chunks.
- Multiplex/cancel/no-replay/actual-send semantics и SHA-pinned Node fingerprint неизменны.
- Limits пока 32/20/64 MiB; 256 MiB остаётся только transport ceiling.

### Commit 3b — Gemini typed executor

- Universal adapters передают один раз переведённый `NativeGeminiRequest` через trusted typed
  extensions; synthetic body serialization и повторный parse удалены.
- Shared native executor сохраняет HTTP shell только для route/auth/context/error/SSE contracts.
- Exact transport fingerprint и money/error/SSE matrix должны быть GREEN.

### Commit 4 — systemd slices и resource headroom gate

- Установить per-slot и aggregate caps, spool directories, FD/task limits.
- Перед стартом inactive router/Anthropic candidate проверять host MemAvailable, свободное место
  private spool filesystem и current/max parent slice; fail-closed без cutover.
- Candidate должен пройти mock large-payload load до cutover; raised public defaults остаются
  отдельным последующим commit после нагрузочного evidence.

### Commit 5 — поднять router + Gemini text/response до 256 MiB

- Caddy model cap 256 MiB.
- Router universal 256 MiB, 4 GiB aggregate raw + 4 GiB estimated-RSS + 16 GiB spool budgets,
  8 MiB threshold, 120 s/30 min upload.
- Gemini text/request/IPC/non-stream response 256 MiB; media остаётся 20 MiB; Gemini unit получает
  отдельный reviewed 8 GiB estimated-RSS budget под 12/16 GiB cgroup envelope.
- Free native `countTokens` canary на large text body первым.
- Любая paid large-input generation требует отдельного явного budget authorization; успешный
  `countTokens` не доказывает generation.

### Commit 6 — Codex transport/history capacity

- Расширить JSONL/history/Redis/admission, но OpenAI public body default оставить 8 MiB.
- Провести controlled private app-server acceptance по размерным ступеням.

### Commit 7 — поднять OpenAI public text envelope

- Включать последовательно 32 → 64 → 128 → 256 MiB только до максимума, реально принятого private
  provider transport на exact implementation SHA.
- Не рекламировать 256, если upstream proof остановился раньше.

### Commit 8 — Anthropic response envelope

- Request cap остаётся 32 MiB.
- Поднять только локальный translated non-stream response envelope под weighted admission.

Rollback каждого шага — предыдущий immutable release/config; нельзя откатывать billing/schema
историю. Oversized requests снова получают прежний deterministic 4xx/413, уже начатые billable
requests не replay.

## 8. Горизонтальное масштабирование после vertical rollout

Высокий будущий трафик нельзя обслуживать только ростом `MemoryMax` одного процесса.
Следующая фаза:

- несколько active router replicas за Caddy;
- provider worker shards с disjoint subscription/profile ownership;
- Gemini helper/profile принадлежит ровно одному active shard;
- deterministic affinity выбирает shard, затем profile;
- shared PostgreSQL billing/fencing остаётся authority;
- shared Redis хранит только opaque affinity/cooling hints;
- shard-local weighted admission не блокирует другие provider planes;
- external load balancer и multi-host требуют synchronous multi-AZ PostgreSQL, как уже зафиксировано
  в `docs/ops/INFRASTRUCTURE.md`.

Критерий перехода к sharding: sustained `MemoryHigh` >70%, admission rejection >0.1%, p99 growth
>20% либо provider helper/CPU saturation при наличии upstream quota headroom.

## 9. Definition of Done

Обновление завершено, только когда одновременно выполнено:

- единый checked limit contract используется всеми customer data routes;
- текущие и effective per-provider limits документированы и возвращаются согласованно;
- router и provider planes имеют storage + estimated-RSS admission;
- большие bodies spill на bounded private storage;
- ни один text route не принимает request `Content-Encoding`: compressed request bodies остаются
  запрещены fail-closed, чтобы 256 MiB wire cap не превращался в decompression-bomb bypass; response
  decompression учитывает decoded bytes в том же response admission;
- Gemini binary IPC не использует base64 для body/response data;
- повторный Gemini adapter parse удалён;
- systemd per-slot и parent-slice budgets установлены и наблюдаемы;
- Caddy имеет explicit streaming cap без SSE buffering;
- все тесты §6 и exact-SHA candidate load gate GREEN;
- alerts/runbooks/dashboard развернуты до raised defaults;
- production canary не показывает OOM, leak, billing/settlement drift или upstream retry regression;
- Anthropic/provider media/control bounds не были ложно расширены;
- production docs (`UNIFIED_ROUTER`, provider docs, monitoring, Caddy/systemd runbooks) обновлены в тех
  же implementation commits;
- каждый commit доставлен через `deploy/agent-merge.sh`, а `deploy/watchdog` GREEN на его exact SHA.

До выполнения этого Definition of Done production остаётся на текущих значениях; один лишь большой
объём RAM хоста не считается защитой от unbounded concurrent materialization.

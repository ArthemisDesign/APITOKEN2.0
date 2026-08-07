# GPT Image 2 private Codex OAuth canary

This runbook covers the private GPT Image 2 canary through the existing sealed Codex OAuth pool. It
adds no image API key, relay, image origin, or image-specific environment variable. It is not an
`AppState` member, HTTP/customer route, catalog entry, router preset, billing path, production default,
public capability, or publication authorization.

Generation and edit use the native subscription endpoints:

- `POST {CodexConfig.base_url}/images/generations` for a prompt without references;
- `POST {CodexConfig.base_url}/images/edits` when `--reference` is supplied one to five times.

Both request `model: "gpt-image-2"`, `background: "opaque"`, and `size: "auto"`. `quality` is `low`
by default; `--quality medium|high` probes a higher tier against its exact official output-token
ceiling (the 16/48/96-cell formula), and the returned metadata is what decides whether the wire
honors the tier. Edit adds strict PNG data URLs under `images[].image_url`; each reference carries
its own whole-envelope input authorization. Production evidence shows
that the native Codex OAuth endpoint converts an explicit request size to an automatic provider-selected
size, so this path intentionally makes no deterministic-dimension claim. It does not prove masks,
partial-image streaming, JPEG/WebP output, output compression, multiple outputs, or Responses API
multi-turn image state. Those features remain absent rather than being silently ignored.

## Pool and retry contract

The canary uses the normal Codex configuration, sealed OAuth roster, proxy, refresh family, official
client identity, health and quota admission. `--profile` may name an opaque roster id; if omitted, the
runner selects the first currently admitted opaque id without dispatching an image request. It then
freezes that id, runs the existing free `/wham/usage` OAuth/quota preflight, and performs at most one
logical exact-home image attempt.

A received first `401` permits the existing single forced refresh and one same-home replay with the
same `x-codex-image-turn-id`. The canary never rotates the paid call to another home. A final auth/quota
rejection, client rejection, unexpected status, invalid success, timeout, connection/body failure, or
other ambiguous outcome is terminal. An ambiguous attempt is never replayed.

## Auto-size generation ceiling

The private generation request has a conservative OpenAI API replacement-price ceiling of
`22_330_000` nanoUSD (`$0.02233`):

```text
659 low image-output tokens × $30 / 1M          $0.01977
512 prompt bytes × $5 / 1M, treated as tokens    0.00256
                                                    -------
maximum authorized replacement estimate         $0.02233
```

The 659-token output bound is the exhaustive maximum of the official GPT Image 2 low-quality token
calculator over every request-valid size: both edges divisible by 16, maximum edge 3840, 655,360 to
8,294,400 pixels, and aspect ratio at most 3:1. Returned native auto dimensions need not be divisible by
16—the production endpoint returned `1254x1254`—but remain bounded by the same edge, pixel and aspect
envelope. This deliberately treats every allowed UTF-8 prompt byte as one fresh text token. It is not
ChatGPT native-credit pricing, a customer reserve, or settlement authority. The runner accepts only
integer `--budget-nanousd`, requires it to exceed the repository default `100000` nanoUSD, and permits
paid generation only when it is at least `22330000`.

OpenAI publishes no GPT Image 2 high-fidelity input-token formula, so PNG bytes are still not treated as
tokens and no expected edit price is invented. The official model page does publish a maximum Tier-5
rate limit of 8,000,000 TPM. One accepted request cannot exceed that whole minute's token admission
envelope; the canary conservatively charges all 8,000,000 tokens at the more expensive fresh image-input
rate and then adds the independently bounded prompt and low output:

```text
8,000,000 tokens × $8 / 1M, all treated as image input   $64.00000
maximum prompt + low image output                          0.02233
                                                           --------
absolute one-reference authorization envelope            $64.02233
```

## Quality and reference-count probe ceilings

The same official token calculator generalizes by quality: the 16-cell low grid becomes 48 cells for
medium and 96 for high. The exhaustive maxima over every request-valid resolution are 659 / 5,930 /
23,719 output tokens, so the exact generation ceilings are `22_330_000` / `180_460_000` /
`714_130_000` nanoUSD, and each edit reference adds its own `64_000_000_000` envelope. `--quality`
selects the probed tier; a successful checkpoint must echo the requested quality, so a wire that
silently normalizes the tier (as it does for size) produces a sanitized `evidence_controls_mismatch`
journal with the returned controls and usage instead of a false success.

## Surface probe gate

`deploy/gpt-image-2-surface-probe-gate.sh` is the one-shot host controller that runs the control
probes on the production pool, pinned to the probe-capable producer SHA through the fixed root
bridge. It performs exactly three paid operations, each in its own fenced subdirectory of
`/var/lib/apitoken/watchdog/gpt-image-2-surface-probe/<sha>`: a `medium` generation (budget
`180_460_000`), a `high` generation (budget `714_130_000`), and a two-reference `low` edit against
two static embedded 1024x1024 PNGs (budget `128_022_330_000`, two whole input envelopes). A probe
verdict is `honored` (exact checkpoint echoing the requested control), `normalized` (sanitized
controls-mismatch journal with the returned controls and usage), or `rejected` (clean pre-dispatch
refusal); `outcome_unknown` or malformed evidence fails the delivery instead of reporting a verdict.
The watchdog validates the bounded JSON summary and publishes one ≤140-character status per probe.

Paid edit therefore requires at least `64_022_330_000` nanoUSD per run — one whole reference envelope
plus the generation ceiling — and each additional `--reference` (up to five) adds its own
`64_000_000_000` envelope. This is a fail-closed authorization ceiling, not an expected charge,
ChatGPT native-credit price, customer reserve, or assertion that a real edit uses eight million
tokens. A request below its exact ceiling reports
`paid_dispatch_requires_edit_ceiling_authorization` before configuration or network access.

## Local file and evidence contract

Validation happens before any network access:

- `--profile`, when present, must be an opaque Codex roster profile id, never an email/account id.
- `--prompt-file` must be a stable regular non-symlink Unix file with exact mode `0600`, containing
  nonempty UTF-8 of at most 512 bytes and 512 Unicode characters.
- Each repeated `--reference` must be a stable regular non-symlink PNG, 1..=16 MiB. Animation,
  dimensions outside 1..=4096, decoded size above 16 MiB, more than five references, and aggregate
  transport/decoded overflow are rejected.
- `--output` must end in `.png`; `--checkpoint` must end in `.json` and have a UTF-8 basename. They must
  differ and not already exist. Their parent directories must already exist, be non-symlinks, and not
  be group- or world-writable.
- Internal recovery state is created in an exclusive mode-`0700` run directory. Artifacts are written
  and synced as mode-`0600`, then externally published without overwrite.

Dry-run omits `--execute`. It reads no Codex environment/configuration, makes no request, and creates no
artifact. Its sanitized JSON plan contains the operation, profile selector (`auto-admitted` when
omitted), fixed controls, counts, authorization budget, required ceiling, blocker, and compile-time SHA
when available; it never contains prompt text, references, credentials, account identity, or paths.

Paid generation additionally requires an exact lowercase 40-hex compile-time
`CLAUDE_API_IMPLEMENTATION_SHA`. A successful checkpoint requires all of:

- exact frozen home and image turn identity;
- one bounded, fully decoded PNG within maximum edge 3840, 655,360–8,294,400 pixels and aspect ratio
  at most 3:1;
- returned `background=opaque`, `quality=low`, and `size=auto` or the decoded PNG's exact
  `WIDTHxHEIGHT`; `output_format`, when present, must be `png`;
- terminal numeric usage after allow-list sanitization;
- the locally generated image turn id; a sanitized provider request-id header is retained when present
  but is optional because the official Codex `ImageResponse` contract does not require one;
- output SHA-256 and exact implementation SHA.

Missing usage or mismatched home, local turn, or controls receive a specific `evidence_*` journal state.
For a parsed mismatch, the private mode-`0600` journal retains only returned exact-home/turn flags,
dimensions, timestamp, controls, allow-listed numeric usage, sanitized optional request id, and image
SHA-256. The rejected image bytes are neither persisted nor published, and the journal is diagnostic—not
publication evidence. Prepared, transport-error, and success journals do not contain this `returned`
object; successful authoritative evidence remains in the separate checkpoint.

## Commands

Generation dry-run, allowing the pool to select and freeze an admitted home:

```bash
claude-api openai-image-canary \
  --prompt-file /private/canary/prompt.txt \
  --output /private/canary/result.png \
  --checkpoint /private/canary/checkpoint.json \
  --budget-nanousd 22330000
```

The first implementation SHA `3f67d43c0ae541979fee66823d251e2e3eea33e0` is deployed and
watchdog-GREEN, but its separately authorized `$0.00856` attempt is complete and withdrawn. Controller
delivery `7a334604f41c367e898073567a7aa0d481614839` stopped before network access because it tried to source
systemd syntax from `config.env` as Bash. Delivery
`2c7dabcce85be9a691597d6b5ab765fe4868a3b6` reached a parsed image result but withheld it because the
provider supplied no optional request-id header. Its exact recovery root is a terminal non-replay fence.

Corrected implementation SHA `012fccc471142fc51a46563da3a87564d674b39f` is independently
watchdog-GREEN. Delivery `d7b394fc5e6b9b603e1e0ab3982038f5479ba2e8` then made its one
authorized `$0.00856` generation through the sealed pool. The endpoint returned a parsed image, but the
returned metadata did not exactly echo `opaque/low/1024x1024`; the canary recorded
`evidence_controls_mismatch`, published neither PNG nor checkpoint, and permanently fenced the attempt.
That older recovery journal intentionally contains no returned controls or usage, so the attempt cannot
prove which field differed and cannot be promoted as partial evidence. The controller now performs only a
non-network verification of that exact SHA/budget/journal and the absence of all output artifacts; it
cannot replay the paid call. The diagnostic journal contract described above applies only to a future
attempt running an implementation that contains it; it does not retroactively enrich this fenced journal.

Diagnostic implementation SHA `8fcd7c3c6f5dc968bedb7260433f2eaff23f8931` is independently
watchdog-GREEN. Active watchdog-GREEN descendant `3c17b31b6dfdcb8867d8def57e7aedc4ebc87644`
has an empty diff against that SHA across the image canary and Codex image transport files. Its completed
one-shot delivery used a fresh SHA-keyed root and the then-applicable explicit `8_560_000` nanoUSD
fixed-size ceiling. Before dispatch it required that exact active binary, the existing free `/wham/usage`
preflight path, the sealed pool environment from that running OpenAI slot, and both ClaudeStore fallback
switches forced off. It could dispatch one exact-home generation only. Full exact evidence would have
produced GREEN; a parsed mismatch produced a terminal sanitized journal with no image artifacts. Neither
branch can be replayed. Controller deliveries `3ba2d941e95419748027bf5fc8a0759821095148` and
`e0618cca78b6b5a650f9a8399c5457572bb44568` stopped before preflight and paid dispatch; the latter passed
policy installation but supplied a different mutable engine SHA to the exact-argument sudo bridge.
Delivery `237a926b054a5fdd6833fca6668040ab6e0d55a7` made the authorized exact-home call through the sealed
pool. The native endpoint returned an opaque/low PNG result with exact home and turn, terminal usage
(`35` input, `229` image output, `264` total tokens), and `1254x1254` dimensions instead of requested
`1024x1024`. The canary retained only the sanitized `evidence_controls_mismatch` journal and digest; it
persisted and published no image or checkpoint. A jq syntax error initially prevented the controller from
accepting that valid terminal journal. The corrective controller is strictly non-network for this SHA:
it verifies the existing journal and absence of artifacts before loading runtime credentials or reaching
the dispatch path. The paid image turn remains permanently non-replayable.

Auto-size implementation SHA `df58715abb4f1ac52b6c46b1ea6f830c6e11178f` and controller delivery
`afcfca46e22d3b123540462c9b20a2249dc9a56b` are watchdog-GREEN. The one-shot used a fresh SHA-keyed
private root and `22_330_000` nanoUSD for one `opaque/low/auto` exact-home generation. It produced a
real bounded PNG with terminal numeric usage, exact home/turn/SHA attribution, and digest-matched
internal and external evidence. That immutable `generation.png` and its successful checkpoint are the
only accepted reference provenance for the edit gate; withdrawn attempts remain ineligible.

Edit-capable implementation SHA `1c48e3769f0fe775e650f60ea3c5839458e5dfe2` is independently
watchdog-GREEN. The one-shot edit delivery `8357ec764d1cdddff652ae4b5d6221267eb14f4e` closed the exact
attempt fence, and fail-closed corrective verifier SHA `354832bc86c3a8365e713faf0f35ad2c239c7087`
is watchdog-GREEN. The corrective path is non-network and accepts only the existing exact success
checkpoint; it would instead validate and fail overall delivery for a terminal withdrawal. Therefore the
owned generation PNG was consumed exactly once as one strict data-URL reference and produced a bounded,
byte-different edited PNG with positive terminal image-input/image-output usage, consistent sums,
returned controls, and exact home/turn/SHA attribution. Exact edit token values were not extracted from
the retained summary and are not inferred here.

## Publication gate

Private generation and edit gates are GREEN. Producer Images API SHA
`d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6` is deployed and watchdog-GREEN, still dormant in every
public catalog. Its metered success header carries the engine reservation identity so the one-shot can
correlate the exact release snapshot, reservation, outbox, usage event and terminal settlement.

Public gate delivery `0dbbfdda054a1a7bda709434c8678b192bf12276` is RED at
`verifying-gpt-image-2-public`. Non-network inspector delivery
`5a16ce96e2d1aef242055e88aa5d38f152d0ecd5` proved the retained journal is exactly `preflight`, both
`generation_dispatched` and `edit_dispatched` are false, and both request identities are null. No paid image
operation was dispatched. The producer-SHA evidence root remains permanently fenced. The corrective
`deploy/gpt-image-2-public-smoke-gate.sh <producer-sha> --inspect` path may accept this exact safe withdrawal
or complete retained success, but cannot load runtime credentials or execute the CLI. A later paid one-shot
must use a new producer SHA and a new root.

Successor producer `d42fc0e3290c0042a16797626326c250e0f6721c` is deployed and watchdog-GREEN. It
separates a no-image `--preflight-only` mode from `--execute`. Both modes use a narrow `config.rs` reader for
only `CLAUDE_API_DATABASE_URL`, so free smoke admission no longer assembles
or validates unrelated server, provider-roster, or fallback settings. Before each database, schema,
credential, runtime/client and authenticated-discovery step, the private journal records the exact stage
with both dispatch flags false and null request identities. Free success is exactly `preflight_success` and
creates no PNG or evidence. Free-preflight delivery
`737d0234fc7d016c31c5b9c56a27e16aef134d83` is RED and permanently fences its SHA-keyed root. The mode
contains no image POST, and every valid journal stage requires both dispatch flags false and null request
identities. Its corrective `deploy/gpt-image-2-public-preflight-gate.sh <producer-sha> --inspect` controller
can only validate the retained one-file journal and publish the exact bounded stage; it has no `/proc`,
environment, binary, credential, or network path. Inspector delivery
`77cf6791c92840dc1e45c1aba252820506f63fd4` reported exact retained stage
`credential_selecting`, with both dispatch flags false and both request identities null. This establishes
where the old attempt stopped but does not rerun the selector or prove its root cause. Selector hardening
`6629ecd7b3725bcd7306ef7a1dc8675ef9160a43` keys service identity from the active release policy's
`owner_type=service` and `owner_id=crm-parsing` rather than comparing opaque engine `account.id` with the
external service ID. It retains canonical service/meter-only resolution, the OpenAI master switch, and
exactly-one-active-unexpired-key checks. Engine and trusted-host validation passed, but the overall delivery
stopped after inspection because the GitHub status bridge rejected the digit in context
`deploy/gpt-image-2-public-preflight`. Corrective deploy SHA
`b0c67351bb25437316afb61d18cd4462c57ef27b` is watchdog-GREEN, permits lowercase numbered `deploy/*`
contexts, and performed no image request. The next free attempt uses the distinct
`deploy/gpt-image-2-public-preflight-v2-gate.sh`, producer
`6629ecd7b3725bcd7306ef7a1dc8675ef9160a43`, and fresh
`gpt-image-2-public-preflight-v2` root. It inherited only the PostgreSQL DSN from the active OpenAI slot and
ran only `openai-image-public-smoke --preflight-only`, but delivery
`267744a02d664f24ca072326fa06771b54188de4` stopped at `credential_selecting` with both dispatch flags false
and null request identities. No image POST occurred and this second root is permanently fenced. The
corrective selector `63972f2ddfd5906d7c30a87406053eb3782f4223` identifies the unique engine account by
handle `crm-parsing`, while independently requiring its active assignment and linked policy to be
service/meter-only; release owner metadata is not used as the engine account identity. Its trusted-host gate
was GREEN, but the overall delivery was RED because the historical v2 trigger attempted to revisit its
already-fenced failed root; no new image request or selector execution occurred. Delivery
`2f11cd62ed0f78b97cf31d2287fa660907975aad` retired the v2 trigger and is watchdog-GREEN. The successor
`deploy/gpt-image-2-public-preflight-v3-gate.sh` is pinned to producer
`63972f2ddfd5906d7c30a87406053eb3782f4223`, uses the fresh `gpt-image-2-public-preflight-v3` root, inherits
only the PostgreSQL DSN, and can run only `--preflight-only`. Delivery
`825b3596983e7420a038feb3e883b11f0ebabba7` completed authenticated discovery and wrote the sole terminal
`preflight_success` journal with both dispatch flags false and both request identities null, but the
controller deadline observed a nonzero process exit during teardown and the overall delivery was RED. No
image POST occurred. The corrective controller accepts that exact terminal artifact after a teardown
timeout while every incomplete state, extra artifact, dispatch flag, or request identity remains RED; the
SHA-keyed fence prevents network replay. Corrective delivery
`df924a10edff41b0d047805d18abe16a397b4809` validated that retained terminal evidence without network replay
and is exact watchdog-GREEN. The separate `deploy/gpt-image-2-public-paid-smoke-gate.sh` is pinned to
producer `63972f2ddfd5906d7c30a87406053eb3782f4223` and the fresh `gpt-image-2-public-paid-smoke` root. It
inherits only the PostgreSQL DSN, runs exactly `openai-image-public-smoke --execute`, and permanently fences
re-entry before the first paid dispatch. It repeats authenticated discovery, then allows one generation and,
only after authoritative generation settlement, one one-reference edit; neither paid operation is retried.
Delivery `d2216bfa276d9fe195b0d1f0c8f4f137612bed5a` is RED at the retained journal state
`generation_received`: one generation request returned a provider-decoded bounded PNG and an engine UUIDv4
request identity, but no complete authoritative settlement evidence was persisted and edit was not
dispatched. The paid root is permanently fenced and its execute trigger is retired. The successor
`deploy/gpt-image-2-public-paid-inspect-gate.sh` has no environment, credential, runtime, CLI or network path;
it accepts only that exact two-file generation-only withdrawal and publishes sanitized PNG dimensions,
byte length and SHA-256. This retained generation is not a successful generation+edit smoke, does not prove
settlement, and does not authorize publication. Watchdog-GREEN follow-up producer
`ab3b4e557f7b870b93f62a88a53e87e46b49fb4c` adds only the PostgreSQL read-only
`openai-image-settlement-diagnostic`: it receives the fenced UUIDv4 on stdin and independently reports
identifier-free reservation/snapshot/outbox/usage/principal presence and bounded states/numeric usage. The
separate controller `deploy/gpt-image-2-settlement-diagnostic-gate.sh` is pinned to that immutable producer
and the original paid fence. It revalidates the exact generation-only journal, requires that diagnostic
release to be current, inherits only `CLAUDE_API_DATABASE_URL` from an active OpenAI slot, and pipes the UUID
over stdin into the diagnostic binary. It validates the closed JSON schema and publishes only bounded state,
attempt/error booleans and integer usage/cost. It has no image transport, credential lookup, image dispatch,
retry, or database write path. Controller delivery
`d66e25babba5e55ef96ebec51971962656a4badf` is watchdog-GREEN and reported
`terminal_evidence_present`: reservation `settled`, outbox `done` after one attempt with no error, usage
present, `real_nano=7_045_000`, and `charge_nano=0`. The generation therefore settled correctly after the
paid runner had stopped waiting; edit still was not dispatched, so this remains insufficient for
publication. The successor runner does not extend or replay that fenced call. Instead, each fresh request's
settlement observer has an explicit 150-second wall-clock deadline with 500 ms polling rather than a fixed
number of attempts. The PostgreSQL session independently limits each statement to 15 seconds and lock wait to
5 seconds. Observer timeout is terminal and does not repeat image POST. Corrective producer
`853fdc6c8d5be486c371b23df6772eeaf7a48029` is exact watchdog-GREEN. Its separate
`deploy/gpt-image-2-public-paid-smoke-v2-gate.sh` uses the fresh
`gpt-image-2-public-paid-smoke-v2` parent, is pinned to that immutable binary, inherits only the active OpenAI
slot's PostgreSQL DSN, and may invoke exactly one `--execute`. The old paid root and controller remain retired.
The v2 gate returns GREEN only for two strict, byte-different PNGs plus exact generation/edit usage and
release-v2 settlement evidence; every partial or ambiguous state permanently fences the new root and fails the
overall deployment. Delivery `2efcfbf69b672e531b62b8602a74d7fb76ee1fae` withdrew at the exact
`generation_received:g=true:e=false` journal state, so v2 is now retired and cannot dispatch again. The
separate `deploy/gpt-image-2-settlement-v2-diagnostic-gate.sh` reuses the already GREEN `853fdc6c...`
diagnostic binary against only the v2 fence. It receives the generation UUID over stdin, inherits only the
PostgreSQL DSN, and reports bounded settlement/outbox/usage/cost state without image HTTP, pool credential
selection, mutation, or retry. Diagnostic delivery `f1fb47c3e6e75c219f7b9f6f229db693e54197f5` is exact
watchdog-GREEN and again reported `terminal_evidence_present`, `settled`, `done/1`, `real_nano=7_045_000`,
and `charge_nano=0`. The repeated stop at `generation_received` was a runner panic, not delayed settlement:
the async orchestration called synchronous `PgStore` while already inside the command's Tokio runtime, while
the `postgres` client implements each query and `Drop` with its own runtime `block_on`. The corrected runner
enters Tokio only around each HTTP future and performs settlement reads and `PgStore` teardown outside Tokio;
it does not replay either fenced request. Corrective producer
`8b68d73a2a6ba6ffae2f24692b283059f15b7c63` is exact watchdog-GREEN. The separately delivered
`deploy/gpt-image-2-public-paid-smoke-v3-gate.sh` is pinned to that immutable binary and the fresh
`gpt-image-2-public-paid-smoke-v3` root. It inherits only the production PostgreSQL DSN, invokes exactly one
`--execute`, and permanently fences any partial outcome. The v2 trigger remains retired.

The one-shot contract was:

1. authenticates `GET https://openai.api.apitoken.sale/v1/models` and requires both image aliases to
   remain absent before dispatch;
2. selects only the existing active `crm-parsing` service assignment and requires exactly one active key;
   no credential is created, logged, serialized, placed in argv or written to disk;
3. sends one `opaque/low/auto/png/b64_json` public generation and, only after its exact settlement, one
   public multipart edit using that generated PNG;
4. never retries either paid operation after dispatch; an existing output directory/journal permanently
   fences replay;
5. requires one bounded PNG, exact modality sums, positive output usage, positive image-input usage for
   edit, byte-different edit output, the canonical lowercase engine UUIDv4 reservation identity in
   `x-request-id`, typed `openai_image_v1` snapshot controls, terminal `settled`/`done`, exact five-leg
   official `real_nano > 0`, and `charge_nano=0` under release-v2 `meter_only`;
6. proves account/key balance, spent and reserved aggregates are unchanged and writes only mode-`0600`
   PNG/evidence files under a new mode-`0700` directory whose existing absolute parent is an actual
   mode-private directory (no group/world permission).

The command uses the same production public origin and the same sealed Codex OAuth pool behind it. There
is no reseller image origin, image-specific API key, fallback, or environment variable.

## Outcome

The v3 gate delivery `d172c6fd0116ba73b051fc5aa02193a4885de5da` ran the exact one-shot contract above
and is overall watchdog-GREEN: two strict byte-different PNGs, exact generation/edit usage, terminal
`settled`/`done`, and `charge_nano=0` under release-v2 `meter_only`. Publication followed in the
separate commit `3917a31b333899aed87396acd6e8e83e403cd3e6`: the generation-6 pricing release activated
the immutable `gpt-image-2-2026-04-21` snapshot in the main and OpenKeys catalogs (release head 41),
the unified router proxies both image routes as a native OpenAI lane and its preset lists the
snapshot (`/v1/models` deliberately does not publish the image model), and the site catalog and
public docs list the model.

The official model contract is non-streaming. The public Image API guide also describes masks, output
formats and Responses multi-turn editing, but the actual native subscription wire has not proved masks,
transparent backgrounds, exact dimensions, medium/high quality, multiple references/outputs,
partial-image streaming, JPEG/WebP/compression, or Responses image state. The producer rejects those
controls rather than simulating or advertising them.

# OpenKeys — prepaid keys without registration

`openkeys.apitoken.sale` is a storefront for keys sold ready-made (FunPay and similar
marketplaces). The buyer needs no registration, email or card: they receive a key and a
personal link to the spend page.

The key difference from competitors: **the face value is set in the dollars of the chosen
API's price list**, not in internal "tokens". The admin issues separate Claude or GPT
batches; the type determines the Base URL, the instructions and the USAGE labels without
changing the shared engine contract or the `sk-pool` format.

## Composition

| Component | What it is |
|---|---|
| `apps/openkeys` | Next.js on port 3410: Claude/GPT docs, `/profile/<token>`, USAGE and the `/admin` console |
| `packages/openkeys-db` | Its own PostgreSQL schema (`openkeys_batches`, `openkeys_keys`) and migration runner |
| `deploy/openkeys-deploy.sh` | Rollout: release promotion, migrations, atomic symlink, readiness gate, rollback |
| `systemd/apitoken-openkeys.service` | The service unit |

Context boundaries: OpenKeys does **not** touch commerce and sales. It talks to the engine
only through the Control API from `docs/engine/CONTROL_API.md` — like the rest of the
commercial layer.

## Data model

One sold key = one engine account. That way the balance belongs to exactly this key, and
the spend page can show the remainder without knowing anything about the user.

`openkeys_batches.api_type` distinguishes `anthropic` and `openai` only as a
historical/storefront label. Historical rows with `NULL` are interpreted as `anthropic`;
the field does not limit models, does not select a pricing rule and does not change the
universal access of a single key. By explicit owner decision a key accesses every provider
and model the runtime can price at 1:1, including Gemini and every future provider; no
OpenKeys catalog cutover is required for admission.

The full `sk-pool-…` secret is stored in the warehouse only as AES-256-GCM ciphertext and
is wiped after issuance or withdrawal. `engine_account_id`, `engine_key_id`, the mask and
`view_token` — a random 128-bit identifier of the public spend page — remain for history.

The spend page formats the model name by canonical family and engine `provider`: only
Anthropic gets the `Claude` prefix, GPT stays GPT, Gemini — Gemini, and unknown future
families are shown neutrally. The fallback has no right to attribute `Claude` to another
provider.

The spend page is live over Server-Sent Events instead of periodic reloads:
`GET /api/usage/stream?token=<viewToken>` re-reads the cached usage snapshot every 5
seconds and emits a frame only when the rendered data actually changed, with heartbeat
comments in between so intermediaries keep the connection open. The per-key view token is
the channel's only credential, so the stream inherits the access contour of the page
itself. If the stream cannot be established (an ancient browser or a buffering proxy),
the client falls back to a full page reload every 15 seconds.

Key login at `/profile` keeps only a signed `__Host-` session cookie. A successful login
atomically replaces the previous profile session, logout removes that same host-only cookie,
and both transitions perform a document-level navigation. This makes a second login render
from the new cookie instead of depending on a browser's cached Next.js router payload.

A new issuance always has `pricing_contract=official_1_to_1`, `mult_bp=10000`: a key with
a $50 face value receives exactly $50 of engine balance, and $1 of the model's full
official cost charges $1. That literal is a contract between the code and the CHECK in
`packages/openkeys-db/migrations/0007_openkeys_pricing_contract_expand.sql`, and the two are
allowed exactly one spelling. They drifted on 2026-08-09 (`official_one_to_one` in
`OFFICIAL_ONE_TO_ONE_CONTRACT` against `official_1_to_1` in the constraint), which would have
failed the next batch insert outright; production was spared only because no batch was issued in
between. The constant is now typed as that literal and the migration test builds its inserts from
it, so a future divergence fails to compile or fails the DB-backed test instead of the next sale.
Until the global Stage 9 cutover, historical
`pricing_contract=legacy` remains the migration source. The target release switches all
existing and new OpenKeys to 1:1; past ledger rows are not recalculated.

## What the buyer sees

All buyer connections point to the unified router `https://router.apitoken.sale` (contract
— `docs/engine/UNIFIED_ROUTER.md`): the `sk-pool` key authenticates on it the same way as
on the former per-provider hosts. The handover text (`universalKeyHandoverText`), the
provider cards on the spend page and the Claude Code/Codex connection commands use:

- Claude / Anthropic API — `https://router.apitoken.sale` (`POST /v1/messages`,
  `x-api-key`, `ANTHROPIC_BASE_URL=https://router.apitoken.sale`);
- GPT / OpenAI-compatible API — `https://router.apitoken.sale/v1` (`/responses`,
  `/chat/completions`, `Authorization: Bearer`, Codex `base_url`);
- Gemini / Google Gemini API — `https://router.apitoken.sale` (`POST
  /v1beta/models/{model}:generateContent`, `x-goog-api-key`,
  `GOOGLE_GEMINI_BASE_URL=https://router.apitoken.sale`).

The Gemini block deliberately stays in the handover and the cards, and Gemini access is live:
the pricing authority admits every runtime-capable provider at 1:1, and the issuance
supported-model list (`MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES`) shows the full
generation-5 main set — Anthropic, OpenAI and Gemini. The former per-provider
hosts (`api.apitoken.sale`,
`openai.api.apitoken.sale`, `gemini.api.apitoken.sale`) keep working, but the buyer is
given only the router address as the primary instruction. Balance lookup by key
(`/balance`) still goes to `ENGINE_PUBLIC_BASE_URL`/`ENGINE_OPENAI_PUBLIC_BASE_URL` — this
is a server-side call that is not part of the router contract.

## Environment variables (`/etc/apitoken/openkeys.env`, root-only, 0600)

| Variable | Purpose |
|---|---|
| `OPENKEYS_DATABASE_URL` | DSN to its own openkeys database |
| `ENGINE_CONTROL_KEY` | The engine's Control API. Server-side only, never sent to the browser |
| `ENGINE_BASE_URL` | Default `http://127.0.0.1:8790` — the stable loopback origin, not a slot |
| `ENGINE_PUBLIC_BASE_URL` | Default `https://api.apitoken.sale`, used for `/balance` when looking up by key |
| `ENGINE_OPENAI_PUBLIC_BASE_URL` | Default `https://openai.api.apitoken.sale`, fallback GPT-key check via `/balance` |
| `OPENKEYS_ADMIN_USER` | Login of the primary admin console account |
| `OPENKEYS_ADMIN_PASSWORD` | Its password |
| `OPENKEYS_ADMIN_ACCOUNTS` | Additional accounts as `user:password`, separated by comma or newline |
| `OPENKEYS_SESSION_SECRET` | Session cookie signing secret, at least 32 characters |
| `OPENKEYS_SECRET_KEY` | 32-byte AES key in hex (64 characters); backed up separately from the database dump |
| `OPENKEYS_SECRET_KEYS` | Keyring for rotation: `kid:64-hex,kid2:64-hex`; old keys are kept until re-encryption |
| `OPENKEYS_SECRET_ACTIVE_KID` | KID of the keyring key used to encrypt new warehouse secrets |
| `OPENKEYS_PUBLIC_BASE_URL` | Base address for links of the form `/u/<token>` |
| `OPENKEYS_SESSION_TTL_SECONDS` | Admin session lifetime, default 12 hours |

The admin console accounts are valid **only on this domain**: the cookie is signed with a
separate secret and set on `openkeys.apitoken.sale`; there is no connection to
`admin.partners.*` or the panel. The cookie carries the name of whoever logged in, and
that name lands in the batch's `created_by` — it is visible who issued the keys. Removing
an account from env immediately invalidates its sessions: the name is checked against the
list on every request. An expired session does not silently redraw the login form: it
shows the explicit message «Сессия истекла — войдите снова» ("Session expired — log in
again"). An env configuration failure (for example, a lost `OPENKEYS_SESSION_SECRET`)
answers with the same 401 but is always written to the server log (`openkeys admin session
check failed`), so that a misconfiguration is not masked as an expired session.

The economics of a new issuance is not configurable via env or request: the face value is
credited to the engine exactly 1:1 and stored as `official_1_to_1`. Old `legacy` keys may
be read with the historical multiplier only until Stage 9. After the global cutover the
live reserve of any OpenKeys uses the canonical 1:1; the legacy contract cannot be chosen
for a new batch and cannot be carried into the target release.

Issuance no longer touches the release-v2 assignment extension: with the release-v2 retirement
(phase 2.1) a new OpenKeys account is born directly on the strict policy path
(`provisionOfficialOpenKeysCredential` in `apps/openkeys/src/lib/openkeys-pricing.ts`). After the
read-only authority check (`resolveOpenKeysPricingAuthority`) the writer activates the canonical
official 1:1 policy on a `strict/strict/verified` binding (`officialOpenKeysStrictBinding`) — on a
zero-key, zero-balance account the unbound→strict activation is vacuously valid — then credits
exactly the face value (allocated into strict funding buckets), issues the `sk-pool` secret WITH
its exact activation ACK, and finally writes the one-way engine opt-out marker through
`EngineClient.optOutPricingReleaseV2`. Only after the marker lands is the uncovered account
servable at all, so the usable secret is returned last. Every step is idempotent under the
issuance job's retry; any rejected ACK, credit failure or opt-out guard rejection aborts the
issuance job, and the existing issuance compensation disables the half-provisioned engine
account. Pre-existing OpenKeys accounts keep working on the release path until the bounded
backfill sweep retires them (release-v2 retirement, phase 2.2): the internal admin endpoint
`POST /api/internal/admin/strict-backfill` (`apps/openkeys/src/lib/strict-backfill.ts`)
advances each warehouse-owned account — `openkeys` funding-class proof, shared strict-cutover
preflight, exact-CAS activation of the same official 1:1 strict policy over the observed
active policy, key re-stamp on the new head, one-way opt-out marker — idempotently,
batch-limited (`limit`, `account_ids` canary list), one account's failure never blocking the
batch, and service/meter-only accounts excluded by construction (they are never in
`openkeys_keys`) and by the engine funding-class check. Ops sequence:
`docs/ops/PRICING_RELEASE_BACKFILL.md`.

The Stage 7 existing-inventory shadow-rollout lane (migration
`0035_pricing_shadow_rollout_jobs.sql`) was removed with the dismantled release cycle: the
`POST /v1/admin/pricing-shadow-rollout-v2/stage` producer (and its paired GET) in `apps/api`
and the bounded `apps/worker` rollout consumer no longer exist. The completed gpt-image-2
rollouts remain durable historical evidence; new OpenKeys issuance never depended on the lane
(release-native from birth through the direct strict provisioning path above). The engine
`locked-openkeys-transition` endpoint stays as expand-only contract surface with no live
consumer. The historical protocol record is `docs/commerce/MULTI_DISCOUNT_STAGE7.md`.

### Authoritative pricing inventory v2 — removed

The loopback/internal `GET /api/internal/pricing/v2/inventory` producer (bounded cursor + full
`sha256:v2` manifest under `X-OpenKeys-Control-Key`) existed only for the Stage 5 v2
materializer and the release-advance preflight. With the release advance retired (head 55 is
the final pricing release — `docs/ops/MODEL_RELEASE_CYCLE.md`) its only consumers are gone, so
the route and `apps/openkeys/src/lib/pricing-inventory.ts` are deleted; the
`openKeysPricingInventory*V2` wire schemas in `packages/contracts` remain as expand-only
contract surface until a separate removal pass.

## Administrative interfaces

Its own `/admin` is built around batches, not denominations. The batch list has
server-side search by label/ID and pagination; the contents of the selected batch expand
directly under its row, and a second click collapses them. Only for the selected batch are
warehouse secrets loaded and decrypted (no more than 100); keys ready for sale and the
issuance history are shown separately. A new batch in the UI requires a label so the
seller does not get lost among a large number of issuances; historical batches without a
label remain visible.

If issuance is blocked (`GET /api/admin/batches` could not confirm the pricing authority),
the warehouse stays available and the response is augmented with the
`issuanceAuthority.reason` diagnostics — a safe machine code and a human-readable
description without internals: a pricing-error code (`pricing_authority_missing`,
catalog/switches drift) means the authority must be fixed in commerce,
`engine_unavailable` — that the engine is unreachable or `ENGINE_CONTROL_KEY` is invalid,
`authority_check_failed` — anything else (check the server log). The reason is shown in
the issuance form next to the blocked button and is simultaneously written to the server
log (`openkeys issuance authority check failed`).

The unified `admin.apitoken.sale` shows all OpenKeys keys in a separate section: the mask,
the mandatory label/batch column, the seller, live spend/remainder and reversible
disabling. Filters work by batch, status and usage. The browser only talks to same-origin
`/openkeys-admin/*`; after managed-admin auth Caddy proxies the request to
`/api/internal/admin/*`, adding the verified actor and the server-side credential. The
public `openkeys.apitoken.sale/api/internal/*` is closed with a `404` response, and the
internal API never returns the full key or the AES-GCM ciphertext.

### Bulk control by issuing admin

The same section lists the issuing admins above the catalog. An admin is not a table row:
it is `openkeys_batches.created_by`, the name from that person's own console session, so
the summary and every bulk action are grouped by that field. `GET
/api/internal/admin/sellers` returns one row per admin — batches, live keys,
active/paused, delivered/stock, already revoked, the total face value and the last
issuance — computed by a single SQL without live balances.

`POST /api/internal/admin/sellers` takes `{createdBy, action}` and applies one action to
**all** keys of that admin, ignoring the catalog filters:

- `pause` — every active key goes to `disabled` in the engine and here. Reversible.
- `resume` — the mirror action; it only lifts paused keys and never resurrects revoked ones.
- `revoke` — irreversible: the engine key is disabled, the row is marked removed
  (`removed_by` is the actor verified by Caddy, never a value from the body), and the
  warehouse ciphertext is wiped. Keys already handed to buyers are revoked too — the
  "the issuing admin is compromised" case is not covered otherwise. Revoked rows leave the
  catalog but stay in history and in the pricing inventory producer.

An unknown `createdBy` answers `404 unknown_seller` instead of a silent "0 keys", so a typo
cannot look like a successful revocation. One call touches at most 500 keys with
concurrency four; the response `{matched, changed, failed, remaining}` reports partial
success, and the panel shows those counters instead of "done" — a key the engine refused
stays in its previous state and is picked up by the next click. In the UI `revoke` is the
only red button and additionally requires typing the admin's name.

The additive read-only producer `GET /api/internal/admin/paying-keys` projects every
non-removed key (`removed_at IS NULL`), including both warehouse stock and keys already
delivered to buyers. Each row carries explicit `lifecycle=stock|delivered` and nullable
`deliveredAt`, so an unissued key cannot disappear or look delivered. It accepts
`days=1|7|30` (default `30`), `limit=1..100` (default `50`), `offset=0..100000`
(default `0`), trimmed `q` up to 80 characters, `status=all|active|disabled`,
`sort=spent|nominal|created|delivered|status` (default `spent`) and
`dir=asc|desc` (default `desc`). `spent` means authoritative lifetime engine spend,
not the selected usage window; the response therefore exposes it separately as exact nullable
`lifetimeSpentNano`. This sort loads filtered accounts through bounded batches of 500,
sorts exact `BigInt` amounts globally before pagination and always places unavailable accounts
last. The other sorts execute directly in PostgreSQL before pagination. Every selected row
carries safe batch/key metadata and the complete engine usage
for the selected window, preserving exact nanoUSD strings, free-form provider/model names
and all token counters. Only the selected page makes usage calls with concurrency four. A failed
account usage call is row-local `{status:"unavailable",window}`; a real zero remains
`status:"available"` with exact zero usage. Responses, including auth/query errors, are `no-store`; invalid auth is hidden as
`404`, invalid query is `400`. The contract excludes the full secret, view token, engine
key id, digest and warehouse ciphertext/nonce. After GREEN exact producer SHA
`558d4b34896792cfaed5760852f9001feb0d0443`, `apps/admin` consumes the endpoint in the
OpenKeys cohort of `/paying-users`; only the visible cohort mounts its poller.

## First launch on the server

```bash
# 1. Create the database and write the env (root)
sudo -u postgres createdb openkeys
install -o root -g root -m 0600 /dev/null /etc/apitoken/openkeys.env
# fill in the variables from the table above

# 2. Roll out the updated units, sudoers and controllers
sudo bash deploy/install-watchdog.sh

# 3. From then on rollout is automatic: the watchdog will see changes in apps/openkeys
#    or packages/openkeys-db and will call openkeys-deploy.sh
```

The password, session secret and `OPENKEYS_SECRET_KEY` are never committed to the
repository — only into `/etc/apitoken/openkeys.env`. Without a separate protected backup
of `OPENKEYS_SECRET_KEY`, a PostgreSQL dump cannot restore warehouse secrets that have not
yet been issued.

## What the watchdog checks

`wd_path_is_openkeys` assigns to the context `apps/openkeys/*`,
`packages/openkeys-db/*`, `packages/engine-client/*`, `packages/contracts/*` and the root
manifests. For every candidate the openkeys migrations are run against a separate
disposable PostgreSQL (`watchdog-test-db openkeys-dsn`), and only then does the rollout
proceed with a readiness gate on `http://127.0.0.1:3410/api/ready`. Readiness checks the
configuration, PostgreSQL and the engine Control API without exposing the reason for
refusal. The `openkeys` database is included in the regular, mandatory pre-deploy backup
together with the other PostgreSQL contexts.

The GitHub context is called `deploy/openkeys`; its own baseline lives in
`$STATE_ROOT/openkeys.sha`, so changes only in OpenKeys touch neither the engine nor the
backend.

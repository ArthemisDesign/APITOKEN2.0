# Caddy production routing

`deploy/Caddyfile` is the complete intended production configuration for Caddy 2.11. It includes the public engine API, the blue-green commerce API, loopback-only stable origins, mail/autodiscovery, support routing, and the managed-session-protected unified admin, partner admin, CRM, Content Studio and monitoring surfaces.

## Managed browser sessions

The five managed admin hosts expose only `/__admin-auth/*` as the same-origin browser projection of
the loopback commerce auth controller. Every other protected request passes through `forward_auth`
with the exact original method, URI and browser navigation headers plus
`X-Admin-Auth-Mode: session-v1`. A document without a valid cookie receives a challenge-free `303`
to the local login form; an API request receives a challenge-free `401` and `X-Admin-Login`.
Successful login sets the host-only `HttpOnly; Secure; SameSite=Lax` cookie for 180 days. Login
and logout POSTs require the exact managed HTTPS origin. Browsers that omit `Origin` on a
same-origin HTML form receive `Referrer-Policy: same-origin` on the login surface; only a parsed
`Referer` with that exact origin is accepted as fallback. A present foreign or `null` `Origin`
never falls through to `Referer`, and missing/foreign/malformed fallback evidence stays forbidden.
The auth producer still resolves the account and exact hostname grant on every request, so disabling
the account or changing its password/domains revokes the cookie immediately.

Caddy's `forward_auth copy_headers` writes successful auth-response headers onto the original
request. For Basic-to-cookie migration, `Set-Cookie` is therefore copied through the private
`X-Admin-Session-Set-Cookie` bridge, added to the eventual browser response, and cleared before the
application upstream runs; successful Basic credentials are removed at the same boundary. Client
copies of identity, auth-mode and bridge headers are removed first. Non-2xx auth responses retain
their safe `Location`, `X-Admin-Login` and cookie-clear headers,
while `WWW-Authenticate` is stripped so iOS never reopens its native Basic prompt. CRM ingest and
public `/r/*` tracking remain outside human auth. `install-caddy.sh` imports the old Caddy bcrypt
rows before the one-time cutover and rolls the live file back if the public redirect/login smoke
does not pass. Final watchdog verification exercises a credential-free invalid login POST with no
`Origin` and an exact same-origin `Referer` on all five hosts, plus a foreign-Referer rejection, so
browser compatibility cannot regress behind a GET-only green check. Caddy's structured-log filter
redacts Authorization, Cookie and the temporary bridge
header in addition to the injected service credentials.

## Host-only secrets

The repository intentionally contains only named placeholders for engine/commerce/sales service keys and the dedicated proxy-admin header. Shared service keys live only in `/etc/caddy/Caddyfile` and are carried forward by `deploy/render-caddy.awk`. The proxy-admin secret instead has one canonical raw source: `/etc/apitoken/proxy-admin.key`, a stable `root:root 0600` regular file with no symlink, containing exactly 64 lowercase hexadecimal bytes and optionally one trailing LF. Its `/etc/apitoken` parent is root-owned and not writable by `deploy`; the key must not live below deploy-writable `/srv/claude-api/data`. `deploy/install-watchdog.sh` provisions it atomically before installing the unit or Caddy definitions. On upgrade it removes one exact legacy `AUTH_BOT_PROXY_ADMIN_KEY=<64-lowercase-hex>` assignment from `authbot.env`; malformed, duplicate, or divergent legacy/canonical state aborts the transaction. It also rejects either `AUTH_BOT_PROXY_ADMIN_KEY` or `AUTH_BOT_PROXY_ADMIN_KEY_FILE` in `server.env`, so a shared environment file cannot supply or redirect this credential.

`deploy/install-caddy.sh` passes only `/etc/apitoken/proxy-admin.key` to AWK, never the secret itself. The renderer emits the canonical key; it matches the live `X-Proxy-Admin-Key` header name case-insensitively, and any existing occurrence must have the exact canonical value or rendering fails without a partial candidate. Missing, malformed, symlinked, or incorrectly owned canonical state aborts before validation/reload. Systemd uses `LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key` to make a private per-service copy. After all `EnvironmentFile` directives load, `ExecStart=/usr/bin/env ... AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key ...` pins the credential path for `claude-authbot.service`; it is deliberately not an `Environment=` directive, so an env file cannot override it. The Rust parser reads only that bounded dedicated file format. Sibling units are not passed the value or credential path. `ProtectProc=invisible` and `ProcSubset=pid` remain in force. After operator-subcommand early-return handling and before loading daemon secrets, Linux authbot calls `prctl(PR_SET_DUMPABLE, 0)`, blocking same-UID `ptrace`, `process_vm_readv`, and sensitive proc-memory access. Code already executing inside authbot itself is within the same trust boundary, and no defense can protect against such in-process code. The root-run Caddy installer is the only other intended consumer. Never print either source, put values in shell history, reload repository placeholders into production, or copy the populated host file back into Git.

Preserve the existing file owner, group, and mode when replacing the host config. The packaged service reads it as the `caddy` user; changing a `root:caddy 0640` file to `root:root 0640` makes `systemctl reload caddy` fail even though a root-run `caddy validate` succeeds. Use `cp -a` for a backup and `chown --reference`/`chmod --reference` after installing a generated candidate.

## Validate and reload

After updating `/etc/caddy/Caddyfile` and inserting the host-only secrets, validate the exact file that Caddy will load:

```bash
caddy validate --config /etc/caddy/Caddyfile
```

Only after validation succeeds, apply it without restarting Caddy:

```bash
caddy reload --config /etc/caddy/Caddyfile
```

The systemd alternative is:

```bash
systemctl reload caddy
systemctl is-active caddy
```

Do not use a stop/start cycle for a configuration update. A reload preserves Caddy availability and avoids needlessly terminating proxy connections.

## Commerce API blue-green behavior

The only commerce slot selector is the loopback balancer `127.0.0.1:8791`, which has two slots:

- blue/green slot A: `127.0.0.1:3000`
- blue/green slot B: `127.0.0.1:3001`

Caddy probes `GET /v1/ready` on each slot every 2 seconds with a 2-second timeout and accepts only a `2xx` response. A process that is draining (`SIGUSR1`) or whose local commerce PostgreSQL check fails must return `503` from `/v1/ready`; Caddy then removes that slot from new-request selection. Engine Control API reachability is not a commerce slot dependency: HTTP 200 with `status: "ok"` and `database: "up"` keeps the slot in the pool even when the JSON reports `engine: "down"`. When the endpoint returns `2xx` again after drain or a local DB outage, Caddy automatically makes the slot eligible. Every public/admin vhost and application targets `8791`, never `3000` or `3001`.

For the first reload that adds `127.0.0.1:3001`, leave `apitoken-api@3001.service` stopped. Validate and reload Caddy while port 3000 continues serving. The new upstream initially dial-fails and Caddy's first active check marks it down; the configured dial-retry window covers the short interval before that health state is recorded. Start 3001 only through `api-bluegreen.sh` after the reload.

Both API units name `/opt/apitoken/releases/current/apps/api`, but a running process retains the resolved immutable release it started from in its working directory, loaded modules, and open files. Moving `releases/current` is therefore safe while the old process remains alive. Restarting that old unit after the symlink moves would instead launch the new release and destroy the rollback anchor, so API unit lifecycle belongs to `api-bluegreen.sh` during a normal deploy.

Slot admission and removal use only the active readiness gate. Application-level `503` responses are intentionally not passive health failures: Caddy shares passive failure state globally by upstream address, so one legitimate `503` could otherwise remove the sole live slot from unrelated routes. A 2-second load-balancer retry window still covers selection and connection races while active checks converge.

### Cutover sequence

1. Run `deploy.sh` to build/finalize the immutable release, execute its locked prebuilt migration, and atomically move `/opt/apitoken/releases/current`. This step does not touch either API process; the old slot continues executing its started-with release.
2. Identify the inactive target. Unless it already proves that its exact `MainPID` serves the current immutable release, stop its unit first even if it appears inactive, require it stopped, and start it fresh through `releases/current`. This clears stale unit/cgroup/port state.
3. Require the exact target unit and direct `/v1/ready` HTTP 200, then wait 6 seconds (`health_interval` 2s + `health_timeout` 2s + margin) and re-verify it. The old slot remains running throughout this admission phase.
4. Commit the verified target as the new availability anchor. If admission failed before this point, stop only the failed target and leave/confirm the old process ready; never restart old through the moved symlink.
5. Send `SIGUSR1` to the old unit. The application immediately changes `/v1/ready` to `503` but keeps its listener open and continues in-flight work.
6. Wait another 6 seconds for Caddy's active checker to depool the old slot. Before closing any listener, require the old endpoint to return exactly `503` and re-require the new slot to return `200` from the current release.
7. Only after both checks pass, stop the old unit. Systemd's `SIGTERM` and the application's bounded drain complete in-flight shutdown. A connection-refusal race is still cushioned by the stable balancer's configured retry window.
8. Leave the stopped port as the inactive slot for the next deployment.

Rollback is state-aware rather than a blind reverse restart. Before pre-drain, the old process is the rollback anchor and a failed new slot is stopped alone. Once the new slot has been admitted and the old slot has drained/stopped, recovery keeps the verified new process. Restarting the old unit is a last-resort availability action only when it has already died/drained and no verified target remains; because `releases/current` has moved, that restart launches the new release and must be warned and readiness-verified explicitly.

A `503` returned by a normal proxied request is returned only to that caller and does not depool the process. The readiness-first drain and wait in steps 4-6 are the sole slot-removal authority; connection retries are a race cushion, not a replacement for orderly draining.

## Engine blue-green and SSE

`api.apitoken.sale` and the unified admin proxy use stable Anthropic origin `127.0.0.1:8790`, whose
health-gated upstreams are `127.0.0.1:8787` and `127.0.0.1:8788`. The OpenAI hostname uses a separate
stable origin, `127.0.0.1:8792`, over blue-green slots `127.0.0.1:8793` and
`127.0.0.1:8797`. During the first split only, 8792 also recognized the old combined slots. That
bridge has been removed after the dedicated runtime was verified in production: 8792 now targets
only its two OpenAI slots and no API-plane routing header exists.
The watchdog resolves the OpenAI hostname to loopback and probes it over HTTPS, covering the public
vhost boundary end to end.

The unified admin's `/openkeys-admin/*` path is routed to the single-instance OpenKeys service on
`127.0.0.1:3410` only after managed-admin authentication. Caddy injects the reused server-side
engine control credential under the dedicated `X-OpenKeys-Control-Key` header; the browser never
receives it. The public OpenKeys vhost returns `404` for `/api/internal/*`, so the internal catalog
cannot be reached through the customer-facing hostname even with a forged actor header.

The same admin vhost preserves `/proxy-admin/*` and routes it to authbot's loopback-only listener on
`127.0.0.1:8806`. Caddy overwrites any client-supplied `X-Proxy-Admin-Key` with the dedicated
canonical value and overwrites `x-api-key` with the shared engine key only so the previous authbot
binary remains usable during mixed-version rollout or rollback. The new binary
ignores `x-api-key`, authenticates only the dedicated header, and uses the shared key only for its
outgoing loopback runtime status calls; shared-key holders therefore cannot read `account_email`.
Caddy also forwards the `X-Admin-Actor` copied by `managed_admin_auth`; a client-supplied actor is
removed before authentication. `GET inventory` is a sanitized managed-admin read, while `POST renew`
is the only paid mutation and requires that verified actor plus a UUID idempotency key. No public
provider hostname routes this listener. Proxy credentials and IPRoyal keys never enter Caddy or the
browser.

Gemini is an independent active/passive pair: `gemini.api.apitoken.sale` targets stable loopback
origin `127.0.0.1:8794`, which health-gates slots `127.0.0.1:8795` and `127.0.0.1:8799`. Its
`lb_policy first` prevents two healthy generations from round-robining the same OAuth roster during
cutover: the preferred ready slot takes new requests, while the readiness-drained predecessor keeps
only established streams. It never participates in the 8792
bridge and never accepts the request-level API-plane marker. Its public matcher allows `/v1beta/*`,
`/upload/v1beta/*`, `/health`, and `/balance`; the upload prefix is not exposed on the Anthropic or
OpenAI provider hosts. An earlier exact `/oauth/callback` handler routes the no-store official
CLI code-entry form and its POST only to Auth Bot on `127.0.0.1:8796`, so OAuth codes never enter
the engine or Caddy access log. The vhost redacts `X-Goog-Api-Key` in access logs. The watchdog probes the
public hostname for a native unauthenticated Gemini envelope before committing the provider cohort.

KIMI is a backend-only active/passive pair with the same shape and no public vhost at all: stable
loopback origin `127.0.0.1:8803` health-gates slots `127.0.0.1:8804` and `127.0.0.1:8805` with
`lb_policy first`, signs its no-upstream 503 with the same execution-state header, and serves the
default-off plane's disabled envelope until a reviewed unit change enables the provider. The only
consumer routes are the Prometheus `provider: kimi` scrape target and the admin `/kimi-subs` data
route with the proxy-injected control key.

Caddy probes `/ready` on all fixed-provider slots. `engine-bluegreen.sh` admits the new Anthropic
slot, sends `SIGUSR1` to make
the old slot return 503 readiness, waits for depooling, then sends SIGTERM so established streams
drain under the systemd deadline. Only after the old cgroup is fully stopped does the first split
start OpenAI, preventing overlap with a legacy combined process. OpenAI slots run the native Codex
provider: both generations read the same sealed credential roster, so authenticated-home parity is
inherent rather than a gate. This parity is a readiness condition, not a soak timer: the candidate's
own preflight proves its profiles open, refresh and answer before any traffic anchor moves; one
equal working home is valid. Only then is the old HTTP slot pre-drained and stopped.

Established OpenAI and Gemini streams remain on the old HTTP slot during pre-drain and may finish
through the shared server deadline. Each replacement generation reopens the same sealed read-only
provider roster. New Gemini requests remain active/passive because readiness fencing and Caddy's
ordered policy prevent normal dual-generation dispatch.

The public OpenAI vhost negotiates `zstd` or `gzip` only for complete `application/json` responses
of at least 512 bytes. Compression happens at the public TLS boundary, after the loopback OpenAI
origin, so it reduces customer traffic without adding encoded bytes to the internal Caddy hop.
`text/event-stream` is deliberately excluded: every Responses and Chat Completions SSE frame keeps
identity encoding and Caddy's immediate event flush, preserving time-to-first-token. Clients that
do not advertise a supported `Accept-Encoding` continue to receive the byte-identical JSON body.

The commerce API and worker use `http://127.0.0.1:8790`, a loopback-only Caddy listener over the
Anthropic slots. They must never address a deployment slot, the OpenAI origin, or the Gemini origin.
The provider controller requires 8790 throughout the handoff and verifies 8792, 8794 and 8803
separately.

Each stable provider origin (8790/8792/8794/8803) handles Caddy's own `503 no healthy upstream` failure
by returning the internal header `X-Apitoken-Execution-State: not_started`. A normal runtime-produced
HTTP 503 is a completed reverse-proxy response and does not enter `handle_errors`, so it never gains
this proof. The unified router may continue an explicit fallback chain only on that exact signal;
the three public provider vhosts remove the header on their outer proxy hop so direct customers
cannot observe or depend on internal execution state.

The 2-second `lb_try_duration` and 100 ms `lb_try_interval` hold and retry a newly arriving request when the loopback dial fails during a brief engine restart/bind gap. Dial failures are retryable for every HTTP method because the connection was never established and the request was not transmitted. The configuration does not broaden Caddy's default rule for failures after a connection was established, so POST bodies are not unsafely replayed after a partial round trip.

The retry window applies only while Caddy is selecting and connecting to an upstream. It does not cap, restart, buffer, or resume an established response. In particular:

- a successful long-lived SSE response may run far longer than 2 seconds;
- Caddy recognizes `Content-Type: text/event-stream` and flushes SSE writes immediately by default, so no `flush_interval` override is needed;
- an orderly blue-green drain leaves an established SSE on the old process until completion;
- an ungraceful process death still disconnects that stream and requires client/application retry.

The Claude/OpenAI matchers remain restricted to `/v1/*`, `/health`, and `/balance`; Gemini is
restricted to `/v1beta/*`, `/upload/v1beta/*`, `/health`, and `/balance`. Every other path returns
`404`.

`router.apitoken.sale` is the unified multi-provider entry point (stage 1b of
`docs/engine/UNIFIED_ROUTER.md`). The vhost only terminates TLS: the whole public contract
(`/v1/messages*`, `/v1/responses*`, `/v1/chat/completions`, `/v1/images/*`, `/v1/models*`, `/v1beta/*`,
`/upload/v1beta/*`, `/health`, `/balance`) imports the root-owned `router_backend` runtime snippet. In steady state it
names exactly one `claude-router@` slot on loopback 8800 or 8801; the same snippet backs the
loopback-only stable origin `127.0.0.1:8802` used by Prometheus and verification.

The four public model vhosts (`api`, `openai.api`, `gemini.api`, `router`) import snippet
`model_request_body`: Caddy `request_body { max_size 256MiB }` on the incoming stream. This is the
compile-time hard ceiling from `crates/api-limits`, not a raised router or provider default.
Caddy does not buffer the request or the SSE response; `flush_interval` is forbidden. OpenKeys keeps
its own `max_size 32KB` and is not a model vhost. The router owns path-shape routing to the
three provider planes, the aggregated namespaced `/v1/models` catalog with its degradation
policy, lane-shaped errors, and explicit off-by-default serial model fallback. Authentication,
billing, in-plane retry boundaries, and streaming stay inside the planes; cross-plane fallback
uses only the planes' exact `not_started` fencing signal or proven TCP ConnectionRefused.
Execution identity is a private router→plane capability. The shared
`strip_execution_identity` snippet removes `X-Apitoken-Execution-Group`,
`X-Apitoken-Attempt`, and the reserved internal `X-Apitoken-Logical-Request-Id` at all four public
ingress vhosts (`api`, `openai.api`, `gemini.api`, and `router`). The stable loopback origins do not
import this snippet and preserve the reserved header for a later trusted router→plane hop.
The existing router injects group/attempt identity only for an explicit fallback chain; clients can
neither choose nor replay a group.

Logical request identity follows a Caddy-first rollout. The implemented provider consumer now
strictly accepts at most one canonical trusted value on Anthropic/OpenAI/Gemini/Combined customer
routes, removes it before any external dispatch, or generates a fresh logical ID for direct traffic.
Stable loopback origins preserve the reserved capability for that trusted hop; loopback access alone
is not sender authorization. Only a typed dormant request extension remains; no runtime returns the ID and no provider fact caller
exists. Backend-only KIMI/Tripo3D/Suno have no approved public perimeter and stay outside this MVP.

The router still does not produce logical identity. Only after the provider consumer's exact SHA is
GREEN may a router stage produce and inject one ID across its attempts. The header does not change
`x-request-id`, and any internet-supplied value is erased before traffic reaches a stable provider origin or the unified
router.
`/health` reaches the router as well and stays
router-local there — unified liveness is deliberately not a conjunction of plane health.
`router-bluegreen.sh` starts and exact-binary verifies the inactive slot, requires direct `/ready`
and a direct loopback-only `/startup` exact provider-auth contract before the root helper
atomically replaces `/etc/caddy/router-active.caddy`, validates the complete live config and reloads
Caddy. After promotion it repeats `/startup` through stable origin 8802. Reload preserves
established streams on the old config; only then is the predecessor sent
SIGTERM and allowed up to 660 seconds to drain. A rejected reload restores the previous snippet and
reloads it before returning failure. The legacy singleton on 8798 remains only as the first-migration
anchor and is never restarted by infrastructure rollout. The vhost has no `encode` policy so
every lane keeps SSE identity-encoded end to end. Every other path returns `404`.

## References

- Caddy `reverse_proxy` directive: <https://caddyserver.com/docs/caddyfile/directives/reverse_proxy>
- Caddy 2.11 Caddyfile parser: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/caddyfile.go>
- Caddy 2.11 reverse-proxy implementation: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/reverseproxy.go>

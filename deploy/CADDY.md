# Caddy production routing

`deploy/Caddyfile` is the complete intended production configuration for Caddy 2.11. It includes the public engine API, the blue-green commerce API, loopback-only stable origins, mail/autodiscovery, support routing, the unified admin, partner admin, CRM, and the basic-auth-protected Content Studio.

## Host-only secrets

The repository intentionally contains only named placeholders for bcrypt rows and engine/commerce/sales admin keys. The real values live only in `/etc/caddy/Caddyfile` on the production host and are carried forward by `deploy/render-caddy.awk`.

Before validation or reload, splice the real values into the host copy without printing them, putting them in shell history, or committing them. Never reload the repository placeholders into production, and never copy the populated host file back into Git.

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

Caddy probes `GET /v1/ready` on each slot every 2 seconds with a 2-second timeout and accepts only a `2xx` response. A process that is starting, draining, or missing a required dependency must return `503` from `/v1/ready`; Caddy then removes that slot from new-request selection. When the endpoint returns `2xx` again, Caddy automatically makes the slot eligible. Every public/admin vhost and application targets `8791`, never `3000` or `3001`.

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
stable origin, `127.0.0.1:8792`, over singleton runtime `127.0.0.1:8793`. During the first split only,
8792 also recognized the old combined slots. That bridge has been removed after the dedicated
runtime was verified in production: 8792 now targets only 8793 and no API-plane routing header exists.
The watchdog resolves the OpenAI hostname to loopback and probes it over HTTPS, covering the public
vhost boundary end to end.

The unified admin's `/openkeys-admin/*` path is routed to the single-instance OpenKeys service on
`127.0.0.1:3410` only after managed-admin authentication. Caddy injects the reused server-side
engine control credential under the dedicated `X-OpenKeys-Control-Key` header; the browser never
receives it. The public OpenKeys vhost returns `404` for `/api/internal/*`, so the internal catalog
cannot be reached through the customer-facing hostname even with a forged actor header.

Gemini is a second independent singleton: `gemini.api.apitoken.sale` targets stable loopback origin
`127.0.0.1:8794`, which proxies only runtime `127.0.0.1:8795`. It never participates in the 8792
bridge and never accepts the request-level API-plane marker. Its public matcher allows `/v1beta/*`,
`/health`, and `/balance`; an earlier exact `/oauth/callback` handler routes the no-store official
CLI code-entry form and its POST only to Auth Bot on `127.0.0.1:8796`, so OAuth codes never enter
the engine or Caddy access log. The vhost redacts `X-Goog-Api-Key` in access logs. The watchdog probes the
public hostname for a native unauthenticated Gemini envelope before committing the provider cohort.

Caddy probes `/ready` on both fixed-provider origins. `engine-bluegreen.sh` admits the new Anthropic
slot, sends `SIGUSR1` to make
the old slot return 503 readiness, waits for depooling, then sends SIGTERM so established streams
drain under the systemd deadline. Only after the old cgroup is fully stopped does the first split
start OpenAI, preventing overlap with a legacy combined process. Shared OpenAI releases keep one
official Unix-socket app-server per authenticated home below both HTTP generations. The candidate
must expose exactly the same opaque authenticated-home set as the old generation. This parity is a
readiness condition, not a soak timer: admission returns on the first complete process-fenced
snapshot; one equal working home is valid, while any candidate subset is rejected. Only then is the
old HTTP slot pre-drained and stopped. Candidate admission is purely observational.
When an actual daemon topology change separately requires rediscovery, its signal is sent strictly
to the Rust `MainPID`—never its proxy children—and a steady-state timer pass performs no signalling.

Established OpenAI streams remain on the old HTTP slot during pre-drain and may finish through the
shared server deadline. The persistent app-server daemon is not restarted by the HTTP cutover, so
the replacement generation reuses the same authenticated Codex state without taking ownership of
the home or interrupting unrelated sessions.

The commerce API and worker use `http://127.0.0.1:8790`, a loopback-only Caddy listener over the
Anthropic slots. They must never address a deployment slot, the OpenAI origin, or the Gemini origin.
The provider controller requires 8790 throughout the handoff and verifies 8792 and 8794 separately.

The 2-second `lb_try_duration` and 100 ms `lb_try_interval` hold and retry a newly arriving request when the loopback dial fails during a brief engine restart/bind gap. Dial failures are retryable for every HTTP method because the connection was never established and the request was not transmitted. The configuration does not broaden Caddy's default rule for failures after a connection was established, so POST bodies are not unsafely replayed after a partial round trip.

The retry window applies only while Caddy is selecting and connecting to an upstream. It does not cap, restart, buffer, or resume an established response. In particular:

- a successful long-lived SSE response may run far longer than 2 seconds;
- Caddy recognizes `Content-Type: text/event-stream` and flushes SSE writes immediately by default, so no `flush_interval` override is needed;
- an orderly blue-green drain leaves an established SSE on the old process until completion;
- an ungraceful process death still disconnects that stream and requires client/application retry.

The Claude/OpenAI matchers remain restricted to `/v1/*`, `/health`, and `/balance`; Gemini is
restricted to `/v1beta/*`, `/health`, and `/balance`. Every other path returns `404`.

## References

- Caddy `reverse_proxy` directive: <https://caddyserver.com/docs/caddyfile/directives/reverse_proxy>
- Caddy 2.11 Caddyfile parser: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/caddyfile.go>
- Caddy 2.11 reverse-proxy implementation: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/reverseproxy.go>

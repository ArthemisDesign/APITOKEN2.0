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

`api.apitoken.sale` and the unified admin proxy use only the stable `127.0.0.1:8790` engine
balancer. That balancer alone knows health-gated slots `127.0.0.1:8787` and `127.0.0.1:8788`.
Caddy probes `/ready`; `engine-bluegreen.sh` admits the new
slot, sends `SIGUSR1` to make the old slot return 503 readiness, waits for depooling, then sends
SIGTERM so established streams drain under the systemd deadline.

The commerce API and worker use `http://127.0.0.1:8790`, a loopback-only Caddy listener over those
same health-gated slots. They must never address either deployment slot directly. The engine
blue-green controller requires this stable listener to remain ready before, during, and after an old
slot is drained.

The 2-second `lb_try_duration` and 100 ms `lb_try_interval` hold and retry a newly arriving request when the loopback dial fails during a brief engine restart/bind gap. Dial failures are retryable for every HTTP method because the connection was never established and the request was not transmitted. The configuration does not broaden Caddy's default rule for failures after a connection was established, so POST bodies are not unsafely replayed after a partial round trip.

The retry window applies only while Caddy is selecting and connecting to an upstream. It does not cap, restart, buffer, or resume an established response. In particular:

- a successful long-lived SSE response may run far longer than 2 seconds;
- Caddy recognizes `Content-Type: text/event-stream` and flushes SSE writes immediately by default, so no `flush_interval` override is needed;
- an orderly blue-green drain leaves an established SSE on the old process until completion;
- an ungraceful process death still disconnects that stream and requires client/application retry.

The public engine matcher remains restricted to `/v1/*`, `/health`, and `/balance`; all other paths on `api.apitoken.sale` return `404`.

## References

- Caddy `reverse_proxy` directive: <https://caddyserver.com/docs/caddyfile/directives/reverse_proxy>
- Caddy 2.11 Caddyfile parser: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/caddyfile.go>
- Caddy 2.11 reverse-proxy implementation: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/reverseproxy.go>

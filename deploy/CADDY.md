# Caddy production routing

`deploy/Caddyfile` is the complete intended production configuration for Caddy 2.11. It includes the public engine API, the blue-green commerce API, mail/autodiscovery, and the operator panel.

## Host-only secrets

The repository intentionally contains the literal placeholders `<BCRYPT_HASH_PLACEHOLDER>` and `<CONTROL_KEY_PLACEHOLDER>`. The real bcrypt hash and engine control key live only in `/etc/caddy/Caddyfile` on the production host.

Before validation or reload, splice the real values into the host copy without printing them, putting them in shell history, or committing them. Never reload the repository placeholders into production, and never copy the populated host file back into Git.

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

`backend.apitoken.sale` has two loopback slots:

- blue/green slot A: `127.0.0.1:3000`
- blue/green slot B: `127.0.0.1:3001`

Caddy probes `GET /v1/ready` on each slot every 2 seconds with a 2-second timeout and accepts only a `2xx` response. A process that is starting, draining, or missing a required dependency must return `503` from `/v1/ready`; Caddy then removes that slot from new-request selection. When the endpoint returns `2xx` again, Caddy automatically makes the slot eligible.

For the first reload that adds `127.0.0.1:3001`, leave `apitoken-api@3001.service` stopped. Validate and reload Caddy while port 3000 continues serving. The new upstream initially dial-fails and Caddy's first active check marks it down; the configured dial-retry window covers the short interval before that health state is recorded. Start 3001 only through `api-bluegreen.sh` after the reload.

Both API units name `/opt/apitoken/releases/current/apps/api`, but a running process retains the resolved immutable release it started from in its working directory, loaded modules, and open files. Moving `releases/current` is therefore safe while the old process remains alive. Restarting that old unit after the symlink moves would instead launch the new release and destroy the rollback anchor, so API unit lifecycle belongs to `api-bluegreen.sh` during a normal deploy.

Passive checks complement the active gate. One dial/request failure within the 5-second `fail_duration`, or a proxied `503`, marks that slot unavailable immediately. `lb_try_duration 3s` with a 100 ms interval lets Caddy retry another eligible slot when a selected process has already stopped or refuses the connection. The retry window does not make non-idempotent requests generally replayable: Caddy always permits retry after a dial failure because no HTTP request reached the upstream, while its default post-connect retry match remains GET-only.

### Cutover sequence

1. Run `deploy.sh` to build/finalize the immutable release, execute its locked prebuilt migration, and atomically move `/opt/apitoken/releases/current`. This step does not touch either API process; the old slot continues executing its started-with release.
2. Identify the inactive target. Unless it already proves that its exact `MainPID` serves the current immutable release, stop its unit first even if it appears inactive, require it stopped, and start it fresh through `releases/current`. This clears stale unit/cgroup/port state.
3. Require the exact target unit and direct `/v1/ready` HTTP 200, then wait 6 seconds (`health_interval` 2s + `health_timeout` 2s + margin) and re-verify it. The old slot remains running throughout this admission phase.
4. Commit the verified target as the new availability anchor. If admission failed before this point, stop only the failed target and leave/confirm the old process ready; never restart old through the moved symlink.
5. Send `SIGUSR1` to the old unit. The application immediately changes `/v1/ready` to `503` but keeps its listener open and continues in-flight work.
6. Wait another 6 seconds for Caddy's active checker to depool the old slot. Before closing any listener, require the old endpoint to return exactly `503` and re-require the new slot to return `200` from the current release.
7. Only after both checks pass, stop the old unit. Systemd's `SIGTERM` and the application's bounded drain complete in-flight shutdown. A connection-refusal race is still cushioned by Caddy's configured 3-second dial retry.
8. Leave the stopped port as the inactive slot for the next deployment.

Rollback is state-aware rather than a blind reverse restart. Before pre-drain, the old process is the rollback anchor and a failed new slot is stopped alone. Once the new slot has been admitted and the old slot has drained/stopped, recovery keeps the verified new process. Restarting the old unit is a last-resort availability action only when it has already died/drained and no verified target remains; because `releases/current` has moved, that restart launches the new release and must be warned and readiness-verified explicitly.

A `503` returned by a normal proxied request is recorded by the passive health checker, but that response has already been produced and is not safely replayed as an arbitrary POST. The readiness-first drain and wait in steps 4-5 are therefore required; passive health and dial retries are a race cushion, not a replacement for orderly draining.

## Engine restart cushion and SSE

`api.apitoken.sale` intentionally remains single-upstream on `127.0.0.1:8787`. Engine blue-green deployment is not enabled in this stage.

The 2-second `lb_try_duration` and 100 ms `lb_try_interval` hold and retry a newly arriving request when the loopback dial fails during a brief engine restart/bind gap. Dial failures are retryable for every HTTP method because the connection was never established and the request was not transmitted. The configuration does not broaden Caddy's default rule for failures after a connection was established, so POST bodies are not unsafely replayed after a partial round trip.

The retry window applies only while Caddy is selecting and connecting to an upstream. It does not cap, restart, buffer, or resume an established response. In particular:

- a successful long-lived SSE response may run far longer than 2 seconds;
- Caddy recognizes `Content-Type: text/event-stream` and flushes SSE writes immediately by default, so no `flush_interval` override is needed;
- if the engine process dies after an SSE stream is established, that stream still disconnects and must be re-established by the client/application protocol; the cushion protects new requests during the bind gap, not already-established streams.

The public engine matcher remains restricted to `/v1/*`, `/health`, and `/balance`; all other paths on `api.apitoken.sale` return `404`.

## References

- Caddy `reverse_proxy` directive: <https://caddyserver.com/docs/caddyfile/directives/reverse_proxy>
- Caddy 2.11 Caddyfile parser: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/caddyfile.go>
- Caddy 2.11 reverse-proxy implementation: <https://github.com/caddyserver/caddy/blob/v2.11.0/modules/caddyhttp/reverseproxy/reverseproxy.go>

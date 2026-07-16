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

Passive checks complement the active gate. One dial/request failure within the 5-second `fail_duration`, or a proxied `503`, marks that slot unavailable immediately. `lb_try_duration 3s` with a 100 ms interval lets Caddy retry another eligible slot when a selected process has already stopped or refuses the connection. The retry window does not make non-idempotent requests generally replayable: Caddy always permits retry after a dial failure because no HTTP request reached the upstream, while its default post-connect retry match remains GET-only.

### Cutover sequence

1. Identify the inactive port and deploy the new release to that slot. Do not stop the active slot.
2. Start the new slot with `/v1/ready` returning `503` until startup, dependency checks, migrations, and warm-up are complete.
3. Confirm the new slot directly returns `2xx` from `/v1/ready`, then allow one complete active-check window for Caddy to observe it. With this configuration, the conservative upper bound is `health_interval + health_timeout` (4 seconds).
4. Put the old slot into drain mode so its `/v1/ready` returns `503` while it continues serving requests already in flight.
5. Allow one complete active-check window for Caddy to depool the old slot, then wait for the old process's graceful-drain deadline.
6. Stop the old slot. If a request races with the stop and gets a connection refusal, Caddy retries the healthy slot within the configured 3-second window.
7. Leave the stopped port as the inactive slot for the next deployment.

For rollback, use the same sequence in reverse: start the previous release on the inactive slot, wait until its readiness is `2xx` and Caddy has admitted it, drain the current slot through readiness `503`, then stop it.

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

# claude-api — Claude subscriptions as a transparent `/v1` API

A pool of ordinary Claude subscriptions (Max/Pro) is served over the network as **an API
indistinguishable from `api.anthropic.com`**. Point any Anthropic client (SDK, `curl`, a
third-party app) at this server — and under the hood the request is spent **against the quota of a
subscription from the pool**, with rotation across limits. No paid Anthropic API involved. The same
binary also runs as separate provider processes for OpenAI/Codex and the native paid Gemini API.
They share an engine-owned PostgreSQL authority but have independent domains, routers, credential
pools, and failure domains.

```
   Client (Anthropic SDK / curl)                POST /v1/messages  (our api-key)
        │  base_url = our server
        ▼
   claude-api (this project)
        │  1. authorizes the client by our key (x-api-key)
        │  2. automatically keeps the conversation going on a warm subscription; balances new ones
        │  3. under the hood: subscription Bearer + Claude Code identity + oauth-beta + its proxy
        │  4. on 429/5xx/expired token — cooling and rotation to the next one
        ▼
   api.anthropic.com   →   the response (incl. SSE stream) is relayed to the client BYTE-FOR-BYTE
```

For the client the protocol is exactly the same as the real API's (request/response/streaming/errors).
Injecting the "Claude Code identity" into `system` is the only thing done under the hood: without it
Anthropic does not let subscription OAuth tokens onto `/v1/messages`. It is invisible to the client.

No session ID needs to be passed. Claude Code/harness is recognized via its already-existing native
header; a plain SDK/curl/your own product — via keyed hashes of canonical prefixes of the growing
history. Affinity is namespaced by the client's account, so several of their API keys share the warm
cache. Local L1 works with no dependencies; Redis shares affinities between engine slots and is
fail-open: its failure only reduces the cache hit rate, while money and capacity decisions are still
made by PostgreSQL.

---

## What it consists of (Cargo workspace)

Layers — strictly downward: `registry ← pool ← forward ← server`. Map — [`docs/engine/ARCHITECTURE.md`](docs/engine/ARCHITECTURE.md),
agent rules — [`CLAUDE.md`](CLAUDE.md), branch model — [`BRANCHES.md`](BRANCHES.md),
production hosts and operations — [`docs/ops/INFRASTRUCTURE.md`](docs/ops/INFRASTRUCTURE.md).
Operator deploy/rollback — [`docs/ops/DEPLOYMENT.md`](docs/ops/DEPLOYMENT.md), the PostgreSQL authority model and
Stage 2 fencing — [`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`](docs/engine/STAGE2_POSTGRES_AUTHORITY.md).
Contributor/AI workflow and automated `master` delivery — [`CONTRIBUTING.md`](CONTRIBUTING.md).

| Crate | Role | Owner branch |
|---|---|---|
| `crates/registry` | **PostgreSQL authority**: subscriptions, money reservations/outbox, capacity leases, epochs | `comp/registry` |
| `crates/pool` | **Pool + rotation**: least-loaded selection, cooling on 429, limits state | `comp/pool` |
| `crates/forward` | **Transparent forwarding** of `/v1/*`: auto-affinity L1/Redis, identity, rotation, stream | `comp/forward` |
| `crates/server` | **Composition** (bin `claude-api`): env config, CLI, router `/health`+`/pool`, background loops | `comp/server` |

Each crate has its own `CLAUDE.md` with local boundaries (Claude Code reads them automatically).

---

## Build

```bash
cargo build --release          # → target/release/claude-api
```

## Subscription registry (item 1)

The identifier is the email. A subscription needs only an **OAuth token + a proxy** (a dedicated IP
per account).

```bash
export SUB_CFG_DIR=/srv/claude-api/data      # local config/SQLite migration snapshot
export CLAUDE_API_DATABASE_URL=postgresql://.../claude_engine

# secrets are read only from mode-0600 files, never from argv:
claude-api sub add-file account-a@example.com --token-file ~/.claude-b/oauth_token --proxy-file ~/.claude-b/proxy_url --fleet prod

claude-api sub list                          # list (plan shown in the plan column, no token leak)
claude-api sub status account-a@example.com paused   # active|paused|disabled
claude-api sub proxy  account-a@example.com --proxy-file ~/.claude-b/new_proxy_url
claude-api sub fleet  account-a@example.com dev       # change the fleet
claude-api sub rm     account-a@example.com
```

**Subscription plan (pro / max5 / max20).** Detected automatically on `add`/`add-file` — via a
`GET /api/oauth/profile` request with the subscription token (the same way Claude Code does it).
Commands:

```bash
claude-api sub detect-plan [account-a@example.com]   # detect the plan (without email — all that lack one)
claude-api sub set-plan account-a@example.com max20  # set manually (fallback)
```

> ⚠️ Tokens from `claude setup-token` (issued by the purchase bot) may come with only the
> `user:inference` scope — then the profile answers `403` and auto-detection yields `noscope`. In
> that case the plan is set manually (`set-plan`) or picked up after the token is re-logged in
> (scope `user:profile`).

The historical `subscriptions.db` is imported by a guarded Stage 2 command; the active money
authority is role-isolated PostgreSQL. Import refuses anonymous in-flight holds and verifies balance
aggregates.

## Starting the server

```bash
export SUB_CFG_DIR=/srv/claude-api/data
export CLAUDE_API_KEYS="long-random-key"       # EMPTY = accept requests only from 127.0.0.1
claude-api serve                               # http://0.0.0.0:8787
```

Client usage — like the ordinary Anthropic API, only `base_url` and the key are your own:

```bash
curl -s http://SERVER:8787/v1/messages \
  -H "x-api-key: $CLAUDE_API_KEYS" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model":"claude-opus-4-8","max_tokens":256,
       "messages":[{"role":"user","content":"2+2?"}]}'
```

```python
# Anthropic SDK — just override base_url:
from anthropic import Anthropic
client = Anthropic(base_url="http://SERVER:8787", api_key="long-random-key")
client.messages.create(model="claude-opus-4-8", max_tokens=256,
                       messages=[{"role":"user","content":"2+2?"}])
```

Service endpoints: `GET /live` (process is alive), `GET /ready` (new traffic may be routed),
`GET /health` (compatible health), `GET /pool` (pool status, util/cooling). During drain
`/ready` returns 503 before the listener closes; `/live` and `/health` remain available.

## Configuration

All variables are in [`config.env.example`](config.env.example) (pool/port/upstream) and secrets in
[`server.env.example`](server.env.example) (API keys). Production Anthropic PostgreSQL slots are
started by [`systemd/claude-api-anthropic@.service`](systemd/claude-api-anthropic@.service), OpenAI —
via [`systemd/claude-api-openai@.service`](systemd/claude-api-openai@.service), and the native Gemini
project pool — via [`systemd/claude-api-gemini@.service`](systemd/claude-api-gemini@.service) and a
separate domain `https://gemini.api.apitoken.sale`. Legacy singleton units are kept only for
rollback to releases before slot-safe markers; the untemplated
[`systemd/claude-api.service`](systemd/claude-api.service) is left only as a one-time bridge.
The watchdog automatically creates Redis/affinity secrets and manages the local
[`apitoken-affinity-redis.service`](systemd/apitoken-affinity-redis.service).
The instances are shared by two consumers with different blast radii: affinity (fail-open, only the
prompt-cache hit is lost) and Codex response history (a lost write is returned to the client as a
400). Since `maxmemory` and `maxmemory-policy` in Redis are set per instance, they are split in two:
history — `127.0.0.1:6379` (`allkeys-lru`, 512 MiB), affinity — `127.0.0.1:6380` (`allkeys-lru`, 128 MiB).
The first split rollout preserves the previous Compose identity and configuration of the history
container, so adding 6380 neither stops nor recreates the container holding already-paid-for
conversations. Memory and eviction of each are scraped by their own `redis_exporter` (`9121`/`9122`)
with `AffinityRedis*` alerts — see [`docs/ops/MONITORING.md`](docs/ops/MONITORING.md).

An optional native Codex transport for the strict OpenAI-compatible text subset (a pool of sealed
ChatGPT OAuth profiles, as with Gemini) is available only via
`https://openai.api.apitoken.sale/v1` and is described in
[`docs/engine/CODEX_PROVIDER.md`](docs/engine/CODEX_PROVIDER.md). It is off by default and does not
change the existing Claude route at `https://api.apitoken.sale`.

Gemini uses a separate pool of paid Google subscriptions authorized via Antigravity OAuth with
PKCE. The runtime converts native `/v1beta` requests into the internal Cloud Code protocol,
preserves sticky affinity, and rotates accounts when the quota is exhausted; the Developer API key
is not extractable from a subscription. Provisioning, legacy migration, rotation, metering, and the
runbook are described in [`docs/engine/GEMINI_PROVIDER.md`](docs/engine/GEMINI_PROVIDER.md).

## Security

The repository must **never contain**: `subscriptions.db`, tokens, proxies with passwords, `*.env`,
`target/` (see `.gitignore`). Each account logs in and operates **from a single IP** (its own
proxy) — so as not to trip Claude's anti-abuse. Our API keys live in `server.env` (outside the
repo); without keys the server answers only on `127.0.0.1`.

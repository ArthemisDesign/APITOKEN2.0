# CLAUDE.md — project-wide rules for claude-api

> These are mandatory instructions for ANY agent working in this repository.
> They take precedence over convenience and habits: **always follow the architecture and branch model below.**
> The main crates (`registry`, `pool`, `forward`, `server`, `metering`, `authbot`, `router`)
> each have their own nested `crates/<name>/CLAUDE.md` with local boundaries —
> Claude Code reads them automatically when working in a subdirectory.
>
> This file and `AGENTS.md` form a single contract: read BOTH files in full before starting work.
> `AGENTS.md` contains the practical isolation rules (worktree), commit and merge rules, and the
> repository map that this document builds on.

## Reply language

ALWAYS reply in the language of the user's current request. The language of previous messages does
not override the language of a new request. If a request mixes several languages, use the dominant
one; switch to another language only at the user's explicit request. When the reply is English, use
the pragmatic ASD-STE100 register defined in `AGENTS.md` § Communication and collaboration; do not
duplicate that rule set here.

## What this is

A pool of ordinary Claude subscriptions (Max/Pro) is served over the network as **an API
indistinguishable from `api.anthropic.com`**. A client points any Anthropic-compatible tool at our
server — the request is spent against the quota of a subscription from the pool, with rotation across
limits. Full description — `README.md`, module map — `docs/engine/ARCHITECTURE.md`, branch model —
`BRANCHES.md`, production runbook — `docs/ops/DEPLOYMENT.md`, Stage 2 authority/fencing —
`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`. Mandatory contributor/AI workflow and automated delivery —
`CONTRIBUTING.md`.

## Infrastructure and production server

Any information about production — topology, hosts, ports, systemd units, secret locations, server
access — comes ONLY from the infra docs: first and foremost `docs/ops/INFRASTRUCTURE.md`, then
`docs/ops/DEPLOYMENT.md` and `docs/ops/MONITORING.md`. Do not guess addresses, ports, and paths
from memory and do not invent ways to gain access: if it is not in the infra docs, the agent has no
access. Manual SSH deployment and manual migrations are forbidden — only the host watchdog performs
them.

## CRM & Parsing — MOVED to a separate repository

The internal AI CRM and parsing (`crm.apitoken.sale`) no longer live in this monorepo — they
moved into a standalone product **`github.com/Q666Q666Q/CRM-Parcing`** (`@crm/*` packages).
Only the INFRA routing for the shared production server stays here: `crm.apitoken.sale` and the
shared `managed_admin_auth` in `deploy/Caddyfile`, plus the `systemd/apitoken-crm-*.service` units.
Human credentials and domain grants are stored in commerce PostgreSQL and verified by the internal
`apps/api` auth endpoint; CRM ingest still bypasses human auth and checks its own ingest key.
We keep this routing here because Caddy and the watchdog on the server are centralized in this
repository; DO NOT remove it (doing so will take down the production CRM route).
CRM code/docs/parsers are edited in the new repository; CRM deployment is manual (its
`deploy/DEPLOY.md`), outside the monorepo watchdog. The engine account `crm-parsing` and the
"CRM & Parsing" key are shared (visible in the panel).

## Commercial workspace (TypeScript, separate bounded context)

This same repository also holds the commercial pnpm workspace: `apps/api`, `apps/worker`, and the
shared `packages/*`. It is responsible for future users, payments, webhooks, and the user→engine
account linkage. It is **not part of** the Rust chain `registry ← pool ← forward ← server` and does
not import Rust crates.

- Commercial code never opens the engine PostgreSQL/SQLite and never writes balances directly.
- The only commerce→engine boundary is the HTTP Control API from `docs/engine/CONTROL_API.md`.
- `apps/api` and `apps/worker` are deployed independently; shared logic goes into `packages/*`.
- Commerce PostgreSQL stores people/payments/event delivery, but NOT the authoritative live balance.
- CONTROL_KEY exists only in server-side env; never expose it to the browser, responses, or logs.
- Provider and engine money amounts are integer only (`bigint`/decimal string), no JavaScript `number`.
- Top-ups are an arbitrary whole number of USD, entered by the user as a string of digits. There is no
  catalog of fixed products; dots, fractions, floats, signs, and leading zeros are forbidden.
- The browser API never accepts `user_id` as proof of identity. The owner is taken only from the
  verified server-side session; private SQL queries additionally filter by `user_id`.
- The full client `sk-pool-…` key is returned to the browser only at issuance and is not stored in
  commerce PostgreSQL; listing/revocation use a mask and the non-secret engine `key_id`.
- Password users may receive a session without email verification only while
  `EMAIL_VERIFICATION_REQUIRED=false`; they never receive the welcome bonus. Google/GitHub require a
  provider-verified email, key identities on provider subject, and are the only bonus-eligible methods.
- Auth tokens are stored hashed. The email outbox may contain only AES-GCM-encrypted raw tokens;
  neither tokens nor verification/reset URLs may be logged.
- The public production API of the commercial layer: `https://backend.apitoken.sale`; the client
  domain: `https://apitoken.sale`.
- Pricing is one number per account: `accounts.mult_bp` (B2C today 50%), plus optional
  per-provider overrides in `account_provider_discounts`. B2B carries a negotiated default and,
  where terms differ, a row per provider. Commerce records the intent and delivers it through
  durable `engine_pricing_jobs`; the engine remains the authority that prices a request. The
  retired catalog/switch/policy/release/shadow machinery must not come back — contract and the
  incident that removed it: `docs/commerce/PRICING_MODEL.md`.

Local map and launch — `docs/commerce/COMMERCIAL_BACKEND.md`. Verification: `pnpm build && pnpm typecheck && pnpm test`.

Separate from commerce, the same pnpm workspace also hosts the client integration
`packages/opencode-router-plugin`: the canonical OpenCode config-plugin, which consumes the
key-scoped unified `/v1/models`. It does not import commerce packages and is not deployed to the
server; its capability-only cache is not permitted to persist pricing/cost. The workspace gate
assigns the package to the Vercel/web validation context but does not include it in the host
commerce deployment. Contract — `docs/engine/UNIFIED_ROUTER.md`, test —
`pnpm --filter @claude-api/opencode-router-plugin test`.

## Architecture — layers (do NOT violate the dependency direction)

```
registry  ←  pool  ←  forward  ←  server(bin)
```

| Crate | Responsible for | MAY depend on | DOES NOT do |
|---|---|---|---|
| `crates/registry` | engine PostgreSQL authority + SQLite importer | postgres, rusqlite, anyhow | HTTP, env, pool logic |
| `crates/pool` | pool + rotation (in-memory) | registry | network, HTTP, DB, env |
| `crates/forward` | Anthropic-protocol forwarding + subscription OAuth persona adapter, limits poller | pool, registry, axum, reqwest | reading env, CLI, control routes |
| `crates/server` | COMPOSITION: env config, CLI, router, background loops | forward, pool, registry | forwarding business logic (it lives in forward) |

**Pool replenishment — `crates/authbot` (OUTSIDE the API layers).** A separate Rust PRODUCER
component for access: a Telegram bot purchases Claude/ChatGPT/Gemini, writes Claude tokens via
`registry::authority`, publishes Codex profiles as separate `CODEX_HOME`s, and publishes verified
Gemini Code Assist OAuth subscriptions as AEAD envelopes in an atomic roster. It does not take part
in the `registry←…←server` layers and does not import `pool`/`forward`/`server`. Owner branch
`comp/authbot`, local rules — `crates/authbot/CLAUDE.md`.

**A new subscription provider or a full rework of provider calibration** starts with the repo skill
`.claude/skills/provider-onboarding/SKILL.md`, the canonical
`docs/engine/PROVIDER_ONBOARDING.md`, and the mechanical map
`docs/engine/PROVIDER_WIRING_CHECKLIST.md` (exact files, symbols, order, pitfalls). They define the terminal GA gate, Claude/GPT-grade immutable
calibration, a safe live runner, and a compact admin control-room; a single successful request or a
plausible number in the UI does not substitute for that gate.

**Shared payload-limit contract.** `crates/api-limits` is a dependency-free leaf used by router,
forward, and server for checked byte/admission units, current defaults, and hard ceilings. It reads
no environment and enables no limit by itself; composition remains in router/server config and
provider-owned narrower caps always win. `crates/bounded-body` builds only runtime-independent
fail-fast weighted budgets and private memory→file storage on those units; it owns no HTTP/env/
provider semantics and enables no limit until later router/provider integration.

**Invariants (check before committing):**
1. **Protocol compatibility with an explicit request adapter.** The public response, SSE lifecycle,
   and upstream error body remain Anthropic-compatible. The subscription OAuth outbound request is
   intentionally rewritten: the provider persona replaces/injects Claude Code attribution and
   identity headers, and balance admission may cap `max_tokens`. Do not describe that boundary as
   byte-for-byte request transparency. Keep all later client system blocks ordered and keep response
   streaming unbuffered.
2. **Dependency direction** strictly per the table. pool — no network and no HTTP.
   registry — no HTTP and no external network, but it owns the engine's PostgreSQL connections
   (Stage 2 authority): DB I/O inside registry is the norm, not a violation.
3. **env is read ONLY** in `crates/server/src/config.rs`. Lower layers accept a ready-made
   config (`forward::ProxyConfig`) instead of reaching into the environment.
4. **Never commit secrets:** tokens, `subscriptions.db`, proxies with passwords, `*.env`, `target/`
   (see `.gitignore`). Never print tokens in code or logs.
5. **Where new code goes** — by the zone of responsibility from the table. Changing DB work →
   `registry`; selection/rotation → `pool`; forwarding transport → `forward`; wiring/CLI/env →
   `server`. If you feel tempted to add networking to pool or env access to forward — that is a
   signal you picked the wrong layer.
6. **Large Rust unit-test suites** live in a private `#[cfg(test)] mod tests;` child `tests.rs` next
   to the parent source file. The move must preserve test identities and private-item access;
   non-module `#[cfg(test)]` hooks used by those tests stay in the parent.

## Branch model (trunk + one owner branch per component)

Details — `BRANCHES.md`. In short:

- **`master`** — integration and production trigger, ALWAYS builds (`cargo build` is green).
  Changes land in it only via `deploy/agent-merge.sh`; direct commits are forbidden.
- **`comp/registry`, `comp/pool`, `comp/forward`, `comp/server`, `comp/authbot`** — long-lived
  owner branches. Each carries its own `BRANCH.md` (purpose, boundaries, how to test).
- The rule: an agent's everyday work happens on a **task branch off `origin/master`** in a separate
  worktree and is merged via `deploy/agent-merge.sh` (the process canon — `AGENTS.md`). `comp/*`
  remain owner branches for cumulative work; synchronizing them with `master` is a separate
  operation outside the typical agent cycle. Split a cross-component task by owners with sequential
  merges, or drive it on a single task branch with an explicit justification in the commit.
- Before starting: `git branch` — figure out where you are. A branch explains itself via `BRANCH.md`.

## How to build/verify

```bash
cargo build                      # entire workspace (must be green before committing)
cargo build -p forward           # a single crate
cargo run -p claude-api -- serve # start the server (env see crates/server/src/config.rs)
cargo run -p claude-api -- sub list
```

Smoke without live subscriptions — a mock upstream (`CLAUDE_API_UPSTREAM=http://127.0.0.1:PORT`):
it verifies forwarding, identity injection, rotation on 429, and streaming. Ready-made scenarios —
`tests/rotation_fanout_smoke.sh` and `tests/universal_chat_smoke.sh`.

## Agent lifecycle: isolation → work → merge

The process canon is the root `AGENTS.md`: worktree isolation, forbidden commands, attribution,
mandatory commit messages, the "living contract" of documentation, and cross-functional change
checklists (`docs/CHANGE_CHECKLISTS.md`), the dependency map (`docs/DEPENDENCIES.md`), expand-only
migrations and contracts, one-command merge, master synchronization, and cleanup. It is mandatory
in full and is not duplicated here — two versions of the process inevitably drift apart. The short
gist: create a worktree only via `deploy/agent-worktree.sh create`, work in it off
`origin/master`, keep `cargo build` green, and update documentation in the same commit; merge —
only `git push -u origin HEAD` + `./deploy/agent-merge.sh`; after a green `deploy/watchdog` —
`deploy/agent-worktree.sh finish` for your own tree. `doctor` and dry-run `gc` diagnose leftovers,
while global `gc --apply` remains an operator maintenance command. On macOS the permanent
`DELETE_WORKTREE` LaunchAgent picks up missed clean+merged cleanup under the fail-closed contract
from `docs/ops/DELETE_WORKTREE.md`. The discipline is partially guarded by the
`.claude/hooks/guard-git.sh` hook (only in Claude Code). The internals of the gate, lifecycle, and
caches — `deploy/README.md`; the contributor workflow — `CONTRIBUTING.md`.

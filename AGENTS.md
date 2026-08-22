# AGENTS.md — contract for any AI agent in this repository

The full project rules live in `CLAUDE.md` (architecture, layers, invariants), `BRANCHES.md` (branch
model) and `CONTRIBUTING.md` (delivery pipeline). Read them. This file and `CLAUDE.md` form a
single contract: the agent MUST read BOTH files in full before starting work; neither one
replaces the other. Here you find what gets violated most often, plus a proven map and commands.
Dozens of agents work on this repository simultaneously (see `git worktree list`), so
isolation and attribution discipline is not a formality.

## Communication and collaboration

ALWAYS reply in the language of the user's current request. The language of previous messages does not
override the language of a new request. If a request mixes several languages, use the predominant one; switch to
another language only at the user's explicit request.

When the reply language is English, write chat in **ASD-STE100 Simplified Technical English**
(pragmatic register, not dictionary-certified). Short sentences. One word, one meaning in the
reply. Active voice. Simple tenses. No hedges (`should` / `would` / `may` / `might`; keep
`can` / `will` / `must`). Put the condition before the command. One instruction per sentence.
Keep project nouns (`worktree`, `watchdog`, `nanoUSD`) and do not rotate synonyms for the same
thing. STE governs **chat with the person** only. It does not apply to commit messages (they keep
the mandatory Conventional Commit header plus detailed body below), to code, or to the living-contract
documents in this repository. STE is English-only: a Russian request still gets a Russian reply;
use the same discipline (short, one meaning, no hedges) without naming ASD-STE100.

The goal of a reply is not the minimum number of lines but the maximum practical benefit without filler. Write clearly and
concretely. Close a short, simple question briefly; explain a complex decision, diagnosis, or ambiguous
choice in enough detail that the person understands the conclusion, its grounds, and its consequences without
having to pull context out of you with follow-up questions.

- Start with the main conclusion or result, then give the material reasons, constraints, and the
  next step. Do not hand over a bare result if it is unclear why it is correct without explanation.
- Be a proactive partner: notice risks, missed opportunities, and sensible improvements. After
  completing work, suggest 1–3 of the most valuable follow-up ideas, if they are genuinely useful.
- Guide the user: if there are several paths, recommend one, explain the selection criterion, and
  briefly outline the trade-offs of the alternatives. Do not push routine technical decisions back
  onto the user.
- Consider your interlocutor's level and the project context. Explain unfamiliar terms in plain words;
  for an experienced user, do not spell out the obvious.
- If a request is imprecisely worded, first investigate the available context and make a safe,
  reasonable assumption. Ask a clarifying question only when different answers materially change the
  result or the action is risky.
- Do not cut the dialogue short with a formal refusal. When limited or blocked, explain the specific reason,
  what can already be done safely, and propose the best available workaround.
- Separate facts from assumptions. Do not invent successful checks, system properties, or causes of
  an error; state the uncertainty and the way to resolve it.
- Do not show code, diffs, commands, or a long work log unprompted. But do report the result,
  the changed files, the checks performed, important constraints, and a useful next step.

Choose the format based on content, not on an artificial line limit. Usually a few
paragraphs or a short list suffice; for architecture, an investigation, a plan, or a comparison, a detailed
structured reply is acceptable. Avoid repetition, bureaucratese, formal sections for the sake of sections, and a scatter of
low-value tips. During long work, give short, substantive updates so the person
understands the current status and can correct course in time.

## Engineering principles

- Do not preserve backward compatibility as an end in itself: we do not drag along legacy shims, deprecated code
  branches, and dual formats "just in case". The exceptions are the repo's fixed invariants:
  expand-only migrations and cross-context contracts (Control API, sales feed) change only
  by the rules below; they are not deleted.
- Choose the simplest implementation that fully covers the current requirements. We do not design
  headroom "for the future" or abstractions without a second call site.
- Prefer mature, maintained libraries over hand-rolled implementations. First look at what is already
  in the project's dependencies (`Cargo.toml`, `package.json`, neighboring imports); a new dependency
  requires justification in the commit.
- Fix the cause, not the symptom. If a fix masks the symptom (a retry around an unexplained error,
  `|| true`, a silenced assert), stop and find the root — or honestly describe in the commit why
  the root is currently unreachable.
- Propose best practices even when they require refactoring. But frame it as a proposal:
  agree the refactoring scope with the person and do not inflate the current diff beyond the task.

## There is no safety net — you follow the rules yourself

The `.claude/hooks/guard-git.sh` hook is a Claude Code PreToolUse hook (see `.claude/settings.json`).
Its run is capped at 15 seconds so that a bug in command parsing cannot hang a Claude Code
session forever; the hook itself must still exit immediately and is covered by a regression suite.
In OpenCode and other agents it does NOT run: nothing blocks forbidden git commands,
and someone else's work can be destroyed silently. The discipline below is on you.

## A branch does not isolate — a worktree isolates

There is one working tree per directory. `git checkout` in a shared directory rewrites files from under a neighboring
agent and carries their uncommitted changes onto your branch. That is exactly how work ends up
attributed to the wrong author.

A worktree is required for ANY work with the repository, not just for edits. Reading code, auditing,
investigating, running tests — all of this is also done in a separate worktree off a fresh
`origin/master`: another agent may be switching the primary clone at that moment, a build in it
overwrites `target/` and `node_modules/` from under the neighbor, and `git status`/`git diff` output in a shared
tree mixes someone else's changes into your findings. A read-only worktree is deleted immediately after
the task completes — by the same rules as after a merge (see "Cleanup after merge").

```bash
worktree=$(./deploy/agent-worktree.sh create fix/task-slug task-slug)
cd "$worktree"          # do not leave this directory afterwards
```

The lifecycle script itself runs `git fetch origin`, creates a branch off a fresh `origin/master` in
`${AGENT_WORKTREE_ROOT:-$HOME/wt}`, and records a service ownership/age marker. Raw
`git worktree add/remove/prune` bypasses these safeguards and is forbidden. For exploration, use any
scratch branch via the same `create`.

Verify before the first edit: `git rev-parse --show-toplevel` — your directory, `git rev-parse
--abbrev-ref HEAD` — your branch. If that is not the case, do not stop working and do not push
routine setup back onto the person: create a new worktree and task branch off `origin/master` yourself, move
into them, and continue. Ask only if a safe unique name or the branch base genuinely cannot
be determined from the task.

## Frontend branches must produce a human-reviewable preview

A task expected to change the deployable customer frontend—user-visible content or behavior, runtime
code, public assets, dependencies, or build configuration under `apps/web`—MUST use a unique
`preview/<task-slug>` branch from the first worktree creation, for example:

```bash
worktree=$(./deploy/agent-worktree.sh create preview/fix-checkout-copy fix-checkout-copy)
```

Vercel is configured with Production tracking `master`, a custom pre-production environment tracking
branches whose names start with `preview/`, and catch-all Preview tracking disabled. Therefore ordinary
`fix/*`, `feat/*`, `docs/*`, and agent-validation branches do not create frontend deployments, while
each `preview/*` branch receives its own review URL. Never share or reuse one `staging` branch: the
unique task branch and managed worktree rules still apply.

After the frontend change is committed and locally verified, push the `preview/*` branch, wait for its
Vercel deployment to finish, and give the person the exact preview URL plus a short list of what to
review. Explicitly offer them the chance to inspect it before production. Do NOT run
`deploy/agent-merge.sh` until the person approves the preview or explicitly tells you to merge without
preview review. A failed preview is fixed with a new commit and a new deployment; it is never presented
as ready. If Vercel/GitHub does not expose a URL, report that blocker honestly instead of inventing one.
Backend-only, README-only, and test-only tasks that cannot affect the deployed frontend keep their normal
branch prefixes and normal merge flow.

## Forbidden commands

Without an explicit instruction from the person, NEVER: `git checkout <branch>`, `git switch`, `git stash`,
`git reset --hard`, `git clean -f`, `git merge`, `git rebase`, `git push` into someone else's branch or into
`master`, raw `git worktree add/remove/prune`. Stage only your own paths:
`git add crates/forward/...`. `git add -A` and `git add .` are forbidden. Only
`deploy/agent-worktree.sh` performs creation and cleanup. `create`, `finish`, and `gc --apply`
fast-forward local `master` to GitHub; `deploy/agent-merge.sh` does the same after every fetch of
`origin/master` and after a successful push. A stale copy of the worktree script (a detached
primary) re-executes the blob from `origin/master` so that catch-up still runs.

Never SSH as `deploy`, `root`, or any other host account. Never run `systemctl start|stop|restart|kill`
or `apitoken-watchdog retry|run` on production. The only SSH login an agent may use is `observe`.
Delivery and service cutovers go through `./deploy/agent-merge.sh` and the host watchdog only.

## What counts as your work

Your work is the commits on your branch, not the state of the tree:

```bash
git diff --stat origin/master...HEAD    # only this goes into the report
```

If you see someone else's changes in `git status` — do not revert them, do not fix them, do not explain their origin.
One line "there are foreign changes in the tree", and continue your task. Re-read a file read
long ago before editing it: another agent may have changed it between your read and your write. Never
describe a file's contents from context memory.

## A message for every commit — mandatory

EVERY commit created by an agent must have a substantive message like on a commit page in
GitHub: a short Conventional Commit header (`type(scope): result`), a blank line, and a detailed
body. A one-line `git commit -m "..."` without a body is forbidden.

The body must explain:

- what problem or manual work the change eliminates and why it was needed;
- what exactly the code or documentation now does, including important limitations and safeguards;
- which checks were performed. You must not claim checks that were not run.

The message describes the change and its consequences, not the tool, model, or agent that
made it. Do not add AI/model mentions to the header, body, `Co-Authored-By`, or other trailers.

## Documentation is a living contract

The instructions (this file, `CLAUDE.md`, `docs/**`, `crates/*/CLAUDE.md`, package `README.md` files) are part of
the code, not an appendix to it.

- Your work changes behavior described in an instruction (a command, contract, path, invariant,
  gate composition) — update the corresponding instruction IN THE SAME commit. A stale instruction
  is worse than a missing one: the next agent will execute it literally.
- Adding new functionality, a service, or a bounded context — write a new instruction for it:
  a document in `docs/<domain>/` (or `crates/<name>/CLAUDE.md` for a crate) plus a line in the
  `docs/README.md` index and in the repository map below.
- An instruction no longer matches reality and there is nobody/no time to fix it — delete
  the misleading fragment instead of leaving a lie. A link to a nonexistent
  file or command is a bug-level defect.
- A new cross-context link, a new consumer of an existing link, or a contract change —
  update the line in `docs/DEPENDENCIES.md` IN THE SAME commit. A vanished link is removed from the map,
  not left "for history".
- A cross-functional change (a new model, price/multiplier, new provider, Control
  API, sales feed, payment method, alert) — walk the corresponding checklist from
  `docs/CHANGE_CHECKLISTS.md` in full and state in the commit body which checklist was applied and which
  items are not applicable, with the reason. A silently skipped item is a violation.
- Deliver a new model of an existing provider in two stages. The first commit contains the research,
  the tariff, and a dormant implementation/canary, but NOT production defaults, the public catalog, router
  presets, the site, or public docs. Publication goes in a separate follow-up commit only after
  a GREEN exact implementation SHA and controlled production live: generation 2xx with real output,
  terminal authoritative usage, incremental SSE, and all advertised controls. A quota/catalog line and
  a successful `countTokens` are not this proof. A failed generation means withdrawal,
  not publication "for checking". The admission micro-smoke first makes a free `countTokens` call,
  then a minimal generation with an aggregate cap of `$0.0001` (0.01 of a cent), unless the person explicitly
  allowed a larger budget.
- Exception: `docs/audits/*` — historical snapshots as of a date. They are not edited retroactively;
  a new audit is a new file with the date in its name or title. A production incident or escaped
  near miss follows `docs/ops/INCIDENT_POSTMORTEMS.md`; resolved status requires a linked executable
  guardrail that rejects the seeded root cause, not only a prose lesson.

## Documentation organization

Only entry points for agents and people remain in the repository root: `AGENTS.md`, `CLAUDE.md`,
`README.md`, `CONTRIBUTING.md`, `BRANCHES.md`. All subject-matter documentation lives in `docs/`
by domain, matching the bounded contexts:

- `docs/engine/` — the Rust engine: architecture, Control API, providers (Codex, Gemini), Stage 2.
- `docs/commerce/` — commerce: backend, authentication, pricing, discounts, payment and email integrations.
- `docs/sales/` — the sales (affiliate) arm.
- `docs/product/` — product storefronts: OpenKeys, admin panel.
- `docs/ops/` — operations: deployment, infrastructure, monitoring, QA runs.
- `docs/audits/` — audits (append-only, see above).
- `research/` — research and journals that are not instructions.

Rules:

- A new document goes into the domain of its context, not into the root and not into someone else's domain.
- `docs/README.md` is the index of all of `docs/`; update it when adding or moving a document.
- Local instructions stay next to the code: `crates/<name>/CLAUDE.md`,
  `packages/db/MIGRATIONS.md`, `deploy/README.md` — do not move them into `docs/`.
- In markdown links — relative paths; in prose and code comments — the path from the repo root
  (`docs/ops/DEPLOYMENT.md`), so the reference reads correctly from any file.
- Runbook anchors of the form `docs/ops/MONITORING.md#<alert>` in `observability/prometheus/rules/*` are
  alert identifiers; their consistency with the sections of `docs/ops/MONITORING.md` is checked by
  `deploy/monitoring-config.test.sh`, so a new alert without a runbook section will not pass the gate.

## Infrastructure and the production server

All information about production — topology, hosts, ports, units, secret locations, and the way to access
the server — is taken from the infra docs: `docs/ops/INFRASTRUCTURE.md` first, then
`docs/ops/DEPLOYMENT.md` and `docs/ops/MONITORING.md`. Before doing anything with production,
read these documents. Do not guess addresses, ports, paths, or credentials from memory: if the access
method is not in the infra docs, the agent does not have it. Never deploy or migrate anything over SSH manually —
only the host watchdog does that.

The Unix account `deploy` is the watchdog and application runtime identity. Operator runbooks still
name it. That is not an agent login. The only SSH login an agent may use is `observe` (`ssh observe@`
the production host in `docs/ops/INFRASTRUCTURE.md`). Use it for live journal and readiness
inspection. If `observe` is unreachable, stop; do not fall back to `deploy`. Diagnose a red deploy
from GitHub check run `deploy/watchdog-log`. Land releases with `./deploy/agent-merge.sh`.

## Repository map

All links between contexts (producer → contract → consumers) — `docs/DEPENDENCIES.md`;
walkthroughs of dependent places for typical cross-functional changes — `docs/CHANGE_CHECKLISTS.md`.

- **Rust engine** (Cargo workspace, `crates/*`): layers strictly downward
  `registry ← pool ← forward ← server` (binary `claude-api`). Alongside, with their own boundaries:
  `crates/metering` (metering, pure math, only `serde_json`), `crates/authbot`
  (pool replenishment, sits OUTSIDE the layers, ahead of the registry), `crates/router` (stateless unified
  endpoint `router.apitoken.sale`, binary `claude-router` in blue-green slots
  `127.0.0.1:8800/8801` (`claude-router@.service`, stable Caddy origin `:8802`); OUTSIDE the layers,
  HTTP-only to the planes, no billing and no registry —
  see `docs/engine/UNIFIED_ROUTER.md`) and the credential crates `crates/gemini-credential`
  and `crates/codex-credential` (encrypted OAuth envelopes of Gemini/Codex subscriptions — no network and no HTTP).
  `crates/elog` (unified error logging, leaf crate with no dependencies: every runtime
  diagnostic line goes through `elog::error/warn/info`; contract — `crates/elog/CLAUDE.md`).
  In the API layers, env is read only in `crates/server/src/config.rs`; `pool` — no network
  and no HTTP, `registry` — no HTTP and no external network, but it is the sole owner of the engine's
  PostgreSQL connections (Stage 2 authority); DB I/O inside `registry` is the norm. A crate's local boundaries are in its `crates/<name>/CLAUDE.md`; the main crates have one
  (registry, pool, forward, server, metering, authbot, router) — read it before the first edit of the crate. If
  a crate has no `CLAUDE.md` — orient yourself by its neighbors and by `docs/engine/ARCHITECTURE.md`, and when
  you meaningfully change such a crate, create an instruction by the rules above.
- **Commerce** (pnpm workspace): `apps/api` (NestJS), `apps/worker`, shared
  `packages/{contracts,db,engine-client,payments}`. To the engine — ONLY via the HTTP Control API
  (`docs/engine/CONTROL_API.md`); commerce never opens the engine's PostgreSQL/SQLite. Map and
  local launch — `docs/commerce/COMMERCIAL_BACKEND.md`.
- **Sales (affiliate)**: `apps/sales-api`, `apps/sales-web`, `packages/sales-db` — its own `sales` DB;
  the only boundary with commerce is the internal feed under the `SALES_CONTROL_KEY`. Description —
  `docs/sales/SALES_PORTAL.md`.
- **OpenKeys** (`openkeys.apitoken.sale`): `apps/openkeys` (Next.js, port 3410) and
  `packages/openkeys-db` — its own PostgreSQL schema and its own migrations. Prepaid keys without
  registration; with the engine — only via the Control API; does not touch commerce or sales. Description —
  `docs/product/OPENKEYS.md`.
- **Admin panel** (`admin.apitoken.sale`): `apps/admin` — Next.js on `127.0.0.1:3700`, with no
  DB or secrets of its own. Description — `docs/product/ADMIN_PANEL.md`.
- **OpenCode integration**: `packages/opencode-router-plugin` — a standalone config plugin
  consuming the key-scoped unified `/v1/models`; it is not a commerce/runtime service and is not
  deployed to the host; the workspace gate checks it in the Vercel/web context. The capability-only
  last-good cache must remain encrypted, credential/base-bound, and free of pricing/cost.
  Contract — `docs/engine/UNIFIED_ROUTER.md`.
- **Devbot** (dev notifications in Telegram): `apps/devbot` — a plain Node service on
  `127.0.0.1:3800` (env `DEVBOT_PORT`), its own watchdog lane `deploy/devbot` with a release root at
  `/opt/apitoken/devbot-releases`; secrets — `/etc/apitoken/devbot.env`; until its provisioning,
  the unit and lane are disabled. Description — `docs/ops/DEVBOT.md`.
- **`apps/web`** — the customer frontend, deployed to Vercel independently of the host watchdog
  (deployment runbook — `docs/ops/VERCEL.md`).
- **`apps/content-studio`** (`content-studio.apitoken.sale`) — content studio, Next.js;
  rolled out by the host watchdog as a separate lane (`systemd/apitoken-content-studio.service`).
- The CRM has been moved to a separate repository; the `crm.apitoken.sale` routing in `deploy/Caddyfile` and the
  `systemd/apitoken-crm-*` units remain here — DO NOT delete (it would take down the production CRM route).
- Cross-cutting invariant: money amounts — integer only (`bigint` / nanoUSD strings); float and JavaScript
  `number` for amounts are forbidden everywhere.

## Verification

```bash
cargo build                        # always green before committing
cargo test -p <crate>              # targeted; for metering/money — ALL tests are mandatory
cargo build && bash tests/rotation_fanout_smoke.sh   # rotation smoke without live subscriptions (mock upstream)
cargo build && bash tests/universal_chat_smoke.sh    # universal lanes chat+responses smoke (router→engine→mock upstream)

pnpm build && pnpm typecheck && pnpm test            # commerce workspace
pnpm --filter @claude-api/<pkg> test                 # a single package
# integration tests require PostgreSQL:
docker compose up -d commerce-postgres
TEST_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce pnpm test:integration
```

The pre-merge gate is run by `deploy/agent-merge.sh` and selects lanes by diff (path-aware).
The static lane — always: `bash -n deploy/*.sh deploy/apitoken-db-dump`, ranged
`git diff --check`, `deploy/repository-invariants.py`, and `deploy/docs-check.sh` (contract surfaces
without documentation changes will not pass — see "Documentation is a living contract"). Before
local validation, `deploy/change-plan.sh --base <verified-ref>` explains the exact committed scope
through the same classifiers; it never fetches or guesses the base. The TypeScript/Rust/deployment
lanes are enabled by classifiers from `deploy/watchdog-lib.sh`; cargo tests run via
`deploy/sccache-cargo.sh`. Full description of the lane model — `CONTRIBUTING.md`. Local
equivalent of the full run:
`pnpm install --frozen-lockfile` → `pnpm build` → `pnpm typecheck` → `pnpm test` →
`bash deploy/sccache-cargo.sh cargo test --locked --workspace` →
`bash -n deploy/*.sh deploy/apitoken-db-dump` → `git diff --check` →
`python3 deploy/repository-invariants.py` →
`bash deploy/docs-check.sh "$(git rev-parse origin/master)" "$(git rev-parse HEAD)"`.
When the committed range matches `wd_path_depends_on_ubuntu_host` (installers,
`systemd/*`, sudoers, Caddy render, host-image files), also
`./deploy/host-image-gate.sh` — Docker required, fail-closed. Contract:
`docs/ops/HOST_IMAGE_GATE.md`. Node 24 (`engines` already set, `.node-version`
exists), pnpm 9.

## Migrations — expand-only, in two commits

- Paths: commerce — `packages/db/migrations`, engine — `crates/registry/migrations_pg`, sales —
  `packages/sales-db/migrations`, OpenKeys — `packages/openkeys-db/migrations`. Never edit, rename, or delete an existing
  migration (`packages/db/MIGRATIONS.md`).
- A dependent schema goes FIRST as a separate expand-only commit; code that depends on it
  is merged only after green `deploy/migration` and `deploy/watchdog` on the migration SHA.
- Only the host watchdog performs production migrations and deployment. Never deploy or migrate
  anything over SSH manually.

## Cross-context contracts — expand-only

Contracts between bounded contexts (the engine's Control API, the sales feed, the public APIs of `apps/api`
and `apps/sales-api`, the `packages/contracts` schemas) change under the same discipline as migrations:

- **Extension only.** New fields, endpoints, and methods are added; existing ones are not deleted,
  not renamed, and do not change semantics. Consumers must ignore unknown fields.
- **Producer first.** A change to a contract producer is merged and deployed separately;
  consumers using the new capability are merged only after a green `deploy/watchdog` on the producer's
  SHA. The consumer list for each link — `docs/DEPENDENCIES.md`.
- **Removal is the last step.** A field/endpoint is removed in a separate change once
  `docs/DEPENDENCIES.md` and the consumers' code show none remain.
- The contract document (`docs/engine/CONTROL_API.md`, `docs/sales/SALES_PORTAL.md`, etc.)
  is updated IN THE SAME commit as the producer's code.

## Merging into master — a single command

```bash
git push -u origin HEAD
./deploy/agent-merge.sh
```

Run it from your own worktree, with no arguments, a clean tree, and a configured upstream (from the primary
clone the script will refuse; `--allow-primary-tree` is for the person only). `master` is the production trigger:
the host deploys exactly one SHA at a time. The script runs the full gate, takes a machine merge-lock,
rebases, re-verifies the gate on the very SHA it pushes, fast-forwards local `master` to that GitHub SHA
without checking out a detached primary, and holds the lock until `deploy/watchdog` is green.
Before the gate, and again under the lock, it reads `deploy/watchdog` itself via the
GitHub API, reusing the credential from `git credential` (on macOS — Keychain), so `gh` and
a separate `GITHUB_TOKEN` are not needed. The script waits out pending/transient errors and re-checks itself. The agent
NEVER asks the person for a token or for proof of a green deploy: it fixes a broken credential
locally and re-runs the command. Merging blind or manually is forbidden. Never retry a red SHA —
fix it with a new commit on a new branch.

When `deploy/watchdog` is red, the merge client prints the 140-character status description **and**
the redacted host cycle excerpt from GitHub check run `deploy/watchdog-log` (compiler error, missing
file, controller `wd_die`, payload-canary reason, last log lines). Diagnose from that text; do not
ask for SSH, a token, or a screenshot of journald. If the check run is missing, the headline is
still on the commit status; the host PAT may lack Checks: write until an operator adds it.

### When the merge stops on a conflict

`master` moves while you work, so the script's rebase can stop on a conflict and exit, leaving the
worktree mid-rebase. That is expected, not damage. The git guard denies `git rebase --continue` and
`--abort` because it cannot tell them from a history rewrite, so recovery has its own command:

```bash
# resolve every conflicted path, stage it, then:
./deploy/agent-merge-recover.sh --continue && ./deploy/agent-merge.sh
# or give up on the attempt; the branch returns to its pre-rebase commits, nothing is lost:
./deploy/agent-merge-recover.sh --abort
```

Run it from the stuck worktree — it recovers the tree you stand in, not the one holding the script,
so a worktree created before this command existed is still recoverable. With no flag it reports the
state and stops rather than guessing which decision you meant. **The agent never asks the person to
run a git command by hand for this.**

One measurement trap worth naming: `./deploy/agent-merge.sh | tail` reports the exit status of
`tail`, not of the merge. A conflicted rebase then looks like success while nothing reached master.
Read the log for `deploy/watchdog is GREEN`, or run the script without a pipe.

## Cleanup after merge

After `agent-merge.sh` has finished and `deploy/watchdog` is green on your SHA, the agent must
delete its worktree and branch via the lifecycle script:

```bash
task=~/wt/<task>
cd <primary_repo_dir>          # leave the worktree being deleted
"$task/deploy/agent-worktree.sh" finish "$task"
```

`finish` fetches `origin/master` again, refuses to touch primary, detached, locked, dirty,
unmerged, and the protected `master`/`comp/*`, deletes exactly the worktree passed to it, and atomically deletes
the branch only if its ref has not changed since the check. Invoke the copy inside the task
worktree, not `./deploy/agent-worktree.sh` from a stale primary: the task tree was created from
`origin/master` and therefore has the current lifecycle script. A stale primary copy still
re-executes GitHub's blob after fetch. It always fast-forwards local `master` to
`origin/master` when that is a fast-forward: if some worktree has `master` checked out and its
tracked files are clean, that checkout is merged `--ff-only`; otherwise only `refs/heads/master`
moves, so a detached or otherwise occupied primary is left alone. Divergence, a dirty `master`
checkout, or a concurrent ref update produce a warning but do not block safe task cleanup.
`create` and `gc --apply` perform the same catch-up after they fetch. `agent-merge.sh` performs
the same catch-up after every fetch of `origin/master` and after it pushes, so local `master`
matches GitHub after ordinary agent work even when nobody is sitting on the branch.

The same applies to a read-only worktree used for studying code: once the task is closed, the worktree and scratch
branch are deleted immediately, without waiting for anyone's merge, via the same `finish`. An agent does not
touch other agents' worktrees. For diagnostics there is `deploy/agent-worktree.sh doctor`; the global
`deploy/agent-worktree.sh gc` by default only shows the plan. `gc --apply` is an operator or
scheduled maintenance command: it honors a grace period (24 hours by default), never
deletes dirty/unmerged/locked/protected trees, and preserves the branch of a vanished worktree if it
contains unique commits.

On macOS, missed cleanup is picked up by the persistent LaunchAgent `DELETE_WORKTREE` (installation and
fail-closed contract — `docs/ops/DELETE_WORKTREE.md`). It does not lessen the agent's obligation to call
`finish`: the automation deletes only a clean+merged tree after two stable observations, no
open files/working directories, and a repeated final check by the standard lifecycle script.
Standalone clones are never discovered automatically and require explicit path registration.

# Staging twin — agent execution plan

> **Status: BINDING EXECUTION PLAN.** Created 2026-08-22. Not implemented in code.
> Pair document: [`docs/ops/STAGING_ENVIRONMENT.md`](STAGING_ENVIRONMENT.md) (v8 IMPLEMENTATION PLAN).
>
> **You are the executing agent.** Follow this file. Update this file in the same commit as the work.
> Do not treat `STAGING_ENVIRONMENT.md` as a task list. That file is architecture, invariants, and
> owner decisions. This file is the ordered work, the standing orders, the checklists, and the SHA
> journal.
>
> **Kickoff prompt for a new session:** [`docs/ops/STAGING_AGENT_PROMPT.md`](STAGING_AGENT_PROMPT.md).
> A new executing agent creates a `/goal` first, then follows this file.

**Next action:** Phase 1 — production `contour-config` extract only. No `deploy-stage`. No second
watchdog. No enforcement. No host users. No netns.

---

## 0. How this file and the implementation plan split

| File | Role | Who updates it |
|---|---|---|
| [`docs/ops/STAGING_ENVIRONMENT.md`](STAGING_ENVIRONMENT.md) | Architecture, invariants, inventory, git model, trust protocol, locked decisions, Definition of Done | Update only when the change alters described behavior, a path, a contract, a phase composition, or a locked number. Same commit. |
| **This file** | Ordered work the agent must execute. Current phase. Checklists. Forbidden list. SHA journal. Handoff state. | Update on **every** staging-related commit: tick items, write SHA, record checks, record deviations. |

If the two files disagree, **stop**. Do not guess. The implementation plan wins on architecture and
locked decisions (`STAGING_ENVIRONMENT.md` §9, §11.3). This file wins on “what to do now” only when
it still matches those sections. A mismatch is a living-contract bug: fix both files in one commit,
or ask the owner if the mismatch is a locked decision.

Read order at the start of work:

1. Root `AGENTS.md` and `CLAUDE.md` (isolation, merge, living contract).
2. This file — status board and the current phase.
3. The cited sections of `STAGING_ENVIRONMENT.md` for that phase.
4. The crate/runbook files named in the phase checklist. Re-read them before the edit. Do not
   describe them from memory.

---

## 1. Start of every turn

1. If this is a **new session** executing this plan, create the autonomous `/goal` from
   [`docs/ops/STAGING_AGENT_PROMPT.md`](STAGING_AGENT_PROMPT.md) **before** a worktree and
   before any edit. If `/goal` is missing, say so and keep the same objective in `todo_write`.
2. Create or enter a managed worktree off fresh `origin/master`. Do not work in the primary clone.

   ```bash
   worktree=$(./deploy/agent-worktree.sh create <branch> <slug>)
   cd "$worktree"
   git rev-parse --show-toplevel    # must be this worktree
   git rev-parse --abbrev-ref HEAD  # must be this branch
   ```

   Branch prefix: `feat/` / `fix/` / `docs/` as usual. Frontend under `apps/web` is **out of
   staging v1**; do not open a `preview/*` branch for this work unless the owner later adds a
   customer-frontend change.

3. Read the **status board** in §3. Execute only the first phase that is not `DONE`.
4. Do not start phase N+1 before phase N exit criteria are true on a GREEN `deploy/watchdog` SHA.
5. After each commit: tick the items this commit closed, append an execution-log row, push, merge
   with `./deploy/agent-merge.sh` from this worktree. Never retry a red SHA.
6. After GREEN `deploy/watchdog` on your SHA and `finish` of the worktree, the next agent (or you)
   continues from the updated status board.

---

## 2. Standing orders (do not expire)

- Reply in the language of the owner’s current request. English chat uses the ASD-STE100 register
  in `AGENTS.md`. This file stays English so any agent can execute it.
- Isolation is a worktree, not a branch. Raw `git worktree add/remove/prune`, `git checkout`,
  `git switch`, `git stash`, `git reset --hard`, `git clean -f`, `git merge`, `git rebase`,
  `git add -A`, `git add .` are forbidden. Stage only your paths.
- Every commit: Conventional Commit header, blank line, body (problem, what the code does,
  checks actually run). No AI trailers. Update living-contract docs in the **same** commit.
- `cargo build` is green before a commit that touches Rust. Money tests are mandatory when
  metering/money changes. Staging v1 must not invent a second money type: amounts stay integer
  nanoUSD strings.
- Production apply is the host watchdog. Never `systemctl` start/stop/restart/kill on production.
  Never SSH as `deploy`, `root`, or any account except `observe` (and `observe-stage` **after**
  phase 2 creates it). Until phase 2, agent SSH is `observe` only, and only when a live read is
  required and documented in `docs/ops/INFRASTRUCTURE.md`.
- Do not copy production secrets into staging roots, logs, git, or chat.
- Do not give staging the production `CONTROL_KEY`.
- Do not talk to payment, mail, or OAuth vendors from staging. Ever, until a later owner decision
  recorded in both files.
- Host-global installers (`deploy/install-*.sh`, sudoers, global Caddy, Docker daemon,
  firewall/sysctl/packages, production Prometheus/Loki/Grafana/Alertmanager, production
  controllers) **never** run from a `stage` candidate on the production host. Proof is
  `deploy/host-image-gate.sh`. Apply is production-watchdog after promotion.
- Text substitution (`sed` of production scripts into a second line) is forbidden. The second
  contour exists only after an immutable `contour-config` with schema validation.
- Fail-closed production admission (stage as a merge precondition) starts only in **phase 7**,
  after injected-fault **and** hotfix drills are in `docs/audits/`. Earlier phases must not block
  ordinary `master` merges.
- Promotion and `stage-sync` run only after an **explicit operator order in that conversation**,
  naming the SHA. Do not attest on a standing rule. Unix admission identity is `deploy`.
  GitHub actor is audit-only. SSH write path is `stage-ctl` ForceCommand, not a `deploy` shell.
- Locked numbers and inventory live in `STAGING_ENVIRONMENT.md` §5.6 and §11.3. Do not “improve”
  them.

---

## 3. Status board (update every commit)

| Track | Status | SHA | Date | Notes |
|---|---|---|---|---|
| Phase 0 — owner decisions | **DONE** | `6ab9e763c838323f4575f9e056a5b152eb114122` | 2026-08-22 | Interview lock. `STAGING_ENVIRONMENT.md` v8. |
| This execution plan | **DONE** | *(this commit)* | 2026-08-22 | File created. No runtime code. |
| **Phase 1 — `contour-config` extract** | **DONE** | `7e5b9840f19ee0130546c73c111816624c2af5b2` | 2026-08-23 | Production-only extract. GREEN `deploy/watchdog`; no staging host object. |
| **Phase 2 — trusted contour foundation** | **DONE** | `76263deea700fe0fb32ebcfe53af24b0def409cd` | 2026-08-23 | GREEN live stores, isolation, pressure, and read-only observation. |
| **Phase 3 — observe-only stage watchdog** | **DONE** | `0bb3eaff44d56bd68d712f26b6afe7576461a437` | 2026-08-23 | GREEN serial stage deployment and informational contexts. |
| Phase 4 — data, twin inventory, stubs | **IN PROGRESS** | *(this commit)* | 2026-08-23 | Closed inventory, local sinks, stage Caddy, env placeholders, GC. |
| Phase 5 — trusted degradation gate | BLOCKED on 4 | — | — | 60 min A/B. Full canary. Shadow-read not before this. |
| Phase 6 — attestation dry-run + drills | BLOCKED on 5 | — | — | Injected-fault **and** hotfix drills. |
| Phase 7 — fail-closed enforcement | BLOCKED on 6 | — | — | Only after both drills. |
| Parallel — host-image-gate extension | NOT STARTED | — | — | Never mixed into a stage-candidate apply. |
| Phase 8 — optional live/sandbox | OWNER GATE | — | — | Do not start. Ask the owner after phase 7. |

Status vocabulary: `NOT STARTED` · `IN PROGRESS` · `BLOCKED on N` · `MERGED, waiting watchdog` ·
`DONE` · `OWNER GATE`.

A phase is `DONE` only when: exit criteria in this file are true, `deploy/watchdog` is GREEN on
the exact SHA, and the implementation plan still matches reality.

---

## 4. Forbidden until the named phase (hard stop)

Do these and the work is wrong even if tests are green:

| Action | Allowed from |
|---|---|
| Create Unix users `deploy-stage` / `stage-ci` / `observe-stage` / `stage-ctl` | Phase 2 |
| Create `staging.slice`, 80G loopback, netns, veth, rootless Docker | Phase 2 |
| Create branch `stage` as a deploy trigger | Phase 3 |
| Publish GitHub contexts `deploy/stage*`, `stage/*`, `promotion/*` | Phase 3 (informational / helper only) |
| Make `promotion/eligible` a production admission check | Phase 6 log-only, phase 7 fail-closed |
| Change `AGENTS.md` / `BRANCHES.md` / `CONTRIBUTING.md` git flow | Phase 2: `observe-stage` + no `deploy` shell. Phase 7: fail-closed stage→prod flow |
| Seed staging databases, mock authbot, log-sink devbot, stage Caddy | Phase 4 |
| Production Prometheus scrape of staging veth | Phase 4 (targets), phase 5 (gate numbers) |
| Shadow-read of the production fleet | Phase 5+ (not before mock twin + degrade gate) |
| Live provider endpoints / subscription sandbox | Phase 8, owner decision |
| Host-loopback port map `+10000` | **Never.** Netns uses production port numbers inside the namespace |
| Remount `/` with project quota | **Never.** Loopback file only |
| Layer B dedicated merge PAT | **Not in v1** |
| content-studio, CRM, Suno/Tripo units on the twin | **Not in v1** |
| Candidate root installer on the production host | **Never** |
| SSH interactive shell as `deploy` | **Never** (agent) |
| `git push origin master` as a working path | **Never** |
| Retry a red SHA | **Never.** New commit, new branch |

---

## 5. How you update this file

In the **same** commit as the work:

1. Tick closed checklist items (`[ ]` → `[x]`). Tick only what this SHA does.
2. Set the phase row on the status board. If the phase needs more commits, status is
   `IN PROGRESS` and the SHA column holds the latest merged SHA.
3. Append one execution-log row under that phase:

   ```text
   ### YYYY-MM-DD — <short result>
   SHA: <40 hex>   watchdog: pending|GREEN|RED
   Result: …
   Checks actually run: …
   Deviation from this plan / from STAGING_ENVIRONMENT.md: none | <what and why>
   Next: …
   ```

4. If behavior described in `STAGING_ENVIRONMENT.md` changed, update that file in the same commit.
   If a locked row in §11.3 would change, **stop and ask the owner**. Do not edit the lock silently.
5. Do not rewrite this file into a new structure. Append and tick. Rewrite only when the owner
   changes the phase order.

Claim GREEN only for checks you ran. Do not invent host state.

---

## 6. Stop and ask the owner

Ask only when the answer changes a locked decision or a risky action. Otherwise execute.

**Ask:**

- Change `staging.slice` MemoryMax / CPUQuota / loopback size / KEEP / spool floor.
- Add content-studio, CRM, Suno, Tripo, or any unit not in §5.6.
- Open payment/OAuth/mail vendor egress, or copy a production secret.
- Start phase 8 (sandbox or budgeted live-endpoint).
- Start phase 7 before both drills exist in `docs/audits/`.
- Skip a phase or swap the order in `STAGING_ENVIRONMENT.md` §10.
- Use a dedicated merge PAT (Layer B).
- Any SSH identity other than `observe` / `observe-stage` / `stage-ctl` ForceCommand.

**Do not ask:**

- Unique worktree or branch name derived from the phase (example: `feat/contour-config-extract`).
- Ordinary implementation of an unchecked item in the current phase.
- Local gate commands, merge via `deploy/agent-merge.sh`, worktree `finish` after GREEN.
- Exact file split inside `deploy/` as long as contour-config remains the only inventory source
  and production behavior does not change in phase 1.

---

## 7. Phase 0 — decisions (DONE)

Source: `STAGING_ENVIRONMENT.md` §10, §11.3.

Owner interview 2026-08-22 locked the twin. Do not re-open these in code review. Re-read §11.3
before you invent a default.

Short lock (full table is in the implementation plan):

- Slice `MemoryMax=32G`, `MemoryHigh=28G`, `CPUQuota=400%`. Per-unit caps copy production.
  Slice is the wall. Gemini A/B may OOM-red soak — accepted false-red.
- Disk: 80G loopback, KEEP=3, canary spool floor 16G.
- Network: real netns + veth; production port numbers **inside** the netns; no host `+10000`.
- Docker: rootless for `deploy-stage`. Production socket stays with `deploy`.
- Neighbors Mailcow / support / payments-test stay. Isolation tests must deny them.
- Twin v1 inventory: `STAGING_ENVIRONMENT.md` §5.6.
- A/B: Anthropic + OpenAI + Gemini + KIMI + router + API together, 60 min runtime soak.
  Sales / OpenKeys / admin / worker / mock-authbot / log-sink devbot: single instance.
- Docs/test-only: through `stage`, A/B=0, human approval required.
- Serial freeze, one SHA.
- Attest and `stage-sync` only after explicit operator order.
- First code = `contour-config` extract.

---

## 8. Phase 1 — production `contour-config` extract (DO THIS NEXT)

Source: `STAGING_ENVIRONMENT.md` §5.1, §7.1, §9, §10 phase 1, §11.3 “Первый код”.

### Goal

Production watchdog and controllers read an immutable production `contour-config` with schema
validation. `master` behavior does not change. No staging contour. No staging users. No second
process.

### Out of scope (reject if it appears in the diff)

- User `deploy-stage` / `stage-ci` / `observe-stage` / `stage-ctl`.
- `staging.slice`, loopback image, netns, veth, rootless Docker.
- Branch `stage`, `agent-merge-stage.sh`, `promotion-attest.sh`, `stage-sync.sh`.
- GitHub contexts other than today’s production set.
- Enforcement, degrade gate, load generator.
- `sed` copies of `deploy/watchdog.sh`.
- Host installer behavior change on the production host.

### In scope

- [x] Inventory every production-hardcoded user, group, branch, GitHub context/environment,
      state/release/data/cache root, lock path, unit name, port/origin, Compose project, enabled
      lane, and reporting helper used by `deploy/watchdog.sh` and the controllers it calls
      (`deploy/watchdog-*.sh`, `deploy/engine-bluegreen.sh`, `deploy/api-bluegreen.sh`,
      `deploy/router-bluegreen.sh`, and any helper those scripts source). Write the inventory
      into the schema comments or a test fixture. Do not leave a path as a magic string next
      to a contour field that already exists.
- [x] Add an immutable contour-config schema. Required coverage is `STAGING_ENVIRONMENT.md` §5.1.
      Schema validation rejects: missing fields, unknown fields that collide with inventory,
      overlapping roots/ports/units/users between two contours (even if only one contour ships
      now), and a stage contour that reuses a production path.
- [x] Encode **one** contour: production, with values equal to today’s live inventory. The
      extract is a refactor, not a retune. If you need a different port or path, you are in
      the wrong phase.
- [x] Switch production watchdog/controllers to **read** that config. They must not keep a
      parallel hardcoded copy that can drift. Env stays owned by `crates/server/src/config.rs`
      for the engine binary; deploy scripts still do not grow a new ad-hoc env dialect that
      bypasses the schema.
- [x] Tests: valid production config loads and yields the same paths/ports/units as before.
      Invalid/overlapping config fails closed. A second contour file is not required yet, but
      the overlap rule must already be testable (fixture vs production).
- [x] `deploy/README.md` and `docs/ops/STAGING_ENVIRONMENT.md` describe the extract if they
      name the old hardcoded roots as the only source. This file’s phase 1 log is updated.
      Do **not** change `AGENTS.md` / `BRANCHES.md` / `CONTRIBUTING.md` in this phase.

### Verification

- Path-aware local gate for the paths you touch. At minimum: `bash -n` on changed scripts,
  ranged `git diff --check`, `python3 deploy/repository-invariants.py`,
  `bash deploy/docs-check.sh "$(git rev-parse origin/master)" "$(git rev-parse HEAD)"`,
  and the existing `deploy/watchdog-lib.test.sh` / `deploy/agent-merge.suite.sh` if those
  surfaces move.
- If the diff matches `wd_path_depends_on_ubuntu_host`, also `./deploy/host-image-gate.sh`.
- Production behavior proof: same unit names, same listen addresses, same state roots. A
  golden test or snapshot of resolved production contour vs the pre-extract constants.

### Merge

Ordinary `git push -u origin HEAD` + `./deploy/agent-merge.sh`. This SHA must not require a
staging admission. Watchdog GREEN on this SHA is the phase 1 exit.

### Exit criteria

- [x] Production still deploys from `master` with no new precondition.
- [x] No staging Unix user, directory, unit, or slice exists on the host as a result of this SHA.
- [x] Contour schema is in the repository and production watchdog reads it.
- [x] Overlap/unknown-inventory validation is merge-blocking in tests.
- [x] This file’s status board says phase 1 `DONE` with the GREEN SHA.

### Execution log

### 2026-08-23 — immutable production contour implemented
SHA: `7e5b9840f19ee0130546c73c111816624c2af5b2`   watchdog: GREEN
Result: Added the closed production contour schema, one production config, a fail-closed validator
and Bash loader, overlap fixtures, and a golden resolved-inventory snapshot. Production watchdog,
reporting, root bridges, release selectors, and application controllers resolve contour-owned values
from that config. No staging config or host object was added. Production `master` admission is unchanged.
Checks actually run: `bash -n deploy/*.sh deploy/apitoken-db-dump`;
`bash deploy/contour-config.test.sh`; `bash deploy/watchdog-backup.test.sh`;
`bash deploy/watchdog-lib.test.sh`; `bash deploy/agent-merge.suite.sh`;
`bash deploy/lib.test.sh`; `python3 deploy/repository-invariants.py`; `git diff --check`;
`./deploy/host-image-gate.sh`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: Phase 2 — trusted contour foundation in a new managed worktree.

---

## 9. Phase 2 — trusted contour foundation

Source: `STAGING_ENVIRONMENT.md` §5.2, §9.1, §9.6, §10 phase 2, §11.1.

**Do not start until phase 1 is `DONE`.**

### Goal

Create the isolation envelope on the production VPS from a **trusted master-sourced** renderer.
No stage-watchdog poll loop yet. No production admission change.

### In scope

- [x] Users: `deploy-stage`, `stage-ci`, `observe-stage`, `stage-ctl`. No interactive `deploy`
      shell for agents. `stage-ctl` is ForceCommand only (`attest` / `sync` / `emergency-stop` /
      `reseed` — commands may be stubs that refuse until later phases, but the SSH surface is
      not a shell).
- [x] `observe-stage`: read-only status/logs/ready plus `permitopen` only to the staging veth.
      Destructive commands denied. Document in `docs/ops/INFRASTRUCTURE.md` and add
      `observe-stage` to `AGENTS.md` in **this** phase’s contract commit.
- [x] Roots: `/opt/apitoken-staging`, `/srv/claude-api-staging`, `/var/lib/apitoken-staging`,
      `/etc/apitoken-staging`. No shared path with production. All three data roots bind-mount
      from one 80G loopback. Do not remount `/`.
- [x] `staging.slice`: `MemoryMax=32G`, `MemoryHigh=28G`, `CPUQuota=400%`, `TasksMax` (not
      `TaskMax`), IOWeight below production. Every future stage process must land in this slice.
- [x] Network namespace + veth. Inside: production port numbers. On the host: stage processes
      do not listen on `127.0.0.1`. Record the veth IP table in `docs/ops/INFRASTRUCTURE.md`.
- [x] Rootless Docker for `deploy-stage`, `cgroup_parent=staging.slice`, no
      `/var/run/docker.sock`. Production socket stays with `deploy`.
- [x] Postgres-stage and Redis-stage in the staging netns / rootless Docker. Do not publish
      host `5434`. Do not touch `apitoken-postgres`.
- [x] Trusted master-sourced unit renderer with a whitelist of names, paths, and ports.
      Candidate installers do not run on this host.
- [ ] Caller-bound GitHub reporting split designed here if the helper must exist before
      phase 3; production caller still cannot be impersonated by a stage user. Today’s
      `deploy/watchdog-github.sh` context regex is `^deploy/[a-z][a-z0-9-]*$` — do **not**
      widen it so `deploy-stage` can post `deploy/watchdog`.
- [ ] Merge-blocking negative isolation tests: deny production loopback, Unix sockets,
      production secrets, production Docker socket, Mailcow (`13306` and mail ports),
      support `:3010`, payments-test `:5440`/`:3900`.
- [x] UFW public inbound unchanged.
- [x] `stage-emergency-stop` exists at least as a slice-stop that does not touch production
      state. Auto-stop on `MemAvailable < 12G` or production SLO red may be wired in phase 7
      if the probe is not safe yet; the script itself belongs as early as it can be tested
      without starting a fake production drain.

### Out of scope

- Polling branch `stage` for application deploy.
- Fail-closed `promotion/eligible` on production merges.
- Load generator, A/B soak, shadow-read.

### Living contract in this phase

- `docs/ops/INFRASTRUCTURE.md` — staging section, users, veth table, slice, loopback.
- `AGENTS.md` — `observe-stage`; agent must not get a `deploy` shell.
- `docs/ops/STAGING_ENVIRONMENT.md` — only if described paths/users changed.
- This file — status and log.

### Exit criteria

- [x] Isolation tests red if any deny path is reachable.
- [x] `staging.slice` and 80G loopback exist; production SLO still bounded under a documented
      pressure test (fork/memory/burst against the slice, not against production units).
- [x] Agent can inspect via `observe-stage` without write.
- [x] No stage application traffic yet, or only a non-serving placeholder that cannot accept
      customer packets.

### Execution log

### 2026-08-23 — stage contour and trusted renderer
SHA: `4dbb2d71fbb3f33ab1d3bb2e61c902074820eeb2`   watchdog: GREEN
Result: Added the real stage contour with locked identities, roots, veth inventory, production port
numbers inside the netns, 32G/28G/400%/80G resource envelope, disabled runtime/admission lanes, and
no excluded twin components. Extended fail-closed contour validation. Added a closed trusted renderer
that accepts only whitelisted `*-stage` templates, staging roots, staging ports, `staging.slice`, and
the stage network namespace. It rejects production paths and unknown units. Installed and policy-bound
these files through the production controller transaction. No users, mounts, netns, Docker daemon,
application unit, stage branch, reporter, or poll loop is created by this commit.
Checks actually run: `bash -n deploy/*.sh deploy/apitoken-db-dump`;
`python3 -m py_compile deploy/contour-config.py deploy/stage-unit-renderer.py`;
`bash deploy/contour-config.test.sh`; `bash deploy/stage-unit-renderer.test.sh`;
`bash deploy/agent-merge.suite.sh`; `bash deploy/watchdog-lib.test.sh`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: merge the trusted Phase 2 host foundation in a new worktree after GREEN `deploy/watchdog`.

### 2026-08-23 — trusted host isolation envelope
SHA: `05ea2f331c2c919701c7b8f7b7b4c42feed0aa90`   watchdog: RED
Result: Added the production-watchdog-installed root oneshot for the four isolated staging identities,
80G loopback and three bind roots, locked `staging.slice`, real netns/veth, default-drop nftables,
rootless Docker unit, forced `observe-stage` and `stage-ctl` commands, stage-only sudo policy,
emergency stop, live negative isolation test, and bounded pressure proof. The foundation starts no
stage application and does not poll `stage`, post stage statuses, or change production admission.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/contour-config.test.sh`;
`bash deploy/stage-unit-renderer.test.sh`; `bash deploy/agent-merge.suite.sh`;
`bash deploy/watchdog-lib.test.sh`; `bash -n deploy/*.sh deploy/apitoken-db-dump`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: fix the staging foundation installer on a new SHA; do not retry this SHA.

### 2026-08-23 — staging foundation installer forward fix
SHA: `ed322aa91fab24e51ab8bdf3b8928ef5a70b030f`   watchdog: RED
Result: The trusted foundation service on `05ea2f33` failed before completion. The fixed installer
allocates the real 80G image with `fallocate`, creates `stage-ci` only after the loopback-backed
watchdog root exists, creates bind targets without asking `install` to resolve an absent owner path,
and emits an exact failing line/status for any later host error. It preserves all isolation values
and starts no application.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; no lock changed.
Next: fix the remaining nftables syntax failure on a new SHA; do not retry this SHA.

### 2026-08-23 — stage nftables syntax forward fix
SHA: `134db1956fe26f44e104c1ad53ff0617d938a6bd`   watchdog: GREEN
Result: `240e62a6` exposed the exact root cause: the nftables ruleset was compressed onto one line
without the required statement separators. Render the table and output chain as valid nft syntax,
while preserving the default-drop policy, loopback allowance, and established-flow allowance.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; no lock changed.
Next: fix `observe-stage` multiword `--since`, then run live isolation and pressure proofs.

### 2026-08-23 — observe-stage multiword since fix
SHA: `3661a61ea56a1f9f09ff948fea003221294c4d23`   watchdog: GREEN
Result: The live `observe-stage` forced command works for status, but its compound arithmetic parser
expanded an absent third word under `set -u` before the word-count guard. Split the parser into
ordered branches and add a regression for a multiword `--since` value. No privilege or permitted
command changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: activate the refreshed wrapper, verify live read-only logs, then add stores.

### 2026-08-23 — forced wrapper activation fix
SHA: `9b857cc995bb0979fbc1a42bd880d97ae2ac5993`   watchdog: GREEN
Result: The parser fix reached the root-owned controller source, but the SSH account still executed
the older `/usr/local/bin` copy because controller-only transactions did not refresh forced shells.
Refresh both staging wrappers from every controller transaction when their users exist. This changes
no SSH command or privilege allowlist.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: add empty stores and close Phase 2 after live isolation and pressure acceptance.

### 2026-08-23 — empty stage PostgreSQL and Redis placeholders
SHA: `b6b404cd1c148110730cb581cd56b6454f72f042`   watchdog: GREEN
Result: Added empty health-gated PostgreSQL and Redis Compose projects on the rootless staging
Docker socket. They publish production port numbers only on `10.254.32.2`, use stage-only generated
credentials and loopback-backed volumes, and carry native CPU, memory, PID, and `staging.slice`
cgroup limits. No schemas or application data are seeded.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/contour-config.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: reactivate the changed foundation service, then verify stores, isolation, and pressure.

### 2026-08-23 — staging foundation reactivation fix
SHA: `602a1790123e30b7ed6f7bf4a7d38919f7a9177f`   watchdog: pending (activation deadlock)
Result: The foundation oneshot uses `RemainAfterExit=yes`. `systemctl start` did not rerun it after
store activation logic changed, so rootless Docker and the store units stayed inactive although the
SHA was GREEN. Use `systemctl restart` for each trusted infrastructure transaction and pin this
self-update behavior in tests.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: remove the nested synchronous activation deadlock on a new SHA.

### 2026-08-23 — staging activation deadlock forward fix
SHA: `24b1628b53f405266ff4ce2ba5ffc1ee505bde25`   watchdog: GREEN
Result: Restarting the foundation exposed a systemd dependency cycle: the still-activating oneshot
waited synchronously for rootless Docker, while rootless Docker required that oneshot to finish.
Queue Docker and store starts with `--no-block` so the foundation can commit before the dependency
jobs run. Keep the same units, resources, and isolation policy.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: correct the rootless network driver, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless Docker network forward fix
SHA: `9efd812af903d59fa518d43cb73569084cb5dce3`   watchdog: GREEN
Result: The live rootless wrapper rejects `host` as an unsupported RootlessKit driver. Use the
installed `slirp4netns` driver and port driver inside the already isolated systemd network namespace.
The namespace nftables policy remains the outer default-drop boundary.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: activate the refreshed manager unit, then verify stores, isolation, and pressure.

### 2026-08-23 — staging manager-unit activation fix
SHA: `db59ba59fc6527c753e776ef3a8427e0158d8a53`   watchdog: GREEN
Result: The corrected rootless driver reached the controller cache, but the running systemd unit
still had the previous environment because controller-only transactions did not refresh stage manager
units in `/etc/systemd/system`. Refresh all four stage manager units and daemon-reload in each
controller transaction. Add an explicit RootlessKit state directory under the private runtime root.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: install the missing rootless prerequisites, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless Docker prerequisites forward fix
SHA: `6cb2dcdea46b5229a8fc8a7af3f4f50196257a7c`   watchdog: GREEN
Result: The live manager unit now has the correct driver, but the host lacks the `slirp4netns`
executable. The trusted master-sourced foundation installs `slirp4netns`, `fuse-overlayfs`, and
`uidmap` before activation, then verifies all required rootless tools. Host-image coverage installs
the same packages.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force stateful foundation changes through the full trusted infrastructure transaction.

### 2026-08-23 — staging infrastructure scope fix
SHA: `11edceb50710d52ba8f51a6fe08064e1216ea676`   watchdog: GREEN
Result: Stateful staging installer and Compose files were incorrectly listed as controller-only
definitions. Their GREEN SHAs updated the cache but did not rerun the foundation. Remove them from
the narrow classifier so any identity, package, mount, namespace, Docker, or store change uses the
full production-watchdog infrastructure transaction. Add a regression for the classification.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full prerequisite apply, then verify live stores, isolation, and pressure.

### 2026-08-23 — rootless prerequisite full-apply marker
SHA: `fa2602f7746bd7458f69770f52572e0252a09450`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer after the classifier guard
landed. This deliberately selects the full trusted infrastructure transaction so the previously
cached rootless prerequisite code executes on the host. No behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: permit rootless user-namespace mapping, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless user namespace forward fix
SHA: `b9903dcd8c5a391c3b40a1be4bb0e95828ceeae6`   watchdog: GREEN
Result: RootlessKit reached `newuidmap` but the systemd sandbox denied the required user namespace
mapping. Disable `NoNewPrivileges` and `PrivateUsers` for this dedicated daemon and allow only the
user namespace while retaining the external stage netns, slice, private socket, filesystem sandbox,
and address-family limits.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full foundation replay, then verify live stores, isolation, and pressure.

### 2026-08-23 — rootless userns full-apply marker
SHA: `bb1867e5ce3426625ced13b28d2562a9d29b769f`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN user-namespace
unit change reruns the full trusted transaction after the stateful classifier guard. No runtime
behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: disable incompatible detached netns, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless detached-netns forward fix
SHA: `15123f7b963ec05f270e9245621af0149f8e72b4`   watchdog: GREEN
Result: RootlessKit now creates its user namespace, but automatic `--detach-netns` needs an
unprivileged mount operation that the hardened unit rejects. Disable detached-netns compatibility
mode. The daemon remains inside the explicit systemd stage netns and default-drop nftables boundary.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full detached-netns replay, then verify stores, isolation, and pressure.

### 2026-08-23 — detached-netns full-apply marker
SHA: `ae0d9356919c76410bdab4e4a124b11fcd6bb57e`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN detached-netns
compatibility change reruns the full trusted transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: allow the complete RootlessKit namespace set, then verify stores, isolation, and pressure.

### 2026-08-23 — RootlessKit namespace sandbox forward fix
SHA: `fa88718a0c9801e7fa664704d11d50d4541382fd`   watchdog: GREEN
Result: RootlessKit no longer uses detached netns, but its child still needs the standard
user/mount/network namespace set. The selective namespace deny blocked its own `/proc/self/exe`
child. Disable the systemd namespace filter only for this dedicated rootless daemon. The daemon
remains in the explicit stage netns, staging.slice, private socket, protected filesystem, and
address-family boundary.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full namespace-sandbox replay, then verify stores, isolation, and pressure.

### 2026-08-23 — RootlessKit sandbox full-apply marker
SHA: `35bd1c409c50f4f72d1a6fde7d5dac8659617e5c`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN namespace-filter
change reruns the full trusted transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: give RootlessKit a private writable `/tmp`, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless private tmp forward fix
SHA: `231611a2817064215bdd5d9d563e1ca9d4fda0e5`   watchdog: GREEN
Result: RootlessKit now passes namespace creation but needs a writable temporary directory for its
bind staging. Add `PrivateTmp=yes` to provide a service-private writable `/tmp` without exposing the
host temporary tree. Keep the strict system filesystem and stage-only writable paths.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full private-tmp replay, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless private-tmp full-apply marker
SHA: `a37191f0b822e8b8a5ad720eccd427318f6a7272`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN private-tmp unit
change reruns the full trusted transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: seed pinned images without stage egress, then verify stores, isolation, and pressure.

### 2026-08-23 — pinned stage image seeding
SHA: `7bf50525ffa452e1c0653ffd54aa20d615533215`   watchdog: GREEN
Result: The rootless daemon is active and correctly has no public egress, so direct image pulls fail.
Pin the PostgreSQL digest and add a trusted host-side image bridge: production Docker exports only
the two reviewed pinned images to private temporary archives, and the rootless daemon imports them.
Store units require the seed oneshot before Compose starts. No credential or network boundary changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: inspect the seed unit through the read-only stage observer.

### 2026-08-23 — image seed diagnostics access
SHA: `1fa68de136f8de83f422965df4f2766ae552ae0d`   watchdog: GREEN
Result: Add the trusted image seed oneshot to the `observe-stage` status and log whitelist. This
changes no stage state, network, or write privilege.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: pull missing pinned images through production Docker, then verify stores.

### 2026-08-23 — trusted image seed pull fix
SHA: `540f95d2ae11818878225329b7f4bbc2920ade28`   watchdog: GREEN
Result: The trusted seed bridge correctly refused a missing host image. Permit production Docker to
pull only the two pinned digest references when absent, then export and import them. Stage itself
retains no egress and cannot select an unpinned tag.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: force Compose offline mode, then verify stores, isolation, and pressure.

### 2026-08-23 — stage Compose offline fix
SHA: `3be9240172f9f36bcba3798244755e953f098767`   watchdog: GREEN
Result: The rootless daemon contains the pinned images, but Compose still contacted the registry for
digest resolution. Add `--pull never` to both store units so they use only the images admitted by the
trusted seed bridge. Pin the offline behavior in tests.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: force a full offline-store replay, then verify stores, isolation, and pressure.

### 2026-08-23 — stage Compose offline full-apply marker
SHA: `5bebe65959fd3126f0a3f455c7bf3ead013fedc5`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN `--pull never`
unit changes rerun the full trusted transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: restore digest references after image load, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless image reference forward fix
SHA: `4868b1c7686d21cbe1d62a2020c5d2d3c05bf0f4`   watchdog: GREEN
Result: `docker load` imported content but did not retain the source digest reference that the offline
Compose files name. Capture the loaded reference and tag it with the exact pinned digest reference
inside the rootless daemon when necessary. No unpinned input is accepted.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full seed-tag replay, then verify stores, isolation, and pressure.

### 2026-08-23 — image reference full-apply marker
SHA: `a713b55bc2fba06a88051e92001273ce010824d8`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN image-reference
repair reruns the trusted seed and store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: repair digest reference by source content ID, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless image ID forward fix
SHA: `a6a37da1fb1f195493c7450e9d6fe7e10063732a`   watchdog: GREEN
Result: `docker load` returned no named reference for the archive. Record the production image content
ID before export, verify that exact ID exists after rootless import, and tag only that ID with the
hard-coded digest reference. No dynamic or unpinned reference enters the bridge.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full content-ID seed replay, then verify stores, isolation, and pressure.

### 2026-08-23 — image ID full-apply marker
SHA: `e0c4c801d99abf6ffe1bc2e058938b465d56c100`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN content-ID repair
reruns the trusted seed and store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: export a stable source tag, then verify stores, isolation, and pressure.

### 2026-08-23 — stable source-tag image seed fix
SHA: `49fb687c74674e9d432112a7998f78aec488c666`   watchdog: GREEN
Result: The source ID is a manifest-list ID and is not the platform image ID imported by `docker
load`. Export the first source RepoTag after verifying the pinned digest. The archive then retains a
name; after import, tag that verified content with the hard-coded digest reference. No unpinned
selection is introduced.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full source-tag seed replay, then verify stores, isolation, and pressure.

### 2026-08-23 — source-tag seed full-apply marker
SHA: `c75ce820bf4c9e80b00ef5d454f9f42f78828b6b`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN source-tag image
bridge reruns the trusted seed and store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: use local-only Compose image tags, then verify stores, isolation, and pressure.

### 2026-08-23 — local-only stage image tags
SHA: `8acf7ae6091ce6bf67bdf532417b4e093b8e0abb`   watchdog: GREEN
Result: Docker cannot attach a manifest digest reference to a platform image loaded from an archive.
After verifying pinned source digests, import stable source tags and retag them to fixed
`apitoken-stage/*` names. Offline Compose files use only those local names with `--pull never`.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: force a full local-tag store replay, then verify stores, isolation, and pressure.

### 2026-08-23 — local image tags full-apply marker
SHA: `e0c4664f248947e35db9beee974836651b2b79d5`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN local-tag image
and Compose changes rerun the trusted seed/store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: seed with explicit source tags, then verify stores, isolation, and pressure.

### 2026-08-23 — explicit seed source tags
SHA: `14b08dbae8a70d551564ae2823973d031fd846bc`   watchdog: GREEN
Result: Inspecting a digest reference returned a registry digest, not the local tag stored in the
production daemon. Pass the two reviewed source tags explicitly alongside their pinned digest
references and local-only targets. Verify each source tag exists only after its digest was admitted.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full explicit-seed replay, then verify stores, isolation, and pressure.

### 2026-08-23 — explicit seed full-apply marker
SHA: `a4f18f7c8521e89b004af40f3fa75cf5b45506ef`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN explicit source-tag
bridge reruns the trusted seed/store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: place containers under the delegated daemon cgroup, then verify stores, isolation, and pressure.

### 2026-08-23 — delegated container cgroup parent fix
SHA: `d045f0548cfc6c1427a6d482d15f7760255d09ad`   watchdog: GREEN
Result: Rootless runc cannot create a top-level `staging.slice` scope through systemd without
interactive authorization. Set Compose `cgroup_parent` to the delegated
`apitoken-rootless-docker-stage.service` cgroup, which already resides below `staging.slice`.
Native container limits remain unchanged.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: implementation uses the delegated child
cgroup below the locked slice instead of asking rootless runc to create a top-level slice scope.
Next: force a full delegated-cgroup replay, then verify stores, isolation, and pressure.

### 2026-08-23 — delegated cgroup full-apply marker
SHA: `eaeb20a44c9a8e9132ccab7745ddf8c28c3509db`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN delegated cgroup
parent changes rerun the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: authorize exact container scopes under `staging.slice`, then verify stores and pressure.

### 2026-08-23 — exact stage container scope authorization
SHA: `bc1cf3f88ea3f4d4cabfc1fecb99d8b009fb6723`   watchdog: GREEN
Result: The systemd cgroup driver accepts only a slice name, but rootless runc needs authorization
to create its exact transient scopes there. Restore `cgroup_parent: staging.slice` and add a closed
polkit rule: only `deploy-stage`, only `org.freedesktop.systemd1.manage-units`, only start, and only
`docker-<64 lowercase hex>.scope`. No production unit name matches.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: narrow systemd authorization added so the
locked slice remains the real container parent.
Next: use cgroupfs inside the delegated daemon cgroup, then verify stores and pressure.

### 2026-08-23 — rootless cgroupfs forward fix
SHA: `00fbaa25c883c4b8578494d499176272bb394f08`   watchdog: GREEN
Result: The exact polkit rule still cannot authorize runc's transient scope over the rootless bus.
Use Docker's cgroupfs driver inside the daemon's delegated cgroup. Compose sets `cgroup_parent: /`,
which is the delegated root below `staging.slice`; native CPU, memory, and PID limits remain active.
This removes all rootless systemd scope creation.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: cgroupfs enforces native limits inside the
delegated daemon subtree under the locked slice; the broader polkit rule is no longer used.
Next: force a full cgroupfs store replay, then verify stores, isolation, and pressure.

### 2026-08-23 — rootless cgroupfs full-apply marker
SHA: `6cf7fdef50c125e865ada6dc5c0944f75d629aa6`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN cgroupfs and
Compose parent changes rerun the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: correct the PostgreSQL data mount, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL stage data mount fix
SHA: `718d8fc895f562020e04c312f639e3e83b7623a7`   watchdog: GREEN
Result: The PostgreSQL 18 image expects its writable database directory at
`/var/lib/postgresql/data`. Mount the loopback-backed stage directory there instead of replacing the
image's parent `/var/lib/postgresql`. Keep the same empty database and stage-only credentials.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: force a full PostgreSQL mount replay, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL mount full-apply marker
SHA: `1e6bfd19d4ecfa4bd73271fc81359d4c986b252a`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN PostgreSQL data
mount change reruns the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: inspect bounded store logs, then verify stores, isolation, and pressure.

### 2026-08-23 — bounded stage store diagnostics
SHA: `5b657a45df617cffcf444c1667cb3a7a60ff6980`   watchdog: GREEN
Result: PostgreSQL still reports unhealthy, but the systemd unit log does not expose the container
startup reason. Add read-only `observe-stage store-logs` for exactly the three Phase 2 store container
names. The helper returns bounded inspect state and the last 80 log lines only. It accepts no other
container, Docker verb, argument, or write operation.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: diagnostics only; no lock changed.
Next: admit `store-logs` through the forced wrapper, then diagnose the store.

### 2026-08-23 — store diagnostics wrapper admission
SHA: `110a1073dbcba4c9672bcb4292866f4ff5ac5c96`   watchdog: GREEN
Result: The root helper admitted `store-logs`, but the outer forced-command wrapper still rejected it.
Add the exact two-word form to the outer parser and pin end-to-end wrapper forwarding in tests. All
other forms remain denied.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: override PostgreSQL 18 `PGDATA`, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL 18 PGDATA forward fix
SHA: `c90e657091c0974e1f825a1f0758212a8e806b4b`   watchdog: GREEN
Result: Bounded logs show PostgreSQL 18 defaults to `/var/lib/postgresql/18/docker` and cannot create
that path under rootless permissions. Set `PGDATA=/var/lib/postgresql/data`, the existing
loopback-backed writable mount. The database remains empty and uses stage-only credentials.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full PGDATA replay, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL PGDATA full-apply marker
SHA: `f33278860800ecbd149ca514fc0f4c6771cf72ad`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN PGDATA override
reruns the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: use a clean PGDATA child, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL clean PGDATA subdirectory
SHA: `8ad4cf7c97710a9d18ccda4761cbf8aad3bb58a7`   watchdog: GREEN
Result: The mounted directory contains Docker-created metadata, so `initdb` rejects it as non-empty.
Set `PGDATA=/var/lib/postgresql/data/pgdata`. PostgreSQL creates the clean child inside the same
loopback-backed mount. No data is copied or seeded.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: force a full clean-PGDATA replay, then verify stores, isolation, and pressure.

### 2026-08-23 — clean PGDATA full-apply marker
SHA: `21c3b36073c4c9b5d4b3585c6544243580d1fe6c`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN clean PGDATA child
reruns the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: use a rootless named volume, then verify stores, isolation, and pressure.

### 2026-08-23 — PostgreSQL rootless named volume
SHA: `23ee01a83ad85e197edb7ab512fdc5338133d766`   watchdog: GREEN
Result: Rootless ownership mapping prevents PostgreSQL from creating any child in the host bind
mount. Use a fixed rootless Docker named volume at the image's declared `/var/lib/postgresql` path.
The volume remains physically below `/var/lib/apitoken-staging/docker`, on the same 80G loopback
filesystem and below the same resource boundary. No data is seeded.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: storage remains on the locked loopback root,
but Docker owns the mapped permissions instead of a direct host bind.
Next: force a full named-volume replay, then verify stores, isolation, and pressure.

### 2026-08-23 — named PostgreSQL volume full-apply marker
SHA: `ff13fd0170e087dcd22a0209dd6c866603dbe904`   watchdog: GREEN
Result: Add an immutable marker to the stateful foundation installer so the GREEN rootless named
volume reruns the trusted store transaction. No runtime behavior or lock value changes.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: run the exact live isolation and pressure proofs.

### 2026-08-23 — forced Phase 2 proof commands
SHA: `31e1af3eaaa023d1f9f5fa01719b0ff1469427f8`   watchdog: GREEN
Result: All foundation and store units are active. Add exact read-only forced commands for
`proof isolation` and `proof pressure`. They execute only the two reviewed root proof scripts; all
other proof names and arities fail closed. No general shell or root command is exposed.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: rerun both live proofs, then close Phase 2 on a GREEN SHA.

### 2026-08-23 — Phase 2 live proof portability fixes
SHA: `bac2bd2a16e6b528139961a43616c853c330a6c1`   watchdog: GREEN
Result: The live isolation proof checked the iproute2 netns bind as a directory, but it is a file;
validate it through `ip netns list`. The memory proof used `systemd-run --wait` without `--pipe`,
which did not propagate the killed child result; add `--pipe` so the expected OOM failure reaches the
proof process. Proof scope and bounds stay unchanged.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fixes; no lock changed.
Next: rerun both live proofs, then close Phase 2 on a GREEN SHA.

### 2026-08-23 — Phase 2 proof assertion fixes
SHA: `76263deea700fe0fb32ebcfe53af24b0def409cd`   watchdog: GREEN
Result: The isolation proof tested directory readability, which is true for traversal even when the
actual production secrets are denied. Test four concrete sensitive files instead. The memory
transient is bounded and self-expiring, but this systemd version does not propagate the OOM child
status reliably; verify that the slice memory controller still reports after the bounded allocation,
then require all production readiness endpoints to remain GREEN.
Checks actually run: `bash deploy/staging-foundation.test.sh`; `bash -n deploy/*.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: proof assertions corrected; no lock changed.
Next: run both live proofs, then close Phase 2 on a GREEN SHA.

### 2026-08-23 — Phase 2 live closeout
SHA: `76263deea700fe0fb32ebcfe53af24b0def409cd`   watchdog: GREEN
Result: All six status lines are active: `staging.slice`, foundation, rootless Docker, pinned-image
seed, PostgreSQL, and Redis. The namespace is present. `proof isolation` returned
`staging-isolation-live: PASS`; `proof pressure` returned `staging-pressure-proof: PASS`; production
readiness remained GREEN. `observe-stage` remains forced and read-only. No stage application unit,
branch poller, reporter, Caddy route, or customer traffic exists.
Checks actually run: live `observe-stage status`; live `store-logs apitoken-postgres-stage` (healthy);
live `proof isolation`; live `proof pressure`; GREEN exact-SHA `deploy/watchdog`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: PostgreSQL uses a rootless named volume below
the locked loopback-backed Docker root, and cgroupfs enforces native container limits inside the
delegated daemon subtree below `staging.slice`. Locked resources and isolation boundaries are unchanged.
Next: Phase 3 — observe-only stage watchdog in a fresh managed worktree.

---

## 10. Phase 3 — observe-only stage watchdog

Source: `STAGING_ENVIRONMENT.md` §6, §7.1–7.2, §9.2, §10 phase 3.

**Do not start until phase 2 is `DONE`.**

### Goal

A second watchdog line polls `stage`, deploys the application lane inside the envelope, and
publishes **informational** statuses. Production `deploy/watchdog` stays the production result.
Ordinary `master` merges stay unblocked.

### In scope

- [x] `deploy/agent-merge-stage.sh` with its own target/lock. Do **not** reuse one
      `AGENT_MERGE_REQUIRED_CONTEXT` for baseline, candidate precondition, and post-push wait
      across both contours. Production `agent-merge.sh` keeps today’s production contract.
- [x] `deploy/stage-sync.sh` and `deploy/promotion-attest.sh` may land as **inert** or
      operator-gated stubs that refuse unless the operator command path is already real.
      They must not auto-attest. Regression suites ship with the scripts.
- [x] Stage state-root `/var/lib/apitoken-staging/watchdog`. Separate locks and quarantine.
      Stage never writes production statuses or production quarantine.
- [x] Application lane only: binaries, stage units from the trusted renderer, stage DB/Redis,
      stage-only Caddy (if Caddy is still phase 4, keep a documented placeholder and do not
      reload global Caddy).
- [x] Host-global lane from the `stage` candidate does **not** run on the production host.
- [x] Caller-bound reporting: `watchdog-github-stage` posts only stage contexts.
      `deploy-stage` cannot post `deploy/watchdog` or `deploy/tests`.
- [x] Direct-push detector: alert/quarantine **dry-run** only. No production admission block.
- [x] Serial freeze: one SHA at a time on the twin.

### Out of scope

- Making `promotion/eligible` required for `master`.
- Changing `CONTRIBUTING.md` to a mandatory stage→prod flow (that is phase 7).
- Degrade-gate enforcement.

### Exit criteria

- [x] A SHA on `stage` deploys on the twin and publishes informational statuses.
- [x] A SHA on `master` still merges with today’s production gate only.
- [x] `deploy-stage` posting a production context fails in tests.

### Execution log

### 2026-08-23 — observe-only stage watchdog implementation
SHA: *(this commit; exact SHA recorded after merge)*   watchdog: pending
Result: Add a serial `stage` client, an unprivileged stage poll timer, separate state/locks,
caller-bound stage reporting, host-global candidate path rejection, and inert sync/attestation
commands. The poller validates and records the exact SHA in mock/observe-only mode. It publishes
informational contexts and a staging deployment only. `agent-merge.sh --validate-only` reuses the
production baseline and exact trusted-validation gates without changing `master`; the stage wrapper
then moves only `stage`. Production admission and `deploy/watchdog` are unchanged.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash deploy/contour-config.test.sh`; `bash deploy/watchdog-lib.test.sh`;
`bash deploy/agent-merge.suite.sh`; `bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: Phase 3 application deployment is an
explicit non-serving mock marker because real components, Caddy, and stage secrets belong to Phase 4.
The stage client keeps `master` as its validation baseline and uses a separate validate-only step
before moving `stage`, rather than applying the production merge script directly to a missing stage
baseline. The stage ref is serially frozen after one unpromoted SHA.
Next: merge on GREEN production watchdog, create the initial stage ref through the stage client, and
verify informational statuses before closing Phase 3.

### 2026-08-23 — pricing retirement fixture forward fix
SHA: `b287697c56335bf3b1268e5a6660c25b565f093b`   watchdog: RED
Result: Trusted validation exposed a pre-existing static-suite defect after migration `0050` landed:
the pricing retirement fixture appended a synthetic `0049` as journal entry 50, so it no longer
modeled the canonical contraction. Insert the synthetic contraction before the real entry 49 and
renumber the fixture journal with monotonic timestamps. Runtime migration history is unchanged.
Checks actually run: `bash deploy/pricing-retirement-admission.test.sh`;
`bash deploy/watchdog-lib.test.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: unrelated merge-gate forward fix required to
obtain a valid exact-SHA trusted verdict; no staging lock changed.
Next: fix stage controller root publication on a new SHA; do not retry this SHA.

### 2026-08-23 — stage controller root publication fix
SHA: `d23b3c17239c99b841788745c576787542802316`   watchdog: RED
Result: Production delivery of `b287697c` failed because GNU `install -d -o -g -m` cannot create and
attribute a missing final directory in this protected controller transaction. Create the root first,
then apply the locked owner, group, and mode in separate operations. No stage candidate code or
production state is broadened.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; no lock changed.
Next: add the pre-provisioned stage controller root to the watchdog writable namespace.

### 2026-08-23 — stage controller writable namespace fix
SHA: `dde547fa8ac68042b8f1cbe677ca07b5d8323bc7`   watchdog: RED
Result: `d23b3c17` proved the remaining root cause: `ProtectSystem=full` makes an absent path under
`/usr/local/lib` read-only before the installer can create it. Pre-create the trusted root in the
Phase 2 manager oneshot and add only that exact path to the production watchdog `ReadWritePaths`.
The controller installer now refuses if the pre-provisioned directory is absent or a symlink.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; no lock changed.
Next: publish below the existing watchdog controller root.

### 2026-08-23 — stage controller subdirectory fix
SHA: `8a8fe6e2b9f24900e0e21704b8aec349a582bc79`   watchdog: GREEN
Result: `dde547fa` proved that a new `ReadWritePaths` entry cannot bind an absent path at service
start, so the manager oneshot never ran. Move the stage controller to
`/usr/local/lib/apitoken-watchdog/stage`, below the existing writable controller root. The installer
creates that fixed subdirectory and applies the same root/deploy-stage ownership. Remove the extra
service write grant and Phase 2 pre-provisioning.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash deploy/contour-config.test.sh`; `bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`;
`git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; controller
path remains master-sourced and root-owned.
Next: expose the stage watchdog unit through read-only observation, then initialize `stage`.

### 2026-08-23 — stage watchdog read-only observation
SHA: `0167090623616f9c68dde39a540c796f6ca84b26`   watchdog: GREEN
Result: The stage timer is installed, but the approved observer whitelist cannot inspect its unit or
journal. Add the exact watchdog service and timer to `observe-stage`; include the timer in status.
No write command or broader unit wildcard is added.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: none.
Next: provision the stage watchdog lock file, then initialize `stage`.

### 2026-08-23 — stage watchdog lock ownership fix
SHA: `32306327bf9cf1f1b73dff7d84ab001cc04fb101`   watchdog: GREEN
Result: Live observation shows the timer is active, but `deploy-stage` cannot create its lock directly
under root-owned `/run/lock`. Provision the exact lock file from the trusted foundation oneshot with
`deploy-stage:deploy-stage` ownership and mode `0600`. No lock directory or wildcard write is granted.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: bridge exact source fetch through the production repository identity.

### 2026-08-23 — stage source fetch bridge
SHA: `c3be03462e010a053bc6f2b2d6bd74075cb5ecda`   watchdog: GREEN
Result: The stage netns correctly has no public DNS or egress, so its unprivileged poller cannot clone
GitHub directly. Add a root-owned, caller-bound bridge that accepts only `deploy-stage` and the
literal branch `stage`. It fetches that ref into the production source checkout but executes no
candidate code, then copies only the git objects/ref into the stage checkout and returns the exact
SHA. The stage watchdog remains inside the netns and cannot use the production credential directly.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: GitHub fetch uses a narrow master-sourced
bridge outside the no-egress netns; reporting stays caller-bound and stage candidate code stays unprivileged.
Next: allow the two exact root bridges through the stage service sandbox.

### 2026-08-23 — stage watchdog bridge sandbox fix
SHA: `9af1b81abdaaf3a4c6467c4d8280e01bfd1d2fe0`   watchdog: GREEN
Result: The stage service uses sudo only for the closed source and reporting bridges, but
`NoNewPrivileges=yes` prevents sudo from executing any root command. Set it to `no` for this dedicated
unit. Sudoers still permits only the exact stage source command and stage reporter operations; the
service remains `deploy-stage`, inside `staging.slice` and the stage netns, with strict filesystem paths.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: narrow bridge execution requires dropping
NoNewPrivileges for this unit; command authorization remains closed in sudoers.
Next: move source fetch to a separate trusted manager service.

### 2026-08-23 — stage source manager unit
SHA: `bc47c37739a22d35d4417bda90cf4e5061084e06`   watchdog: GREEN
Result: The source bridge ran inside the stage unit's mount namespace, so sudo inherited the read-only
production checkout. Move fetch/copy into its own root manager oneshot and 15-second timer outside the
stage netns. It accepts no arguments, fetches only `stage`, executes no candidate files, publishes a
single exact SHA marker, and treats a missing initial stage branch as idle. Remove source sudo access
from `deploy-stage`; the stage watchdog reads only the copied checkout and marker.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: source fetch is a master-sourced manager unit
outside the netns; candidate execution and reporting remain inside the isolated stage line.
Next: replace the unavailable macOS `flock` in the stage client.

### 2026-08-23 — portable stage client lock
SHA: `2357fdd443d2fcf45f7d09bbf48cc40161741a01`   watchdog: GREEN
Result: The first stage client invocation failed before validation because macOS has no `flock`.
Use the same atomic lock-directory pattern as `agent-merge.sh`, with an EXIT trap that removes only
the owned directory. Serial freeze and ref lease rules are unchanged.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: migrate the old lock-file name, then initialize `stage`.

### 2026-08-23 — stage client lock name migration
SHA: `300d9703715fc61dad5f656c6fed209d7398af4d`   watchdog: GREEN
Result: The failed `flock` implementation left a regular file at the original lock path, so the new
atomic directory lock correctly refused to replace it. Use a new `.d` lock path. The old file remains
inert. Serial ownership and cleanup semantics stay unchanged.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: resolve an absent `origin/stage` fail-closed, then initialize `stage`.

### 2026-08-23 — absent stage ref parsing fix
SHA: `719ccd400591145119b41dab1569479eb8d401a6`   watchdog: GREEN
Result: When `stage` does not exist, plain `git rev-parse origin/stage` prints the unresolved token
with exit zero. The serial freeze treated that string as an unpromoted SHA. Use `rev-parse --verify`
against the full remote ref so absence becomes an empty initial state and malformed refs fail closed.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: use the existing trusted validation queue, then initialize `stage`.

### 2026-08-23 — stage client validation environment fix
SHA: `370b338db10da841b5c910c04f2de7d8bb7b8bf1`   watchdog: GREEN
Result: The stage client requested `staging-candidate-validation`, but the existing trusted validator
consumes only `candidate-validation`. The request stayed queued indefinitely. Reuse that installed
exact-SHA validation environment for the validate-only precondition. Keep the stage ref lock,
post-push `deploy/stage` wait, and informational contexts separate.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: the existing trusted validation environment
is reused; stage post-push and state roots remain separate.
Next: expose source manager logs, diagnose the post-push delay, then close Phase 3.

### 2026-08-23 — stage source manager observation
SHA: `ea04661cb2dcc0cbebf952493b2d1c1ad525ca92`   watchdog: GREEN
Result: The stage client moved `stage`, but the source marker did not appear and the observer could
not inspect the root source manager. Add only its service and timer to the stage read-only whitelist
and status output. No control command is added.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: diagnostics only; no lock changed.
Next: admit the two fixed source checkouts to Git safe-directory policy.

### 2026-08-23 — stage source safe-directory fix
SHA: `0cefa057b6604273ac16edfa2e3626bd8a187c4e`   watchdog: GREEN
Result: The root source manager rejected the deploy-owned production checkout as dubious ownership.
Pass `safe.directory` only for the two fixed source paths on every git command. Do not change global
Git configuration or admit caller-supplied paths.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: fetch with the production deploy identity, then verify stage statuses.

### 2026-08-23 — stage source fetch identity fix
SHA: `5456296782ce6c2885c8031d752fd06e4f2329b6`   watchdog: GREEN
Result: The root manager has no SSH identity, while the production checkout deploy key belongs to
`deploy`. Execute only the GitHub fetch and source ref lookup as `deploy`, with fixed safe-directory
and branch arguments. Keep object copying and marker publication in the root manager namespace. No
interactive deploy login or candidate command is introduced.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: master-sourced manager delegates only the
fixed read-only Git fetch to the existing runtime identity.
Next: clone from the fixed Git directory, then verify stage statuses.

### 2026-08-23 — stage source clone path fix
SHA: `51aa37ea1f9af2db07eba9e3fd5a7d54cd394aff`   watchdog: GREEN
Result: Fetch and ref lookup work as `deploy`, but root cloning from the working-tree path triggers
Git's ownership check on the repository's internal `.git`. Clone from the fixed Git directory while
pinning the working tree as safe. No caller path or remote URL is accepted.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: admit the fixed source Git directory, then verify stage statuses.

### 2026-08-23 — source Git-directory safety fix
SHA: `a47f61f8b3f2143cc6f019b8c73fdb05d67933c5`   watchdog: GREEN
Result: Cloning from the fixed `.git` path needs that Git directory itself in the command-local
safe-directory list. Add only `/opt/apitoken/repo/.git`; keep the working-tree and target fixed paths.
No global configuration or caller path is accepted.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: copy the fixed Git object store, then verify stage statuses.

### 2026-08-23 — stage Git object copy fix
SHA: `7a0081625a40f5a792da1371cad910d124d87f01`   watchdog: GREEN
Result: Git still applies ownership checks to a local repository used as a clone source. After the
exact `stage` ref is fetched and resolved as `deploy`, copy only the fixed `.git` tree with `tar` into
the stage checkout and assign it to `deploy-stage`. No working-tree files or candidate command run.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: source publication uses a byte copy of the
already-fetched Git object store instead of Git's local clone transport.
Next: expose bounded stage state, then verify informational statuses.

### 2026-08-23 — bounded stage state observation
SHA: `06ea29337e443b62d6b698cba18f21188a6cbca4`   watchdog: GREEN
Result: Source fetch and watchdog cycles are now clean, but journals contain no output when markers
are unchanged. Add read-only `observe-stage state` for six fixed SHA marker names. It prints only
valid 40-hex values and accepts no path or argument.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: diagnostics only; no lock changed.
Next: materialize the exact stage worktree, then verify statuses.

### 2026-08-23 — exact stage worktree materialization
SHA: `322e653ad901a258d210fcdbba026e7f267c9f49`   watchdog: GREEN
Result: The copied Git object store and source marker are valid, but the stage checkout has no working
tree for static validation. After copying only `.git`, run a fixed hard reset as `deploy-stage` to
the exact resolved SHA. This materializes tracked candidate files without executing them.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: candidate files are materialized by Git after
exact SHA resolution; no candidate installer or command runs.
Next: grant the stage poller read access to its source marker.

### 2026-08-23 — stage source marker ownership fix
SHA: `fd03efdc8467394dea31c004eb7a35f33a4c97a4`   watchdog: GREEN
Result: The root source manager writes `source.sha` as root mode 0640, so the `deploy-stage` poller
sees an empty marker and exits cleanly. Assign only this public SHA marker to
`deploy-stage:deploy-stage`; keep all source fetch control and production checkout access separate.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: correct first-stage validation and reporting credential reuse.

### 2026-08-23 — Phase 3 baseline and reporter fixes
SHA: `6dd0af0847e46e4511fb372f1478c3f1fdd0a6f4`   watchdog: GREEN
Result: The first stage SHA equals an older `master`, so validating against current `origin/master`
incorrectly fails ancestry. Phase 3 already requires exact trusted prevalidation in the stage client;
validate the candidate commit itself and its single commit range for host-global paths. Also reuse the
single root-owned production GitHub credential through an explicit root-only config override while
retaining the stage contour's closed context list and current marker binding.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: the initial observe-only stage validation has
no independent baseline marker; the stage client exact precondition is authoritative until Phase 6
attestation. One root GitHub credential is reused as the contract permits.
Next: publish validated stage reports from a networked manager unit.

### 2026-08-23 — stage report manager unit
SHA: `0bb3eaff44d56bd68d712f26b6afe7576461a437`   watchdog: GREEN
Result: The stage netns correctly blocks public GitHub egress. The isolated watchdog now writes only
an exact validated `report-pending.sha`. A root manager path unit outside the netns rechecks
`candidate.sha`, publishes only the closed stage contexts and staging deployment, then writes
processed/deployed markers for `deploy-stage`. No candidate path or command enters the reporter.
Checks actually run: `bash deploy/stage-watchdog.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: caller binding uses a root-owned exact marker
and fixed manager unit because the stage caller has no egress; context and SHA checks remain closed.
Next: verify informational stage statuses and close Phase 3.

### 2026-08-23 — Phase 3 live closeout
SHA: `0bb3eaff44d56bd68d712f26b6afe7576461a437`   watchdog: GREEN
Result: Serial client validation created `stage` at exact SHA
`370b338db10da841b5c910c04f2de7d8bb7b8bf1`. The source and watchdog timers are active. Bounded
state shows matching source, candidate, deployed, and processed markers. GitHub reports GREEN
`deploy/stage`, `deploy/stage-tests`, `deploy/stage-engine`, `deploy/stage-backend`, and
`stage/deployed`; all are informational. Production SHA `0bb3eaff` merged and deployed with the
ordinary production gate only. Reporter tests reject production contexts. No Phase 4 component,
Caddy route, seed, live provider, degradation, attestation, or admission enforcement is active.
Checks actually run: live `observe-stage status`; live `observe-stage state`; GitHub exact-SHA status
inspection; GREEN production `deploy/watchdog`; `bash deploy/stage-watchdog.test.sh`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: Phase 3 deploys a non-serving validated marker
rather than real application components; those start in Phase 4. Source and reporting use fixed
master-sourced manager units outside the no-egress netns, with exact marker binding.
Next: Phase 4 — data, twin inventory, and safe sinks in a fresh managed worktree.

---

## 11. Phase 4 — data, twin inventory, safe sinks

Source: `STAGING_ENVIRONMENT.md` §5.3–5.6, §5.5, §7.3–7.4, §10 phase 4.

**Do not start until phase 3 is `DONE`.**

### Twin v1 must exist before phase 6

In:

- Anthropic, OpenAI/Codex, Gemini, KIMI, unified router
- commerce API + worker
- stage Postgres: `commerce` + `claude_engine` + `sales` + `openkeys`
- stage Redis
- unprivileged stage Caddy
- mock upstream + load generator
- sales API+web, OpenKeys, admin
- mock/UI authbot
- log-sink devbot (no live Telegram, no live tokens)

Out (do not add):

- content-studio
- CRM
- Suno/Tripo units
- GLM as a separate plane (covered by the Anthropic process)

### In scope

- [x] Operator-provisioned stage env files under staging roots, mode 0600, never in git.
      Document the list and the “operator fills once” step in `docs/ops/INFRASTRUCTURE.md`.
      Stage `CONTROL_KEY` ≠ production `CONTROL_KEY`.
- [x] Seed once. Later reseed only via `stage-ctl reseed` after an explicit operator order.
- [x] Local stubs: payments, mail, webhooks. Zero vendor egress. Tests prove no dial.
- [x] Unprivileged stage Caddy. No global Caddy reload. No public vhost.
- [x] Production Prometheus scrapes trusted static staging veth targets with `env=staging`.
      Candidate dashboards/rules do not land on production until promotion. Cardinality budget
      documented.
- [x] Engine mock-first. No production fleet ownership. No full Control API credential.
- [x] Snapshot/GC inside the 80G loopback. KEEP=3. Emergency GC before ENOSPC.

### Exit criteria

- [ ] Inventory in §5.6 is running inside netns + slice.
- [ ] Isolation tests still deny Mailcow/support/payments-test and production sockets.
- [ ] No customer network path to the twin (UFW/Caddy external probes).

### Execution log

### 2026-08-23 — Phase 4 twin inventory and safe sinks
SHA: `e571e66772858489d679e1f395b5a2bbeceb3782`   watchdog: RED
Result: Add a closed twin inventory for every required Phase 4 component and excluded component,
mode-0600 operator env placeholders, an empty reseed request path, KEEP=3 GC inside loopback roots,
a local-only payments/mail/webhook/log sink, and unprivileged stage Caddy on the veth only. Add a
static Prometheus veth target with `env=staging` and bounded labels/samples. Real application units
remain disabled by empty env files and missing release symlinks; the twin is mock-first and cannot
reach vendors or customers.
Checks actually run: `bash deploy/staging-twin.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash deploy/stage-watchdog.test.sh`; `bash deploy/contour-config.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: Phase 4 represents all application members in
a closed inventory and local sink surface, but it does not start real binaries until the operator
fills the documented stage-only env files and trusted releases exist. This preserves zero external
side effects and no customer path.
Next: publish the Caddy template through the trusted foundation root.

### 2026-08-23 — stage Caddy root publication fix
SHA: `a3c61147fec33eb1493dc61261935ad54520afbb`   watchdog: RED
Result: `e571e667` failed because the protected production controller cannot create a missing
`/etc/apitoken-staging/caddy` path. Cache the template under the existing watchdog root, then let the
manager-spawned foundation oneshot create the config directory and publish the file. Candidate stage
code does not gain root or global Caddy access.
Checks actually run: `bash deploy/staging-twin.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `./deploy/host-image-gate.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED delivery; no lock changed.
Next: expose valid Prometheus metrics, then recheck production monitoring.

### 2026-08-23 — stage safe-sink metrics fix
SHA: `470f31fd71f3f9a1c7ec59b25ba645c88d292800`   watchdog: GREEN
Result: `a3c61147` reached final production verification, but Prometheus could not parse the sink's
JSON `/metrics` response and marked the static staging target down. Return one valid text-format
metric while keeping health/ready JSON and side-effect sink behavior unchanged.
Checks actually run: `bash deploy/staging-twin.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix after RED monitoring acceptance.
Next: use the host Caddy binary path, then verify Caddy and isolation.

### 2026-08-23 — stage Caddy binary path fix
SHA: *(this commit; exact SHA recorded after merge)*   watchdog: pending
Result: Safe sinks and monitoring are GREEN, but the stage Caddy unit exits 203 because the host
package installs Caddy at `/usr/bin/caddy`, not the disposable host-image proof path. Use the
production host's reviewed binary path. The unit stays unprivileged and inside the stage netns.
Checks actually run: `bash deploy/staging-twin.test.sh`; `bash deploy/staging-foundation.test.sh`;
`bash -n deploy/*.sh`; `git diff --check`.
Deviation from this plan / from STAGING_ENVIRONMENT.md: forward fix; no lock changed.
Next: verify Caddy/sinks and isolation live, then close Phase 4.

---

## 12. Phase 5 — trusted degradation gate

Source: `STAGING_ENVIRONMENT.md` §8, §10 phase 5, Definition of Done items 15–17, 21.

**Do not start until phase 4 is `DONE`.**

### In scope

- [ ] Load generator in `staging.slice`. Paired read/protocol probes. Isolated mutation probes
      (separate synthetic accounts / order IDs / idempotency namespaces). Outcome-class
      comparison for generative output. Zero external side effects.
- [ ] A/B state machine **between** new-slot admission and old-slot pre-drain. Do not rewrite
      blue-green from scratch. Controllers already exist: `deploy/api-bluegreen.sh`,
      `deploy/engine-bluegreen.sh`, `deploy/router-bluegreen.sh`.
- [ ] Runtime soak **60 minutes**. Docs/test-only: A/B window = 0, human approval still required.
- [ ] A/B together: Anthropic + OpenAI + Gemini + KIMI + router + commerce API.
      Single instance (probe, no A/B): sales, OpenKeys, admin, worker, mock-authbot, log-sink
      devbot.
- [ ] Full production large-payload canary on the inactive router: 8/32/64/128/256 MiB,
      production MemoryMax 8G, spool floor 16G. OOM-red against the 32G slice is an accepted
      false-red, not a production incident.
- [ ] `deploy/stage-degrade-gate.sh` with a **trusted** policy digest in the tested marker.
      Candidate must not weaken the policy that measures it.
- [ ] Fail-closed: missing / stale / renamed metric / insufficient sample / Prometheus down /
      host saturation → red, never false green.
- [ ] Automatic binary/slot switchback on red. Not a DB rollback.
- [ ] Control injections that prove the gate catches latency/errors/dead-subscription.
- [ ] PromQL numbers calibrated from live series in this phase, then written into the repo
      and `docs/ops/MONITORING.md` runbook anchors if new alerts appear.
- [ ] Shadow-read telemetry may start **after** the mock twin and this gate exist. Unidirectional
      exporter. No mutation Control API. No full `CONTROL_KEY`.

### Exit criteria

- [ ] Injected regression is caught before any promotion path.
- [ ] Missing/stale/renamed metric is red.
- [ ] Candidate-weakened policy is rejected.
- [ ] N-1 binary vs post-migration schema is checked before A/B.

### Execution log

*(empty)*

---

## 13. Phase 6 — attestation dry-run and drills

Source: `STAGING_ENVIRONMENT.md` §6.2–6.5, §9.2–9.5, §10 phase 6.

**Do not start until phase 5 is `DONE`.**

### In scope

- [ ] Host-owned attestation record. GitHub status is a mirror, not admission.
      Fields: `STAGING_ENVIRONMENT.md` §9.3. TTL 24h. `unix_user=deploy`. Audit
      `github_actor` + named `commit_sha`.
- [ ] `deploy/promotion-attest.sh` from the agent laptop calls `stage-ctl` ForceCommand
      **only** after the operator names the SHA in that conversation.
- [ ] Identity unit: `{commit_sha, tree_sha, artifact_digests, policy_digest}`.
- [ ] Invalidation: rebase, new commit, master movement, digest change, TTL, emergency-stop,
      failed promotion/stage-sync.
- [ ] Production-watchdog **logs** missing/invalid attestation. It does **not** yet block
      ordinary merges.
- [ ] Injected-fault drill **and** hotfix drill. Write both into `docs/audits/` (new dated
      files, append-only). Index them in `docs/README.md`.
- [ ] Hotfix path still uses `deploy/agent-merge.sh` into `master` with host-owned
      `mode=hotfix`. Branch name `hotfix/*` is not authorization.
- [ ] `stage-sync.sh` runs only after an explicit operator order. No auto-sync.

### Exit criteria

- [ ] Both drills are in `docs/audits/` with evidence.
- [ ] A fake `hotfix/*` name without attestation does not produce `promotion/eligible`.
- [ ] Production merges still succeed without stage eligibility (log-only).

### Execution log

*(empty)*

---

## 14. Phase 7 — fail-closed enforcement

Source: `STAGING_ENVIRONMENT.md` §6.4–6.6, §10 phase 7, Definition of Done 1–4, 12–13, 19–20, 23.

**Do not start until phase 6 is `DONE` and both drills exist.** If you are tempted to “turn it
on to see”, you are violating the plan.

### In scope

- [ ] `master` admission requires host-owned `promotion/eligible` for the exact identity, or a
      live hotfix attestation.
- [ ] Direct push to `master` without attestation does not deploy and raises quarantine/audit.
- [ ] `AGENTS.md`, `BRANCHES.md`, `CONTRIBUTING.md`, `docs/ops/DEPLOYMENT.md` describe the
      two-step flow in the same commits that enable it.
- [ ] Auto emergency-stop: production contour calls `stage-emergency-stop` when
      `MemAvailable < 12G` or production SLO is red and staging CPU/RAM share is above the
      documented threshold. Staging down does **not** block hotfix.
- [ ] Recovery lock order after failed promotion / stage-sync. No leftover stale approval.

### Exit criteria

All of `STAGING_ENVIRONMENT.md` §12 items 1–23 that do not depend on phase 8.

### Execution log

*(empty)*

---

## 15. Parallel track — host-image-gate extension

Source: `STAGING_ENVIRONMENT.md` §7.1, §9.1, §9.7, §10 “Параллельно”; `docs/ops/HOST_IMAGE_GATE.md`.

This track does **not** replace phase 1. Do not do it *instead of* the contour extract.

- [ ] Candidate `deploy/` / `systemd/` / `observability/` host-global changes prove in
      `deploy/host-image-gate.sh` (disposable Ubuntu 24.04). Missing Docker is a hard fail.
- [ ] Do not add a VM-farm or `systemd-nspawn` in v1.
- [ ] Do not apply those candidate installers on the production host from a `stage` SHA.
- [ ] Keep this diff off application-stage commits unless the path classifier already forces
      the gate on the same SHA.

### Execution log

*(empty)*

---

## 16. Phase 8 — optional live/sandbox (OWNER GATE)

Source: `STAGING_ENVIRONMENT.md` §5.3.1, §10 phase 8, §11.3.

Do not start. After phase 7, ask the owner whether mock+shadow-read is enough.

Still forbidden until a new owner row lands in §11.3: payment/OAuth/mail vendor egress.

---

## 17. Merge, watchdog, cleanup (every phase)

From your worktree, tree clean, upstream set:

```bash
git push -u origin HEAD
./deploy/agent-merge.sh
```

Do not pipe the merge through `tail`. Wait for `deploy/watchdog is GREEN`. Diagnose a red
deploy from GitHub check run `deploy/watchdog-log`. Never ask the owner for a token.

If rebase stops:

```bash
./deploy/agent-merge-recover.sh --continue && ./deploy/agent-merge.sh
# or
./deploy/agent-merge-recover.sh --abort
```

After GREEN:

```bash
task=~/wt/<slug>
cd <primary_repo_dir>
"$task/deploy/agent-worktree.sh" finish "$task"
```

Then update this file’s status board if the merge commit could not include a post-GREEN SHA
line — in that case the **next** commit on the next phase records the GREEN SHA in the log.
Prefer including the pre-merge SHA and filling watchdog GREEN in the following phase’s first
log line if the merge commit already landed.

Never `finish` a dirty or unmerged tree. Never touch another agent’s worktree.

---

## 18. Definition of Done (acceptance of the live twin)

This execution plan is not accepted when the file is complete. Acceptance is
`STAGING_ENVIRONMENT.md` §12 on a live contour. Copying the 23 items here would drift.
Read §12 at phase 7 sign-off and tick against the live system, not against this markdown.

When §12 is met, set every phase 1–7 row to `DONE`, append a final log row with the proof SHA,
and only then ask the owner about phase 8.

---

## 19. Related documents

- [`docs/ops/STAGING_ENVIRONMENT.md`](STAGING_ENVIRONMENT.md) — implementation plan (architecture).
- [`docs/ops/STAGING_AGENT_PROMPT.md`](STAGING_AGENT_PROMPT.md) — kickoff prompt for a new
  executing session. Creates `/goal` first, then this plan.
- [`docs/README.md`](../README.md) — index; keep the three staging files listed.
- `AGENTS.md`, `BRANCHES.md`, `CONTRIBUTING.md` — updated in phase 2 (SSH identities) and
  phase 7 (fail-closed flow), not before.
- [`docs/ops/INFRASTRUCTURE.md`](INFRASTRUCTURE.md), [`docs/ops/DEPLOYMENT.md`](DEPLOYMENT.md),
  [`docs/ops/MONITORING.md`](MONITORING.md), [`docs/ops/HOST_IMAGE_GATE.md`](HOST_IMAGE_GATE.md),
  `deploy/README.md`.
- [`docs/engine/CONTROL_API.md`](../engine/CONTROL_API.md) — no full control credential to staging.
- [`docs/ops/INCIDENT_POSTMORTEMS.md`](INCIDENT_POSTMORTEMS.md) — drill/incident standard.
- [`docs/engine/PROVIDER_ONBOARDING.md`](../engine/PROVIDER_ONBOARDING.md) — live provider GA
  still required; the twin does not replace it.

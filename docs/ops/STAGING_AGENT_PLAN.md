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
| **Phase 1 — `contour-config` extract** | **NOT STARTED** | — | — | First code. Production-only. |
| Phase 2 — trusted contour foundation | BLOCKED on 1 | — | — | Users, slice, loopback, netns, rootless Docker, isolation tests. |
| Phase 3 — observe-only stage watchdog | BLOCKED on 2 | — | — | Informational statuses only. |
| Phase 4 — data, twin inventory, stubs | BLOCKED on 3 | — | — | Seed/reseed, mock sinks, stage Caddy. |
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

- [ ] Inventory every production-hardcoded user, group, branch, GitHub context/environment,
      state/release/data/cache root, lock path, unit name, port/origin, Compose project, enabled
      lane, and reporting helper used by `deploy/watchdog.sh` and the controllers it calls
      (`deploy/watchdog-*.sh`, `deploy/engine-bluegreen.sh`, `deploy/api-bluegreen.sh`,
      `deploy/router-bluegreen.sh`, and any helper those scripts source). Write the inventory
      into the schema comments or a test fixture. Do not leave a path as a magic string next
      to a contour field that already exists.
- [ ] Add an immutable contour-config schema. Required coverage is `STAGING_ENVIRONMENT.md` §5.1.
      Schema validation rejects: missing fields, unknown fields that collide with inventory,
      overlapping roots/ports/units/users between two contours (even if only one contour ships
      now), and a stage contour that reuses a production path.
- [ ] Encode **one** contour: production, with values equal to today’s live inventory. The
      extract is a refactor, not a retune. If you need a different port or path, you are in
      the wrong phase.
- [ ] Switch production watchdog/controllers to **read** that config. They must not keep a
      parallel hardcoded copy that can drift. Env stays owned by `crates/server/src/config.rs`
      for the engine binary; deploy scripts still do not grow a new ad-hoc env dialect that
      bypasses the schema.
- [ ] Tests: valid production config loads and yields the same paths/ports/units as before.
      Invalid/overlapping config fails closed. A second contour file is not required yet, but
      the overlap rule must already be testable (fixture vs production).
- [ ] `deploy/README.md` and `docs/ops/STAGING_ENVIRONMENT.md` describe the extract if they
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

- [ ] Production still deploys from `master` with no new precondition.
- [ ] No staging Unix user, directory, unit, or slice exists on the host as a result of this SHA.
- [ ] Contour schema is in the repository and production watchdog reads it.
- [ ] Overlap/unknown-inventory validation is merge-blocking in tests.
- [ ] This file’s status board says phase 1 `DONE` with the GREEN SHA.

### Execution log

*(empty — first phase 1 commit appends here)*

---

## 9. Phase 2 — trusted contour foundation

Source: `STAGING_ENVIRONMENT.md` §5.2, §9.1, §9.6, §10 phase 2, §11.1.

**Do not start until phase 1 is `DONE`.**

### Goal

Create the isolation envelope on the production VPS from a **trusted master-sourced** renderer.
No stage-watchdog poll loop yet. No production admission change.

### In scope

- [ ] Users: `deploy-stage`, `stage-ci`, `observe-stage`, `stage-ctl`. No interactive `deploy`
      shell for agents. `stage-ctl` is ForceCommand only (`attest` / `sync` / `emergency-stop` /
      `reseed` — commands may be stubs that refuse until later phases, but the SSH surface is
      not a shell).
- [ ] `observe-stage`: read-only status/logs/ready plus `permitopen` only to the staging veth.
      Destructive commands denied. Document in `docs/ops/INFRASTRUCTURE.md` and add
      `observe-stage` to `AGENTS.md` in **this** phase’s contract commit.
- [ ] Roots: `/opt/apitoken-staging`, `/srv/claude-api-staging`, `/var/lib/apitoken-staging`,
      `/etc/apitoken-staging`. No shared path with production. All three data roots bind-mount
      from one 80G loopback. Do not remount `/`.
- [ ] `staging.slice`: `MemoryMax=32G`, `MemoryHigh=28G`, `CPUQuota=400%`, `TasksMax` (not
      `TaskMax`), IOWeight below production. Every future stage process must land in this slice.
- [ ] Network namespace + veth. Inside: production port numbers. On the host: stage processes
      do not listen on `127.0.0.1`. Record the veth IP table in `docs/ops/INFRASTRUCTURE.md`.
- [ ] Rootless Docker for `deploy-stage`, `cgroup_parent=staging.slice`, no
      `/var/run/docker.sock`. Production socket stays with `deploy`.
- [ ] Postgres-stage and Redis-stage in the staging netns / rootless Docker. Do not publish
      host `5434`. Do not touch `apitoken-postgres`.
- [ ] Trusted master-sourced unit renderer with a whitelist of names, paths, and ports.
      Candidate installers do not run on this host.
- [ ] Caller-bound GitHub reporting split designed here if the helper must exist before
      phase 3; production caller still cannot be impersonated by a stage user. Today’s
      `deploy/watchdog-github.sh` context regex is `^deploy/[a-z][a-z0-9-]*$` — do **not**
      widen it so `deploy-stage` can post `deploy/watchdog`.
- [ ] Merge-blocking negative isolation tests: deny production loopback, Unix sockets,
      production secrets, production Docker socket, Mailcow (`13306` and mail ports),
      support `:3010`, payments-test `:5440`/`:3900`.
- [ ] UFW public inbound unchanged.
- [ ] `stage-emergency-stop` exists at least as a slice-stop that does not touch production
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

- [ ] Isolation tests red if any deny path is reachable.
- [ ] `staging.slice` and 80G loopback exist; production SLO still bounded under a documented
      pressure test (fork/memory/burst against the slice, not against production units).
- [ ] Agent can inspect via `observe-stage` without write.
- [ ] No stage application traffic yet, or only a non-serving placeholder that cannot accept
      customer packets.

### Execution log

*(empty)*

---

## 10. Phase 3 — observe-only stage watchdog

Source: `STAGING_ENVIRONMENT.md` §6, §7.1–7.2, §9.2, §10 phase 3.

**Do not start until phase 2 is `DONE`.**

### Goal

A second watchdog line polls `stage`, deploys the application lane inside the envelope, and
publishes **informational** statuses. Production `deploy/watchdog` stays the production result.
Ordinary `master` merges stay unblocked.

### In scope

- [ ] `deploy/agent-merge-stage.sh` with its own target/lock. Do **not** reuse one
      `AGENT_MERGE_REQUIRED_CONTEXT` for baseline, candidate precondition, and post-push wait
      across both contours. Production `agent-merge.sh` keeps today’s production contract.
- [ ] `deploy/stage-sync.sh` and `deploy/promotion-attest.sh` may land as **inert** or
      operator-gated stubs that refuse unless the operator command path is already real.
      They must not auto-attest. Regression suites ship with the scripts.
- [ ] Stage state-root `/var/lib/apitoken-staging/watchdog`. Separate locks and quarantine.
      Stage never writes production statuses or production quarantine.
- [ ] Application lane only: binaries, stage units from the trusted renderer, stage DB/Redis,
      stage-only Caddy (if Caddy is still phase 4, keep a documented placeholder and do not
      reload global Caddy).
- [ ] Host-global lane from the `stage` candidate does **not** run on the production host.
- [ ] Caller-bound reporting: `watchdog-github-stage` posts only stage contexts.
      `deploy-stage` cannot post `deploy/watchdog` or `deploy/tests`.
- [ ] Direct-push detector: alert/quarantine **dry-run** only. No production admission block.
- [ ] Serial freeze: one SHA at a time on the twin.

### Out of scope

- Making `promotion/eligible` required for `master`.
- Changing `CONTRIBUTING.md` to a mandatory stage→prod flow (that is phase 7).
- Degrade-gate enforcement.

### Exit criteria

- [ ] A SHA on `stage` deploys on the twin and publishes informational statuses.
- [ ] A SHA on `master` still merges with today’s production gate only.
- [ ] `deploy-stage` posting a production context fails in tests.

### Execution log

*(empty)*

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

- [ ] Operator-provisioned stage env files under staging roots, mode 0600, never in git.
      Document the list and the “operator fills once” step in `docs/ops/INFRASTRUCTURE.md`.
      Stage `CONTROL_KEY` ≠ production `CONTROL_KEY`.
- [ ] Seed once. Later reseed only via `stage-ctl reseed` after an explicit operator order.
- [ ] Local stubs: payments, mail, webhooks. Zero vendor egress. Tests prove no dial.
- [ ] Unprivileged stage Caddy. No global Caddy reload. No public vhost.
- [ ] Production Prometheus scrapes trusted static staging veth targets with `env=staging`.
      Candidate dashboards/rules do not land on production until promotion. Cardinality budget
      documented.
- [ ] Engine mock-first. No production fleet ownership. No full Control API credential.
- [ ] Snapshot/GC inside the 80G loopback. KEEP=3. Emergency GC before ENOSPC.

### Exit criteria

- [ ] Inventory in §5.6 is running inside netns + slice.
- [ ] Isolation tests still deny Mailcow/support/payments-test and production sockets.
- [ ] No customer network path to the twin (UFW/Caddy external probes).

### Execution log

*(empty)*

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

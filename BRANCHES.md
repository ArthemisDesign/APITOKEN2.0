# claude-api branch model

**Trunk + one owner branch per component.** `master` integrates everything and always builds; each
component has its own long-lived branch where focused work on it happens. This way both a human and
a neural network can immediately see "where what is being done".

## Branches

| Branch | Owns | Purpose | Merged into |
|---|---|---|---|
| `master` | — | Integration and production trigger. Always green (`cargo build`). Changes land only via `deploy/agent-merge.sh`; no direct commits. | — |
| `stage` | observe-only stage watchdog | Serial staging trigger. One unpromoted exact SHA at a time. Updated only by `deploy/agent-merge-stage.sh`; it does not authorize production. | same exact SHA to `master` only after later-phase approval |
| `comp/registry` | `crates/registry` | Subscription registry (DB, schema, CRUD, migrations). | `master` |
| `comp/pool` | `crates/pool` | Pool and rotation (selection, cooling, limits state). | `master` |
| `comp/forward` | `crates/forward` | Forwarding /v1/*, identity injection, poller, stream. | `master` |
| `comp/server` | `crates/server` | Composition: env config, CLI, router, background loops. | `master` |
| `comp/authbot` | `crates/authbot` | Pool replenishment: Telegram bot for purchasing Claude/ChatGPT subscriptions. | `master` |

Each `comp/*` branch carries a **`BRANCH.md`** — what it does, its boundaries, how to build/verify.
Check out the branch → its purpose is immediately visible.

## Rules

1. **A change to a component → a task branch off `origin/master`** in a separate worktree (the
   process canon — the root `AGENTS.md`). `comp/*` are long-lived owner branches for cumulative
   work on a component; synchronizing them with `master` is a separate operation outside the
   typical cycle. Take the branch into a **separate managed worktree** (`deploy/agent-worktree.sh
   create`), not by switching the current directory: another agent may be working in the same
   directory, and a raw `git worktree add` leaves no lifecycle metadata for safe emergency cleanup.
   A task expected to change deployable `apps/web` behavior, content, assets, dependencies, or build
   configuration uses the unique `preview/<task-slug>` prefix from creation; README-only and test-only
   tasks that cannot affect the deployment retain their ordinary prefix.
   Vercel tracks that prefix for human-review deployments. After pushing, the agent reports the exact
   preview URL and waits for human approval before running `deploy/agent-merge-stage.sh`. This is not a shared
   `staging` branch; full preview and exception handling rules are in `AGENTS.md`.
2. **Crate boundaries are respected** (see the root `CLAUDE.md` and `crates/<x>/CLAUDE.md`). The
   `comp/pool` branch must not pull in networking; `comp/forward` must not read env; and so on.
3. **`stage` = mandatory staging trigger.** `deploy/agent-merge-stage.sh` first runs the exact
   production baseline and trusted-validation gates without changing `master`, then uses a separate
   stage lock to move `stage`. A stage SHA remains frozen through degradation and explicit operator
   attestation, unless `--fix-red` is recovering a red `master` (`origin/master` `deploy/watchdog`
   must be RED). Production accepts only the same
   exact attested SHA or a valid hotfix attestation. `agent-merge.sh` refuses a `master` push
   without GREEN `deploy/stage` unless `--hotfix` skips that client check. `--hotfix` does not
   prove a host-owned hotfix record.
4. **`master` = production trigger.** Merge only via `deploy/agent-merge.sh` and only when the
   change is fully production-ready. Before the gate, the script rejects a red target and rebases
   the branch onto the latest committed `master`, then runs in parallel a fail-closed local
   path-aware gate and trusted host-validation of the exact feature SHA, reusing the credential
   from `git credential` and never offloading the token/proof onto a human. Local shell/whitespace
   checks always run; language/deployment lanes are selected by the diff, and an unknown path or a
   change to the gate enables them all. An ordinary TypeScript diff is verified over the
   dependency-aware workspace closure; shared inputs and deletions enable the full workspace, and
   the Next.js cache is reused between candidates. A pending target may overlap with these checks,
   but push is allowed only after a green re-verification under the lock. Both results are valid
   only for the unchanged SHA. The script serializes merges with a machine lock, so two production
   candidates never deploy on top of each other. The same frozen host candidate is reused after the
   push to `master`; the watchdog then performs the migration-before-app and blue-green deploy, and
   the outcome is visible in `deploy/watchdog`.
5. **A cross-component task** (for example, you changed the `Sub` contract in registry and its
   consumers): split it by owners with sequential merges OR drive it on a single task branch off
   `origin/master` with an explicit description in the commit. Direct commits to `master` are
   forbidden — merge only via `deploy/agent-merge.sh`.
6. **Synchronization:** `git fetch` before starting work. Branches are synchronized by a human; an
   agent does not merge `master` into its own branch itself — `deploy/agent-merge.sh` rebases its
   branch at merge time.
7. **Migration first:** a new append-only expand migration is added before the code that depends on
   it; never edit migration history. Full contributor/AI workflow — `CONTRIBUTING.md`.

## Typical cycle

A branch does not isolate — a worktree isolates. Several agents work on the repository in parallel,
so switching branches in a shared directory is forbidden: it rewrites files underneath your
neighbor and carries their uncommitted changes onto your branch.

```bash
worktree=$(./deploy/agent-worktree.sh create feat/forward-<task> forward-<task>)
cd "$worktree"                      # work only here from now on
# … edits strictly in crates/forward …
cargo build                          # green
git add crates/forward               # only your own paths, never git add -A
git commit                           # Conventional header + detailed body (see AGENTS.md)
git push -u origin HEAD
./deploy/agent-merge-stage.sh     # serial freeze of this exact SHA on stage
# wait for GREEN deploy/stage; operator attests this SHA
./deploy/agent-merge.sh           # same SHA to master; never manually
```

Task finished — after a green `deploy/watchdog`, the agent runs the task worktree's
`deploy/agent-worktree.sh finish <path>` from the primary clone (`"$task/deploy/agent-worktree.sh"
finish "$task"`). The script itself verifies clean+merged, fast-forwards local
`master` to `origin/master` (checkout `--ff-only` when `master` is checked out and clean; otherwise
only the ref, so a detached primary is not rewritten), re-executes GitHub's copy when the local
file is stale, and removes only the selected worktree/branch. `agent-merge.sh` already fast-forwards
that same local `master` ref after it pushes. Never
touch other agents' worktrees: for global state use the safe `doctor` and dry-run `gc`, while
`gc --apply` is left to the operator or the scheduled maintenance process. On macOS, missed cleanup
can be safely picked up by the permanent LaunchAgent `DELETE_WORKTREE`
(`docs/ops/DELETE_WORKTREE.md`).

## Creating the branches (initial setup)

```bash
for c in registry pool forward server authbot; do
  git branch comp/$c master         # branch off master
done
# then add a BRANCH.md on each (see commit history)
```

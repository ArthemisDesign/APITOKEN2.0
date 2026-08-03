# DELETE_WORKTREE

`DELETE_WORKTREE` is the local macOS cleanup agent for this repository. It prevents completed task
worktrees from retaining worktree-local Cargo `target/`, pnpm `node_modules`, and Next.js `.next`
outputs after their branch has reached `origin/master`.

## Install and lifecycle

Install it from the primary clone after the change is present in `master`:

```bash
./deploy/DELETE_WORKTREE.sh install
./deploy/DELETE_WORKTREE.sh status
```

The installer writes `~/Library/LaunchAgents/sale.apitoken.DELETE_WORKTREE.plist`, validates it with
`plutil`, bootstraps it into the current GUI session, and starts it immediately. `RunAtLoad` and
`KeepAlive` restart the agent after login, reboot, or an unexpected exit. A per-user LaunchAgent is
intentional: it operates on user-owned worktrees and reuses that user's Git and macOS Keychain
environment instead of running destructive filesystem operations as root before login.

The daemon polls every 15 seconds. A candidate must remain identical and eligible for at least two
observations and 30 seconds, so a newly merged worktree is normally reclaimed within 30–45 seconds.
State and logs live under `~/Library/Application Support/DELETE_WORKTREE/`:

```bash
tail -f "$HOME/Library/Application Support/DELETE_WORKTREE/DELETE_WORKTREE.error.log"
./deploy/DELETE_WORKTREE.sh once --dry-run
```

Use `uninstall` to stop and remove the LaunchAgent. It deliberately preserves the state directory
and standalone-clone allow-list:

```bash
./deploy/DELETE_WORKTREE.sh uninstall
```

## Worktree deletion proof

The daemon does not call global `gc --apply --grace-hours 0`. It asks the canonical
`deploy/agent-worktree.sh doctor` for clean merged candidates and then applies additional checks:

1. the exact path and branch/head fingerprint remain unchanged across the settle window;
2. `lsof` reports no process with a working directory or open file anywhere below that path;
3. no Git lock, merge, cherry-pick, revert, bisect, or rebase marker is present;
4. the final mutation is delegated to `agent-worktree.sh finish`, which takes the repository
   lifecycle lock, performs a strict fresh `fetch`, and rechecks clean status, lock state, protected
   names, branch ancestry, and the unchanged branch ref immediately before removal.

Primary/current, dirty, unmerged, detached, locked, `master`, `main`, and `comp/*` worktrees are
never candidates. An unavailable network, failed `lsof`, malformed state, or failed final check is
fail-closed: the path remains and the next pass retries. This also prevents the daemon from removing
the worktree beneath an `agent-merge.sh` process while it is still waiting for `deploy/watchdog`.

Agents must still call `agent-worktree.sh finish` themselves after a green merge. `DELETE_WORKTREE`
is a continuously running recovery net for missed cleanup, not a replacement for ownership and
attribution discipline.

## Standalone clones

Linked worktrees such as sibling `Claude_API-*` directories are discovered through Git and need no
registration. A standalone clone is more dangerous because it may contain unrelated ignored data,
so it is never discovered or deleted implicitly. Register one exact path explicitly:

```bash
./deploy/DELETE_WORKTREE.sh register-clone /absolute/path/to/clone
```

Registration requires the same normalized `origin` as the primary repository and exactly one Git
worktree. If ignored files exist, registration stops until the operator reviews them and gives
explicit whole-clone consent:

```bash
./deploy/DELETE_WORKTREE.sh register-clone /absolute/path/to/clone --allow-ignored
```

Deletion still waits for the normal settle/idle checks and additionally requires a clean status,
an empty stash, a non-detached HEAD, no in-progress Git operation, and every local branch and tag
commit to be an ancestor of fresh `origin/master`; every local tag ref must also exist unchanged on
`origin`. The path is canonicalized and revalidated
against the allow-list and repository identity immediately before the exact clone directory is
removed. Remove consent without deleting the clone with:

```bash
./deploy/DELETE_WORKTREE.sh unregister-clone /absolute/path/to/clone
```

`DELETE_WORKTREE` does not clean browser caches, Xcode DerivedData, package-manager caches, the
shared bounded 10 GiB `sccache`, or arbitrary repositories. Those have different ownership and
retention rules and must not be inferred from worktree eligibility.

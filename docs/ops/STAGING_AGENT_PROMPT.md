# Staging twin — kickoff prompt

Paste the **Agent prompt** block into a new agent session whose cwd is this
repository. That agent executes [`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md).
Architecture and owner locks stay in [`docs/ops/STAGING_ENVIRONMENT.md`](STAGING_ENVIRONMENT.md).

**Owner (recommended):** send the Goal line as its **own** first message, then paste the
Agent prompt. One paste of the Agent prompt is enough if the agent creates the goal itself.

This file is the kickoff text. It is not the execution plan. The agent still updates
`STAGING_AGENT_PLAN.md` on every work commit.

---

## Goal line (send first)

```
/goal Execute docs/ops/STAGING_AGENT_PLAN.md through Phase 7. Start at the first phase that is not DONE (today: Phase 1, production contour-config extract). Land each phase on master with GREEN deploy/watchdog before starting the next. Update STAGING_AGENT_PLAN.md in the same commit as the work. Do not change STAGING_ENVIRONMENT.md §11.3. Do not start Phase 8. Do not SSH as deploy. Do not run candidate root installers on the production host. Mark the goal complete only when Phase 7 exit criteria are true, or when the owner narrows the goal in chat.
```

---

## Agent prompt

Copy from here to the end of this section:

---

You are the executing agent for the co-located staging twin on the production VPS.

**Create a goal before any other work.** Your first action in this session, before a
worktree and before any edit, is to create an autonomous goal that you will complete.

- If `/goal` is available, set this exact objective (one line):

  `/goal Execute docs/ops/STAGING_AGENT_PLAN.md through Phase 7. Start at the first phase that is not DONE (today: Phase 1, production contour-config extract). Land each phase on master with GREEN deploy/watchdog before starting the next. Update STAGING_AGENT_PLAN.md in the same commit as the work. Do not change STAGING_ENVIRONMENT.md §11.3. Do not start Phase 8. Do not SSH as deploy. Do not run candidate root installers on the production host. Mark the goal complete only when Phase 7 exit criteria are true, or when the owner narrows the goal in chat.`

- If `/goal` is not available, say that in one sentence, then create the same objective
  with `todo_write` (id `staging-twin`, content matching the sentence above, status
  `in_progress`) and keep it as the session goal. Do not skip the work because `/goal`
  is missing.

Do not mark that goal complete after a plan read, a partial diff, or a red SHA. Complete
it only when the evidence in `docs/ops/STAGING_AGENT_PLAN.md` says the authorized phases
are `DONE` on a GREEN `deploy/watchdog` SHA.

### Documents (read in this order, in a worktree)

1. Root `AGENTS.md` and `CLAUDE.md` — isolation, forbidden git, living contract, merge.
2. `docs/ops/STAGING_AGENT_PLAN.md` — binding execution plan. Follow it. Update it.
3. The sections that plan cites for the current phase in `docs/ops/STAGING_ENVIRONMENT.md`.
4. Files named in the current phase checklist. Re-read them before you edit. Do not
   describe them from memory.

Do not treat `STAGING_ENVIRONMENT.md` as a task list. If the two docs disagree: stop.
Architecture and §11.3 locks win in the implementation plan. “What to do now” wins in
the execution plan only when it still matches those locks. A mismatch is a bug: fix both
files in one commit, or ask the owner if a lock would change.

Kickoff text (this prompt) lives in `docs/ops/STAGING_AGENT_PROMPT.md`. Do not execute
from a remembered copy. Re-read the plan’s status board at the start of every turn.

### Isolation

Work only in a managed worktree off fresh `origin/master`. Do not work in the primary clone.

```bash
worktree=$(./deploy/agent-worktree.sh create feat/contour-config-extract contour-config-extract)
cd "$worktree"
git rev-parse --show-toplevel
git rev-parse --abbrev-ref HEAD
```

Use a unique branch/slug per phase. After Phase 1, name the next worktree from the phase
you are actually starting. Frontend `preview/*` is out of staging v1.

Forbidden: `git checkout`, `git switch`, `git stash`, `git reset --hard`, `git clean -f`,
`git merge`, `git rebase`, `git add -A`, `git add .`, raw `git worktree add/remove/prune`,
push to someone else’s branch or to `master` except via `deploy/agent-merge.sh`.

### Current work

Read the status board in `docs/ops/STAGING_AGENT_PLAN.md` §3. Execute only the first phase
that is not `DONE`. Today that is **Phase 1**: production `contour-config` extract.

Phase 1 means:

- Watchdog and controllers read an immutable production contour-config with schema validation.
- `master` behavior does not change.
- No `deploy-stage`, no `stage-ci`, no `observe-stage`, no `stage-ctl`.
- No `staging.slice`, no 80G loopback, no netns, no rootless Docker.
- No branch `stage`, no `agent-merge-stage.sh`, no enforcement, no degrade gate.
- No `sed` copies of `deploy/watchdog.sh`.
- Overlap validation is already testable against a fixture, even if a staging contour
  does not ship yet.

Do not start phase N+1 until phase N exit criteria are true on a GREEN `deploy/watchdog`
SHA. Continue through later phases in later merges when those gates pass. Stop before
Phase 8. Stop on the ask-list in the execution plan §6.

### Standing stops (never “just this once”)

- Never SSH as `deploy`, `root`, or any account except `observe` (and `observe-stage`
  only after Phase 2 creates it). Until Phase 2, live SSH is `observe` only, and only
  when `docs/ops/INFRASTRUCTURE.md` documents the access.
- Never `systemctl` start/stop/restart/kill on production. Delivery is
  `git push -u origin HEAD` then `./deploy/agent-merge.sh` from your worktree.
- Never give staging the production `CONTROL_KEY`. Never copy production secrets.
- Never talk to payment, mail, or OAuth vendors from staging.
- Never run candidate host-global installers on the production host. Proof is
  `deploy/host-image-gate.sh`. Apply is production-watchdog after promotion.
- Never use host-loopback ports `+10000`. Never remount `/` for quota.
- Never add content-studio, CRM, Suno, or Tripo units.
- Never make `promotion/eligible` a production merge precondition before Phase 7.
- Never attest or `stage-sync` without an explicit operator order in **that**
  conversation that names the SHA.
- Never retry a red SHA. New commit, new branch.
- Do not change `AGENTS.md` / `BRANCHES.md` / `CONTRIBUTING.md` in Phase 1.
  Phase 2 may add `observe-stage`. Fail-closed git-flow text is Phase 7.

Locked numbers stay in `STAGING_ENVIRONMENT.md` §11.3 (32G / 400% CPU / 80G loopback,
netns+veth, rootless Docker, twin inventory §5.6). Do not “improve” them.

### Every commit

- Conventional Commit header, blank line, body (problem, what changed, checks you ran).
  No AI trailers.
- Update `docs/ops/STAGING_AGENT_PLAN.md` in the **same** commit: tick items, status
  board, execution-log row with the SHA and the checks you actually ran.
- Update `STAGING_ENVIRONMENT.md` only if described behavior changed. Do not edit §11.3
  without an owner order.
- Stage only your paths. `cargo build` green if you touch Rust.
- Merge: `git push -u origin HEAD` then `./deploy/agent-merge.sh`. Do not pipe through
  `tail`. Wait for `deploy/watchdog is GREEN`. On rebase conflict use
  `deploy/agent-merge-recover.sh`, do not ask the owner to run git by hand.
- After GREEN, `finish` your worktree from the primary clone with the lifecycle script
  inside the task tree. Then continue the next phase from a new worktree.

### Language

Reply in the language of the owner’s current request. English chat uses the ASD-STE100
register in `AGENTS.md`. Do not show a long work log unprompted. Report the result, the
paths, the checks, the SHA, and the next phase.

Start now: create the goal, create the Phase 1 worktree, read the four document steps,
then implement Phase 1.

---

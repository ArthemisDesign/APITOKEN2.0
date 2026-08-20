# Gemini Batch Stage 5 controlled canary

`tools/gemini_batch/run_live.py` is the credential-safe operator runner for the controlled Stage 5
Batch evidence required by `docs/engine/GEMINI_BATCH_MODE_PLAN.md`. It is **dry-run by default** and
must not be used to enable or publish Batch. This runbook does not authorize a live invocation; the
operator must separately choose to execute it after reviewing the exact production state.

## Safety contract

- `--execute` requires an exact 40-character implementation SHA and an explicit integer
  `--previous-spend-nanousd` checkpoint. The runner reads the active immutable production release and
  refuses a mismatch. The SHA must already have GREEN `deploy/watchdog`; the operator verifies that
  status before invoking the tool. The tool cannot substitute a mutable branch name or local HEAD.
- The original Stage 5 + Stage 6 authorization is exactly `10_000_000_000 nanoUSD` ($10). All money
  parsing and arithmetic is integer-only. `previous spend + current settlements + newly observed
  server-authoritative holds` must fit the remaining aggregate budget.
- Credentials are loaded only inside the production SSH session from
  `/srv/claude-api/data/server.env`. The dedicated test key is
  `GEMINI_BATCH_STAGE5_API_KEY`; the panel key and engine PostgreSQL URL stay remote as well. No key,
  environment value, prompt, result, ciphertext, provider subject, email, proxy, or database URL is
  returned to the workstation.
- Before each paid create, the remote helper performs one free `countTokens` call for every exact
  item. It also attempts a create dry preflight. An explicit unsupported response is recorded as
  `unavailable`; any other dry-preflight failure stops before paid create.
- A process invocation has **one paid create attempt**. The create transport is never retried. A
  timeout, SSH loss, missing response envelope, or malformed response after dispatch is
  `ambiguous-nonresumable`: preserve the checkpoint, investigate the authority, and never replay that
  invocation.
- Immediately after a successful create, the same remote process reads a secret-free PostgreSQL
  diagnostic projection containing only batch/item identifiers and integer holds. Missing or
  incomplete holds fail the run before any further action. The query never selects encrypted
  request/result blobs, raw keys, metadata, or customer content.
- The local checkpoint is immutable across invocations. It contains only sanitized batch/item IDs,
  holds, settlement nanoUSD, terminal classes, and opaque profile IDs. It contains no request body or
  result body.

## Scenarios

The dry-run plan records the complete controlled matrix:

1. **distribution-two-items** — two independent items; terminal evidence must show distribution over
   at least two eligible opaque profiles when the fleet provides them;
2. **cancel** — create, cancel, and confirm per-item hold release/settlement;
3. **restart-safe-boundary** — observe progress across the reviewed Gemini blue-green replacement
   boundary, never by manually killing or deploying over SSH;
4. **headroom-no-paid** — observation only; prove queued/no-dispatch behavior while the 5-hour gate is
   at or below the configured floor. This scenario performs no paid create;
5. **ordinary-parity** — compare the same bounded request through ordinary `generateContent`, with no
   Batch discount and the same tariff/multiplier authority.

Because one invocation permits one paid create, paid scenarios are run as separate immutable
checkpoints. Pass the cumulative settled spend from all earlier Stage 5 runs as the next invocation's
`--previous-spend-nanousd`. Do not infer spend from holds; journal exact settlement facts.

## Operator procedure

First generate and review a network-free plan:

```bash
python3 tools/gemini_batch/run_live.py
python3 -m unittest tools.gemini_batch.test_run_live
```

Before an authorized execution:

1. Confirm the exact implementation SHA is the active production release and its `deploy/watchdog`
   status is GREEN.
2. Reconcile every prior Stage 5/6 settlement and the append-only journal. The current baseline in
   `docs/engine/GEMINI_BATCH_MODE_JOURNAL.md` is zero until a later append-only entry says otherwise.
3. Confirm the dedicated remote test account has sufficient funds, the Batch runtime is default-off
   from public discovery, diagnostic authority is healthy, and the scenario's preconditions are
   intentionally established.
4. Choose a new checkpoint path. Never pass an existing path and never delete a terminal ambiguous
   checkpoint to make it reusable.
5. Only after explicit operator authorization, invoke the runner with the exact values. Do not copy a
   credential into the command or local environment.

Example shape (placeholders only; this runbook does not authorize execution):

```bash
python3 tools/gemini_batch/run_live.py \
  --execute \
  --implementation-sha <exact-production-green-40-char-sha> \
  --previous-spend-nanousd <exact-cumulative-integer> \
  --checkpoint /tmp/gemini-batch-stage5-<unique-run>.json
```

The runner stops after the first paid scenario in its fixed ordering. Subsequent paid scenarios
require a reviewed follow-up invocation and updated checkpoint/spend input. The headroom observation
can be assessed from the sanitized diagnostics without a paid create.

## Evidence and failure handling

After each invocation, append a new entry to `docs/engine/GEMINI_BATCH_MODE_JOURNAL.md` with:

- exact implementation SHA and GREEN production status;
- scenario and sanitized batch/item IDs;
- per-item holds, exact settlements, remaining aggregate budget, and opaque profile IDs;
- terminal state, cancellation/restart/headroom behavior, and ordinary parity conclusion;
- any deviation or ambiguity.

Never paste prompts, generated text, raw HTTP payloads, keys, database URLs, profile subjects, emails,
or encrypted blobs into the journal. A paid ambiguity is terminal even when settlement is not yet
visible. Investigate read-only authority and reconciliation evidence; do not issue another create for
that scenario until root cause is known and a distinct, explicitly reviewed run is budgeted. No
execute/live command is part of repository verification.

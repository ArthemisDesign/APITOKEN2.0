# GPT Image 2 private dry-run planner

This is an internal, locked planning surface for the dormant APIYI-hosted OpenAI Images candidate. It
is not a customer route, runtime provider, catalog entry, router preset, live canary, or publication
authorization. The binary exposes the private `openai-image-canary` subcommand only as a dry-run
planner; `--execute` is intentionally retained but always blocked.

## Local validation contract

Before printing a plan or returning the live-execution block, the command validates all local inputs:

- `--prompt-file` is a regular, non-symlink Unix file with exact mode `0600`, unchanged between path
  inspection and open. It must contain nonempty UTF-8 of at most 512 bytes and 512 Unicode characters.
- `--output` ends in `.png` and `--checkpoint` ends in `.json`. Neither target may exist or be a
  symlink. Each parent must already be an actual non-symlink directory; the checkpoint basename must
  be valid UTF-8.
- Only the reviewed alias `gpt-image-2` is accepted.
- `--budget-nanousd` is a proposed future budget, not an admission hold. It must be at least the
  official-list estimate `prompt_utf8_bytes * fresh_text_input_rate + 196 * image_output_rate`, using
  the immutable metering tariff. The estimate does not prove a request maximum, APIYI's actual charge,
  or its group multiplier.

Dry-run reads no environment or API key, creates no output or checkpoint, and performs no network
request. It prints one privacy-safe JSON plan with `state: "blocked"`, `executable: false`,
`implementation_sha: null`, the four blockers below, tariff identity, proposed budget, official-list
estimate, prompt counts, and timestamp. It never serializes prompt text, paths, or credentials.

```bash
cargo run -p claude-api -- openai-image-canary \
  --prompt-file /private/canary/prompt.txt \
  --output /private/canary/result.png \
  --checkpoint /private/canary/checkpoint.json \
  --budget-nanousd 8440000
```

`implementation_sha: null` explicitly means the plan is not live evidence tied to a reviewed build.
Passing `--execute` performs the same complete local validation, then returns only
`GPT Image 2 live execution is blocked`; it still reads no env/key, creates no file, and makes no
network request.

## Live blockers

The planner reports four closed blockers:

1. `no_free_preflight`: the Images surface has no reviewed free `countTokens` equivalent. A paid
   attempt cannot skip the repository admission sequence; it requires a separate reviewed,
   image-specific free-preflight exception or an actual free preflight.
2. `spend_above_default_cap`: even the minimum official-list estimate is about `$0.005885`, above the
   repository's default aggregate `$0.0001` admission cap. A specific larger spend must be explicitly
   authorized before any paid attempt.
3. `no_exact_green_sha`: no exact clean implementation SHA has completed the required gate and been
   designated for a controlled live attempt.
4. `reserve_ceiling_unproved`: the official-list estimate is not a conservative request-level ceiling,
   and APIYI's actual charged amount/group multiplier remains unproved.

A future live-capable command requires a separate reviewed change that closes all four blockers and
establishes owned credential handling, non-duplicating attempt semantics, and privacy-safe evidence.
Provisioning `CLAUDE_API_APIYI_IMAGE_API_KEY` before that change is prohibited; the current binary does
not read it anywhere.

Publication remains a separate later change and must satisfy the repository model gate in full:
successful generation with real output, terminal authoritative usage, incremental SSE, and every
advertised control on the exact implementation SHA. The Images documentation contradiction does not
authorize a streaming exception. See `research/GPT_IMAGE_2_EVIDENCE.md`.

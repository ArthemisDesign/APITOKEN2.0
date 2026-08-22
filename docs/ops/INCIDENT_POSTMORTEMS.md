# Incident postmortems and executable guardrails

Use this reference for a production incident or near miss that crossed an existing prevention or validation layer. Broad code/test/process reviews remain dated append-only audits in `docs/audits/`; an incident postmortem explains one causal chain and the permanent guardrail that prevents its recurrence.

## When to write one

Write a postmortem when at least one condition holds:

- production availability, customer money, authentication, privacy, or a durable contract was affected;
- a credible near miss crossed the same safeguards and could have caused that impact;
- the defect passed the checks that were expected to catch it;
- the root cause is a reusable lifecycle, concurrency, integration, or delivery failure class.

Do not create a postmortem for ordinary local defects caught before merge. Keep those in the commit and regression test.

## Placement and lifecycle

Create a new append-only snapshot at `docs/audits/YYYY-MM-DD-<INCIDENT>.md` and add it to `docs/README.md`. Never rewrite an older incident to describe a later recurrence. A follow-up incident gets its own dated file and links the earlier one.

An open incident can state a missing guardrail. A resolved incident must link at least one executable test, checker, monitor, or fail-closed runtime assertion that reproduces the causal failure rather than only the symptom. A prose rule, manual checklist, retry, muted warning, or screenshot is not an executable guardrail.

## Required template

```markdown
# <Incident> — YYYY-MM-DD

Status: investigating | mitigated | resolved

## Executive summary

One paragraph: user/production impact, duration or exposure, and final state.

## Impact and detection

What external or authoritative state changed. How the incident was first detected. Separate measured facts from estimates.

## Timeline

Only evidence-bearing events needed to establish causality. Use absolute UTC timestamps when timing matters.

## Root cause

The complete causal chain from admitted input/change through the failed ownership or lifecycle boundary to the impact. Name the decision point that produced the wrong result.

## Why existing safeguards missed it

Name the exact tests, validation lanes, monitoring, or review assumptions that stayed green and why they could not observe this failure.

## Correction

The production correction and any limits it retains. Do not present a retry or hidden fallback as the root fix.

## Executable guardrail

Relative links to the tests/checkers/alerts/assertions added or changed. State the seeded regression each guardrail rejects and the command or gate that executes it.

## Remaining risk

Known unproved cases, external dependencies, or follow-up work. Use `none` only when every material item is closed.
```

## Review standard

A resolved postmortem is incomplete if a reviewer cannot answer all of these from the file and linked code:

1. What authoritative or user-visible outcome was wrong?
2. Which exact decision or ownership boundary caused it?
3. Why did the previous gate stay green?
4. Which executable guardrail turns red when the cause is seeded again?
5. Where does that guardrail run for every relevant future change?
6. What risk remains outside that proof?

When one incident yields a rule that applies in at least two current places, add the short present-tense rule to the owning architecture, testing, or deployment document and link the incident as rationale. Do not grow `AGENTS.md` with incident narrative.

# Vercel deployments (apitoken.sale)

Operational runbook for the `apps/web` frontend deployments. Project setup (Root Directory,
framework preset, env, domains) is in `docs/ops/INFRASTRUCTURE.md` ("Vercel frontend"); analytics —
in `docs/ops/VERCEL_PRODUCT_ANALYTICS.md`. This document covers the deployment lifecycle: what
triggers a build, how to read deployment state without Vercel access, and the failure signatures.

## Deployment model

- The Vercel project builds `apps/web` from this repository through the GitHub integration:
  every push to `master` creates a **Production** deployment that promotes `apitoken.sale` on
  success, and every push to a `preview/<task-slug>` branch creates a **Preview** deployment for
  human review (Branch Tracking — `docs/ops/INFRASTRUCTURE.md`; the agent naming and approval
  contract — `AGENTS.md` and `apps/web/README.md`). Pushes to ordinary task branches intentionally
  create no deployment.
- Vercel deploys **independently of the host watchdog**: `deploy/agent-merge.sh` holds the
  merge-lock only until `deploy/watchdog` is green and deliberately does not trust the combined
  commit status (see `docs/ops/DEVBOT.md`). A green merge therefore says nothing about the
  frontend — Vercel state must be checked separately.
- `apps/web/vercel.json` runs `apps/web/scripts/vercel-ignore-build.sh` as the ignored-build step
  (contract and shallow-history recovery — `apps/web/README.md`). Exit 0 skips the build (commit
  status `Skipped - Not affected`, a success); exit 1 builds; the script fails closed to a normal
  build on any doubt. A skipped or failed **Production** deployment leaves the live site unchanged,
  so the site is only as fresh as the last completed Production deployment.

## Reading deployment state without Vercel access

Vercel posts both GitHub commit statuses and GitHub Deployments for every build. Both are readable
with the same credential `deploy/agent-merge.sh` uses for `deploy/watchdog` — no Vercel login or
token is needed for triage:

```bash
TOKEN=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill | awk -F= '/^password=/{print $2}')
# commit statuses (pending → success/failure per build, with descriptions):
curl -sS -H "Authorization: Bearer $TOKEN" \
  "https://api.github.com/repos/3xcalibur-tech/Claude_API/commits/<sha>/statuses?per_page=100"
# deployments (environment Production/Preview; statuses name the exact dpl_<id>):
curl -sS -H "Authorization: Bearer $TOKEN" \
  "https://api.github.com/repos/3xcalibur-tech/Claude_API/deployments?sha=<sha>"
```

Deployment-record timestamps mark when the build concluded, not when it started; compare against
the push/merge time, and use the commit-status `pending → terminal` pairs for durations.

Live-site freshness probes:

```bash
curl -sSI https://apitoken.sale/ | grep -i -E '^(age|x-vercel-cache)'   # age of the cached copy
curl -sS -o /dev/null -w '%{http_code}\n' https://apitoken.sale/<page-only-in-newer-master>
```

A 404 on a page that exists in `master` means the live Production deployment predates that commit.

## Failure signatures

- **`pending → success` in tens of seconds** — a real build; the deployment URL serves the new tree.
- **`Skipped - Not affected`** — a clean ignore-step skip; the live site intentionally stays as is.
- **`pending → failure` within a few seconds** — admission-time failure before install/build. The
  code is exonerated (a Preview of the same tree and a local `pnpm --filter @claude-api/web build`
  both build it), so look at Production-scoped project state: an environment variable referencing a
  deleted secret, Production Overrides (build/install command, Root Directory, Node version), plan
  or concurrency limits. Precedent: on 2026-08-06 every Production deployment died in seconds
  because the previous inline ignore command compared against a `VERCEL_GIT_PREVIOUS_SHA` that had
  aged out of Vercel's depth-limited clone (git exit 128); `e36f1a64` hardened the step into
  `scripts/vercel-ignore-build.sh` with exact-SHA fetch and fail-closed-to-build semantics.
- **Site frozen while merges are green** — check the signatures above per commit; the watchdog does
  not cover Vercel, so nothing else will page you.

The exact failing step is only visible in the Vercel build log:

```bash
npx vercel inspect <dpl-id> --logs   # dpl-id comes from the deployment status description
```

## Access

Vercel dashboard/CLI access to the hosting team is operator-held and is **not** provisioned to
agents; per the standing rule, an access method that is not in the infra docs does not exist for
agents. Agent duty: triage through the GitHub API above and escalate with the exact `dpl_<id>` and
the observed signature — never guess or hunt for Vercel credentials. If access is provisioned
later, record it in `docs/ops/INFRASTRUCTURE.md` in the same change.

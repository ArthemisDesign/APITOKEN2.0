import { describe, expect, it, vi } from "vitest";
import { EngineClient } from "./index.js";

function runtime() {
  return {
    observed_at: 10,
    process_started_at: 1,
    continuity: "process_local",
    queue_capacity: 4096,
    queue_depth: 0,
    accepted_total: 1,
    persisted_total: 1,
    deduplicated_total: 0,
    dropped_invalid_total: 0,
    dropped_full_total: 0,
    dropped_closed_total: 0,
    dropped_unsupported_total: 0,
    persistence_failed_total: 0,
    persistence_health: "healthy",
    stuck_nonterminal_count: 0,
  };
}

function coverage() {
  return {
    scope_version: 1,
    from: 1,
    to: 2,
    persisted_facts: 1,
    terminal_facts: 1,
    nonterminal_facts: 0,
    required_evidence_unknown_facts: 0,
    drops: { value: null, reason: "no_durable_window_attribution" },
    persistence_failures: { value: null, reason: "no_durable_window_attribution" },
    admitted_denominator: null,
    coverage_percentage: null,
    status: "unknown",
  };
}

describe("private request analytics", () => {
  it("builds bounded summary and page URLs with the control key", async () => {
    const calls: Array<{ url: string; key: string | null }> = [];
    const fetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, key: new Headers(init?.headers).get("x-api-key") });
      if (url.includes("summary")) {
        const empty = { groups: [], truncated: false };
        return new Response(JSON.stringify({
          scope_version: 1, from: 1, to: 2,
          summary: { totals: { persisted: 1, terminal: 1, nonterminal: 0, required_evidence_unknown: 0 }, clients: empty, routes: empty, requested_models: empty, executable_models: empty, terminal_classes: empty, delivery_states: empty, billing_outcomes: empty },
          coverage: coverage(), runtime: runtime(),
        }), { status: 200, headers: { "content-type": "application/json" } });
      }
      return new Response(JSON.stringify({ scope_version: 1, from: 1, to: 2, rows: [], next_cursor: null, coverage: coverage(), runtime: runtime() }), { status: 200, headers: { "content-type": "application/json" } });
    });
    const engine = new EngineClient({ baseUrl: "https://engine.test", controlKey: "control", fetch: fetch as typeof globalThis.fetch });
    await engine.getRequestFactSummary({ from: 1, to: 2, accountId: "acct_1" });
    await engine.listRequestFacts({ from: 1, to: 2, limit: 10, cursor: "abc" });
    expect(calls).toEqual([
      { url: "https://engine.test/admin/request-facts/summary?from=1&to=2&account_id=acct_1", key: "control" },
      { url: "https://engine.test/admin/request-facts?from=1&to=2&cursor=abc&limit=10", key: "control" },
    ]);
  });

  it("rejects invalid windows, limits and logical IDs before fetch", async () => {
    const fetch = vi.fn();
    const engine = new EngineClient({ baseUrl: "https://engine.test", controlKey: "control", fetch: fetch as typeof globalThis.fetch });
    await expect(engine.getRequestFactSummary({ from: 2, to: 2 })).rejects.toBeInstanceOf(RangeError);
    await expect(engine.listRequestFacts({ from: 1, to: 2, limit: 201 })).rejects.toBeInstanceOf(RangeError);
    await expect(engine.getRequestFactsByLogicalId("NOT-A-UUID")).rejects.toBeInstanceOf(RangeError);
    expect(fetch).not.toHaveBeenCalled();
  });
});

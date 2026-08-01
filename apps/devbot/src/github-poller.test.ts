import { describe, expect, it } from "vitest";
import { diffSnapshots } from "./github-poller.js";
import type { GithubSnapshot } from "./state.js";

const SHA1 = "1bd14c3deadbeef1";
const SHA2 = "2bd14c3deadbeef2";

function snapshot(overrides: Partial<GithubSnapshot> = {}): GithubSnapshot {
  return {
    sha: SHA1,
    title: "feat: something",
    statuses: {},
    deployments: {},
    ...overrides,
  };
}

describe("github diffSnapshots", () => {
  it("emits nothing on the first (baseline) snapshot", () => {
    expect(diffSnapshots(null, snapshot())).toEqual([]);
  });

  it("emits new-sha with the current phase states on HEAD change", () => {
    const prev = snapshot();
    const next = snapshot({ sha: SHA2, statuses: { "deploy/tests": "pending" } });
    const events = diffSnapshots(prev, next);
    expect(events[0]).toEqual({ kind: "new-sha", sha: SHA2, title: "feat: something" });
    expect(events).toContainEqual({ kind: "phase", sha: SHA2, phase: "tests", state: "pending" });
  });

  it("emits phase transitions only on state change", () => {
    const prev = snapshot({ statuses: { "deploy/tests": "pending" } });
    const same = snapshot({ statuses: { "deploy/tests": "pending" } });
    expect(diffSnapshots(prev, same)).toEqual([]);
    const progressed = snapshot({ statuses: { "deploy/tests": "success" } });
    expect(diffSnapshots(prev, progressed)).toEqual([
      { kind: "phase", sha: SHA1, phase: "tests", state: "success" },
    ]);
  });

  it("maps tests/migration failure to quarantine (critical)", () => {
    const prev = snapshot({ statuses: { "deploy/tests": "pending" } });
    const failed = snapshot({ statuses: { "deploy/tests": "failure" } });
    const events = diffSnapshots(prev, failed);
    expect(events).toContainEqual({ kind: "phase", sha: SHA1, phase: "tests", state: "failure" });
    expect(events).toContainEqual({ kind: "quarantine", sha: SHA1, phase: "tests" });
  });

  it("maps watchdog success to green and watchdog failure to quarantine", () => {
    const prev = snapshot({ statuses: { "deploy/watchdog": "pending" } });
    expect(diffSnapshots(prev, snapshot({ statuses: { "deploy/watchdog": "success" } })))
      .toEqual([{ kind: "green", sha: SHA1 }]);
    expect(diffSnapshots(prev, snapshot({ statuses: { "deploy/watchdog": "failure" } })))
      .toEqual([{ kind: "quarantine", sha: SHA1 }]);
  });

  it("maps production deployments to deploy phases and candidate-validation to CI", () => {
    const prev = snapshot();
    const next = snapshot({
      deployments: {
        "production-engine": { id: 1, state: "success", sha: SHA1 },
        "candidate-validation": { id: 2, state: "failure" },
      },
    });
    const events = diffSnapshots(prev, next);
    expect(events).toContainEqual({ kind: "phase", sha: SHA1, phase: "engine", state: "success" });
    expect(events).toContainEqual({ kind: "ci", environment: "candidate-validation", state: "failure" });
  });

  it("ignores deployments of a different SHA (no premature phase paint)", () => {
    const prev = snapshot();
    const next = snapshot({
      deployments: { "production-engine": { id: 1, state: "success", sha: SHA2 } },
    });
    expect(diffSnapshots(prev, next)).toEqual([]);
  });

  it("does not re-emit an unchanged deployment", () => {
    const deployments = { "production-engine": { id: 1, state: "success" as const, sha: SHA1 } };
    const prev = snapshot({ deployments });
    const next = snapshot({ deployments });
    expect(diffSnapshots(prev, next)).toEqual([]);
  });
});

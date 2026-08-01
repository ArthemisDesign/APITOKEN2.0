import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { diffSnapshots, GithubPoller } from "./github-poller.js";
import { StateStore, type GithubSnapshot } from "./state.js";
import type { DeployEvent } from "./events.js";

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

  it("carries the commit author through the new-sha event when present", () => {
    const prev = snapshot();
    const next = snapshot({ sha: SHA2, author: "qqjamba" });
    expect(diffSnapshots(prev, next)[0]).toEqual({
      kind: "new-sha",
      sha: SHA2,
      title: "feat: something",
      author: "qqjamba",
    });
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

  it("maps every deploy component status, including no-change and devbot verdicts", () => {
    const contexts = {
      "deploy/tests": "success",
      "deploy/migration": "success",
      "deploy/engine": "success",
      "deploy/backend": "success",
      "deploy/sales": "success",
      "deploy/openkeys": "success",
      "deploy/admin": "success",
      "deploy/devbot": "success",
    } as const;
    const events = diffSnapshots(snapshot(), snapshot({ statuses: contexts }));
    expect(events.filter((event) => event.kind === "phase").map((event) => event.phase)).toEqual([
      "tests", "migration", "engine", "backend", "sales", "openkeys", "admin", "devbot",
    ]);
  });

  it("renders failed phases before one terminal quarantine even when watchdog is newest", () => {
    const prev = snapshot({ statuses: { "deploy/watchdog": "pending", "deploy/tests": "pending" } });
    const next = snapshot({ statuses: { "deploy/watchdog": "failure", "deploy/tests": "failure" } });
    expect(diffSnapshots(prev, next)).toEqual([
      { kind: "phase", sha: SHA1, phase: "tests", state: "failure" },
      { kind: "quarantine", sha: SHA1, phase: "tests" },
    ]);
  });

  it("maps production deployments to deploy phases and candidate-validation to CI", () => {
    const prev = snapshot();
    const next = snapshot({
      deployments: {
        "production-engine": { id: 1, state: "success", sha: SHA1 },
        "candidate-validation": { id: 2, state: "failure" },
        "production-devbot": { id: 3, state: "pending", sha: SHA1 },
      },
    });
    const events = diffSnapshots(prev, next);
    expect(events).toContainEqual({ kind: "phase", sha: SHA1, phase: "engine", state: "success" });
    expect(events).toContainEqual({ kind: "phase", sha: SHA1, phase: "devbot", state: "pending" });
    expect(events).toContainEqual({ kind: "ci", environment: "candidate-validation", state: "failure" });
  });

  it("uses commit status as authority over an older deployment state", () => {
    const prev = snapshot({
      statuses: { "deploy/engine": "pending" },
      deployments: { "production-engine": { id: 1, state: "pending", sha: SHA1 } },
    });
    const next = snapshot({
      statuses: { "deploy/engine": "success" },
      deployments: { "production-engine": { id: 2, state: "pending", sha: SHA1 } },
    });
    expect(diffSnapshots(prev, next)).toEqual([
      { kind: "phase", sha: SHA1, phase: "engine", state: "success" },
    ]);
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


/**
 * Мок GitHub API по суффиксу URL: /commits/master, /commits/{sha}/statuses?…, /deployments?… .
 * Возвращает 404 на неизвестный путь — poller тогда бросает, и тест это увидит.
 */
function ghMock(routes: Record<string, unknown>, calls: string[] = []): typeof fetch {
  return (async (input: unknown) => {
    const url = String(input);
    calls.push(url);
    for (const [suffix, payload] of Object.entries(routes)) {
      if (url.endsWith(suffix)) return new Response(JSON.stringify(payload), { status: 200 });
    }
    return new Response(JSON.stringify({ message: "not found" }), { status: 404 });
  }) as typeof fetch;
}

function commitPayload(sha: string, message: string) {
  return { sha, commit: { message, author: { name: "agent" } }, author: null };
}

describe("GithubPoller tail polling", () => {
  async function makePoller(
    routes: Record<string, unknown>,
    calls: string[] = [],
    handleEvents?: (events: DeployEvent[]) => void | Promise<void>,
  ) {
    const dir = await mkdtemp(path.join(tmpdir(), "devbot-poller-"));
    const state = new StateStore(path.join(dir, "state.json"));
    await state.load();
    const events: DeployEvent[] = [];
    const poller = new GithubPoller({
      token: "x",
      repo: "acme/repo",
      state,
      onEvents: handleEvents ?? ((batch) => {
        events.push(...batch);
      }),
      fetchFn: ghMock(routes, calls),
    });
    return { poller, state, events };
  }

  it("awaits asynchronous event routing before a poll completes", async () => {
    const routes: Record<string, unknown> = {
      "/commits/master": commitPayload(SHA1, "feat: one"),
      [`/commits/${SHA1}/statuses?per_page=100`]: [{ context: "deploy/watchdog", state: "pending" }],
      [`/commits/${SHA2}/statuses?per_page=100`]: [{ context: "deploy/watchdog", state: "success" }],
      "/deployments?per_page=30": [],
    };
    let routed = false;
    const { poller } = await makePoller(routes, [], async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
      routed = true;
    });
    await poller.pollOnce();
    routes["/commits/master"] = commitPayload(SHA2, "feat: two");
    await poller.pollOnce();
    expect(routed).toBe(true);
  });

  it("keeps polling the previous SHA after HEAD moves and emits its green exactly once", async () => {
    const calls: string[] = [];
    const routes: Record<string, unknown> = {
      "/commits/master": commitPayload(SHA1, "feat: one"),
      [`/commits/${SHA1}/statuses?per_page=100`]: [
        { context: "deploy/tests", state: "success" },
        { context: "deploy/watchdog", state: "pending" },
      ],
      "/deployments?per_page=30": [],
    };
    const { poller, state, events } = await makePoller(routes, calls);

    // База: SHA1 в полёте, событий нет.
    await poller.pollOnce();
    expect(events).toEqual([]);
    expect(state.data.github?.sha).toBe(SHA1);
    expect(state.data.github?.tail).toBeUndefined();

    // HEAD уходит на SHA2, а SHA1 за это время доезжает до зелёного — финал SHA1
    // больше не теряется: приходит и new-sha(SHA2), и green(SHA1) из tail'а.
    routes["/commits/master"] = commitPayload(SHA2, "feat: two");
    routes[`/commits/${SHA2}/statuses?per_page=100`] = [{ context: "deploy/watchdog", state: "pending" }];
    routes[`/commits/${SHA1}/statuses?per_page=100`] = [
      { context: "deploy/tests", state: "success" },
      { context: "deploy/watchdog", state: "success" },
    ];
    await poller.pollOnce();
    expect(events).toContainEqual(expect.objectContaining({ kind: "new-sha", sha: SHA2 }));
    expect(events).toContainEqual({ kind: "green", sha: SHA1 });
    // Терминал → хвост снят.
    expect(state.data.github?.tail).toBeUndefined();

    // Дальше хвост не опрашивается и green не дублируется.
    const greenCount = events.filter((event) => event.kind === "green" && event.sha === SHA1).length;
    await poller.pollOnce();
    expect(events.filter((event) => event.kind === "green" && event.sha === SHA1)).toHaveLength(greenCount);
    expect(calls.filter((url) => url.includes(`/commits/${SHA1}/statuses`)).length <= 2).toBe(true);
  });

  it("keeps the tail across polls while the previous pipeline is still pending", async () => {
    const routes: Record<string, unknown> = {
      "/commits/master": commitPayload(SHA1, "feat: one"),
      [`/commits/${SHA1}/statuses?per_page=100`]: [{ context: "deploy/watchdog", state: "pending" }],
      [`/commits/${SHA2}/statuses?per_page=100`]: [{ context: "deploy/watchdog", state: "pending" }],
      "/deployments?per_page=30": [],
    };
    const { poller, state, events } = await makePoller(routes);
    await poller.pollOnce();
    routes["/commits/master"] = commitPayload(SHA2, "feat: two");
    await poller.pollOnce();
    // Хвост жив (watchdog SHA1 ещё pending), событий финала нет.
    expect(state.data.github?.tail?.sha).toBe(SHA1);
    expect(events.some((event) => event.kind === "green")).toBe(false);
    // Карантин хвоста тоже доизлучается и снимает хвост.
    routes[`/commits/${SHA1}/statuses?per_page=100`] = [{ context: "deploy/watchdog", state: "failure" }];
    await poller.pollOnce();
    expect(events).toContainEqual({ kind: "quarantine", sha: SHA1 });
    expect(state.data.github?.tail).toBeUndefined();
  });
});

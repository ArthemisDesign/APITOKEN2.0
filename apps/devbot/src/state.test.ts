import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { emptyState, StateStore } from "./state.js";

async function tempDir(): Promise<string> {
  return await mkdtemp(path.join(tmpdir(), "devbot-state-"));
}

describe("StateStore", () => {
  it("roundtrips state through an atomic save/load", async () => {
    const dir = await tempDir();
    const file = path.join(dir, "state.json");
    const store = new StateStore(file);
    await store.load();
    store.data.lastProcessedSha = "1bd14c3";
    store.data.deploy = { sha: "1bd14c3", title: "feat: x", messageId: 101, startedAt: 5, phases: { tests: "success" }, done: false };
    store.data.fingerprints.fp1 = { messageId: 10, topic: "warnings", count: 3, firstAt: 1, lastAt: 2, lastEditAt: 2, resolved: false };
    store.recordEvent({ ts: 100, kind: "alert", name: "X", severity: "warning" }, 1000);
    await store.save();

    const reloaded = new StateStore(file);
    await reloaded.load();
    expect(reloaded.data.lastProcessedSha).toBe("1bd14c3");
    expect(reloaded.data.deploy?.phases.tests).toBe("success");
    expect(reloaded.data.fingerprints.fp1?.count).toBe(3);
    expect(reloaded.data.events).toHaveLength(1);
  });

  it("starts fresh with a warning on a corrupt file instead of crashing", async () => {
    const dir = await tempDir();
    const file = path.join(dir, "state.json");
    await writeFile(file, "{corrupt json!!!", "utf8");
    const store = new StateStore(file);
    await expect(store.load()).resolves.toBeUndefined();
    expect(store.data).toEqual(emptyState());
  });

  it("starts fresh on a missing file", async () => {
    const dir = await tempDir();
    const store = new StateStore(path.join(dir, "absent.json"));
    await store.load();
    expect(store.data).toEqual(emptyState());
  });

  it("creates missing parent directories on save", async () => {
    const dir = await tempDir();
    const file = path.join(dir, "deep", "nested", "state.json");
    const store = new StateStore(file);
    store.data.lastProcessedSha = "abc";
    await store.save();
    const raw = JSON.parse(await readFile(file, "utf8"));
    expect(raw.lastProcessedSha).toBe("abc");
  });

  it("prunes digest events older than 48h", async () => {
    const store = new StateStore(path.join(await tempDir(), "state.json"));
    const now = 1_000_000_000_000;
    store.recordEvent({ ts: now - 49 * 3600_000, kind: "alert", name: "Old", severity: "warning" }, now);
    store.recordEvent({ ts: now - 1000, kind: "alert", name: "New", severity: "warning" }, now);
    expect(store.data.events.map((event) => event.name)).toEqual(["New"]);
  });
});

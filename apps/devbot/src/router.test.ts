import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { Dedup } from "./dedup.js";
import type { AlertInstance } from "./events.js";
import { Router } from "./router.js";
import { StateStore, type TopicKey } from "./state.js";
import type { TelegramBot } from "./tg.js";

const T0 = 1_700_000_000_000;
const CHAT_ID = -100999;
const TOPICS: Record<TopicKey, number> = { critical: 1, deploys: 2, warnings: 3, commerce: 4, ci: 5, digest: 6 };

interface SentMessage {
  chatId: number;
  text: string;
  options: { threadId?: number; replyTo?: number };
}

interface EditedMessage {
  chatId: number;
  messageId: number;
  text: string;
}

async function makeRouter(now: () => number = () => T0) {
  const sent: SentMessage[] = [];
  const edited: EditedMessage[] = [];
  let nextId = 100;
  const tg = {
    sendMessage: vi.fn(async (chatId: number, text: string, options: { threadId?: number; replyTo?: number }) => {
      sent.push({ chatId, text, options });
      nextId += 1;
      return nextId;
    }),
    editMessageText: vi.fn(async (chatId: number, messageId: number, text: string) => {
      edited.push({ chatId, messageId, text });
      return true;
    }),
  };
  const dir = await mkdtemp(path.join(tmpdir(), "devbot-router-"));
  const state = new StateStore(path.join(dir, "state.json"));
  await state.load();
  const router = new Router({
    tg: tg as unknown as TelegramBot,
    chatId: CHAT_ID,
    topics: TOPICS,
    state,
    dedup: new Dedup(state.data.fingerprints),
    repoSlug: "acme/repo",
    now,
  });
  return { router, sent, edited, state, tg };
}

function alert(overrides: Partial<AlertInstance> = {}): AlertInstance {
  return {
    fingerprint: "fp-default",
    status: "firing",
    alertname: "EngineCircuitBreakerOpen",
    severity: "critical",
    component: "claude-engine",
    summary: "Breaker open",
    description: "Detailed <description> here",
    startsAt: new Date(T0 - 60_000).toISOString(),
    ...overrides,
  };
}

describe("Router alert routing", () => {
  it("routes critical alerts to the Critical topic with HTML formatting", async () => {
    const { router, sent } = await makeRouter();
    await router.handleAlert(alert());
    expect(sent).toHaveLength(1);
    expect(sent[0]?.options.threadId).toBe(TOPICS.critical);
    expect(sent[0]?.text).toContain("🔴");
    expect(sent[0]?.text).toContain("<b>EngineCircuitBreakerOpen</b>");
    expect(sent[0]?.text).toContain("[critical · claude-engine]");
    // HTML в описании экранируется.
    expect(sent[0]?.text).toContain("&lt;description&gt;");
    expect(sent[0]?.text).toContain("MONITORING.md#enginecircuitbreakeropen");
  });

  it("routes warning alerts to the Warnings topic", async () => {
    const { router, sent } = await makeRouter();
    await router.handleAlert(alert({ severity: "warning", alertname: "HostDiskSpaceLow" }));
    expect(sent[0]?.options.threadId).toBe(TOPICS.warnings);
  });

  it("duplicates money-domain alerts to Commerce as a reply header", async () => {
    const { router, sent } = await makeRouter();
    await router.handleAlert(alert({ alertname: "FailedWebhooksPresent" }));
    expect(sent).toHaveLength(2);
    expect(sent[0]?.options.threadId).toBe(TOPICS.critical);
    expect(sent[1]?.options.threadId).toBe(TOPICS.commerce);
    expect(sent[1]?.options.replyTo).toBeDefined();
    expect(sent[1]?.text).toContain("💰");
    expect(sent[1]?.text).toContain("FailedWebhooksPresent");
  });

  it("does not duplicate non-money alerts to Commerce", async () => {
    const { router, sent } = await makeRouter();
    await router.handleAlert(alert({ alertname: "EngineCircuitBreakerOpen" }));
    expect(sent).toHaveLength(1);
  });

  it("edits the existing message with a ×N counter on repeat firing", async () => {
    let now = T0;
    const { router, sent, edited } = await makeRouter(() => now);
    await router.handleAlert(alert());
    now += 3600_000; // repeat_interval Alertmanager для critical = 1 ч
    await router.handleAlert(alert());
    expect(sent).toHaveLength(1);
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("×2");
  });

  it("collapses warning repeats: no edit inside the 5-minute window", async () => {
    let now = T0;
    const { router, sent, edited } = await makeRouter(() => now);
    const warning = alert({ severity: "warning", alertname: "HostDiskSpaceLow" });
    await router.handleAlert(warning);
    now += 60_000;
    await router.handleAlert(warning);
    expect(edited).toHaveLength(0);
    now += 5 * 60_000;
    await router.handleAlert(warning);
    expect(sent).toHaveLength(1);
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("×3");
  });

  it("sends critical resolved as a separate closing message", async () => {
    const { router, sent, edited } = await makeRouter();
    await router.handleAlert(alert());
    await router.handleAlert(alert({ status: "resolved" }));
    expect(edited).toHaveLength(0);
    expect(sent).toHaveLength(2);
    expect(sent[1]?.options.threadId).toBe(TOPICS.critical);
    expect(sent[1]?.text).toContain("🟢");
    expect(sent[1]?.text).toContain("RESOLVED");
  });

  it("resolves warnings by editing the original message within 48h", async () => {
    const { router, sent, edited } = await makeRouter();
    const warning = alert({ severity: "warning", alertname: "HostDiskSpaceLow" });
    await router.handleAlert(warning);
    await router.handleAlert(alert({ ...warning, status: "resolved" }));
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("RESOLVED");
    expect(sent).toHaveLength(1);
  });

  it("falls back to a new reply when the original warning is older than 48h", async () => {
    let now = T0;
    const { router, sent, edited } = await makeRouter(() => now);
    const warning = alert({ severity: "warning", alertname: "HostDiskSpaceLow", startsAt: new Date(T0 - 49 * 3600_000).toISOString() });
    await router.handleAlert(warning);
    now += 49 * 3600_000;
    await router.handleAlert(alert({ ...warning, status: "resolved" }));
    expect(edited).toHaveLength(0);
    expect(sent).toHaveLength(2);
    expect(sent[1]?.options.threadId).toBe(TOPICS.warnings);
    expect(sent[1]?.text).toContain("RESOLVED");
  });
});

describe("Router deploy events", () => {
  it("creates one collapsible message per SHA and edits it on phase transitions", async () => {
    const { router, sent, edited } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x" });
    expect(sent).toHaveLength(1);
    expect(sent[0]?.options.threadId).toBe(TOPICS.deploys);
    expect(sent[0]?.text).toContain("<code>1bd14c3</code>");
    expect(sent[0]?.text).toContain("⏳ tests");

    await router.handleDeployEvent({ kind: "phase", sha: "1bd14c3deadbeef", phase: "tests", state: "success" });
    expect(sent).toHaveLength(1);
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("✅ tests");
  });

  it("finishes the deploy message with duration on green", async () => {
    const { router, edited } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x" });
    await router.handleDeployEvent({ kind: "green", sha: "1bd14c3deadbeef" });
    const last = edited.at(-1)?.text ?? "";
    expect(last).toContain("✅ done");
    expect(last).toContain("✅ tests");
    expect(last).toContain("✅ admin");
  });

  it("duplicates quarantine to Critical and marks the deploy message failed", async () => {
    const { router, sent, edited } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x" });
    await router.handleDeployEvent({ kind: "quarantine", sha: "1bd14c3deadbeef", phase: "tests" });
    expect(edited.at(-1)?.text).toContain("❌ failed");
    const criticalMessage = sent.find((message) => message.options.threadId === TOPICS.critical);
    expect(criticalMessage?.text).toContain("quarantined");
    expect(criticalMessage?.text).toContain("tests");
  });

  it("sends candidate-validation events to the CI topic", async () => {
    const { router, sent } = await makeRouter();
    await router.handleDeployEvent({ kind: "ci", environment: "candidate-validation", state: "success" });
    expect(sent[0]?.options.threadId).toBe(TOPICS.ci);
    expect(sent[0]?.text).toContain("candidate-validation");
  });
});

describe("Router journal events", () => {
  it("routes journal events to Deploys and duplicates critical to Critical", async () => {
    const { router, sent } = await makeRouter();
    await router.handleJournalEvent({ source: "admin-deploy", severity: "info", text: "retry requested" });
    expect(sent[0]?.options.threadId).toBe(TOPICS.deploys);
    await router.handleJournalEvent({ source: "admin-deploy", severity: "critical", text: "rollback target also unhealthy — manual intervention required" });
    expect(sent).toHaveLength(3);
    expect(sent[2]?.options.threadId).toBe(TOPICS.critical);
  });
});

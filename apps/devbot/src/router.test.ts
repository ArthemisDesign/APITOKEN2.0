import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { Dedup } from "./dedup.js";
import type { AlertInstance, ChatwootIncomingMessage, PartnerApplicationEvent } from "./events.js";
import { Router } from "./router.js";
import { StateStore, type TopicKey } from "./state.js";
import type { TelegramBot } from "./tg.js";

const T0 = 1_700_000_000_000;
const CHAT_ID = -100999;
const TOPICS: Record<TopicKey, number> = { critical: 1, deploys: 2, warnings: 3, commerce: 4, digest: 5, support: 6, partners: 7 };

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

async function makeRouter(
  now: () => number = () => T0,
  timeZone = "Asia/Tbilisi",
  sendDelayMs = 0,
  topics: Record<TopicKey, number> = TOPICS,
) {
  const sent: SentMessage[] = [];
  const edited: EditedMessage[] = [];
  let nextId = 100;
  const tg = {
    sendMessage: vi.fn(async (chatId: number, text: string, options: { threadId?: number; replyTo?: number }) => {
      sent.push({ chatId, text, options });
      if (sendDelayMs > 0) await new Promise((resolve) => setTimeout(resolve, sendDelayMs));
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
    topics,
    state,
    dedup: new Dedup(state.data.fingerprints),
    repoSlug: "acme/repo",
    timeZone,
    chatwootBaseUrl: "https://support.apitoken.sale",
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
    expect(sent[0]?.text).toContain("🚀 <b>Deploy</b>");
    expect(sent[0]?.text).toContain("<code>1bd14c3</code>");
    // Чеклист — две фиксированные строки 4+4.
    expect(sent[0]?.text).toContain("⏳ tests · ⏳ migration · ⏳ engine · ⏳ backend\n⏳ sales · ⏳ openkeys · ⏳ admin · ⏳ devbot");

    await router.handleDeployEvent({ kind: "phase", sha: "1bd14c3deadbeef", phase: "tests", state: "success" });
    expect(sent).toHaveLength(1);
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("✅ tests");
  });

  it("shows the commit author bold in the deploy message meta line", async () => {
    const { router, sent } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x", author: "3xcalibur @3xcalibur-tech" });
    expect(sent[0]?.text).toContain("👤 <b>3xcalibur @3xcalibur-tech</b>");
  });

  it("renders operator-facing times in the configured time zone", async () => {
    const startedAt = Date.parse("2026-08-01T19:39:00Z");
    const { router, sent } = await makeRouter(() => startedAt, "Asia/Tbilisi");
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x" });
    expect(sent[0]?.text).toContain("Started 23:39");
  });

  it("serializes a same-snapshot new SHA and green final behind Telegram message creation", async () => {
    const { router, sent, edited, state } = await makeRouter(() => T0, "Asia/Tbilisi", 10);
    const sha = "ba8fddb272fce950655f6efb2461c4f73536d3fd";
    await router.handleDeployEvents([
      { kind: "new-sha", sha, title: "docs(engine): design" },
      // GitHub returns newest statuses first, so watchdog commonly precedes phases.
      { kind: "green", sha },
      { kind: "phase", sha, phase: "migration", state: "success" },
      { kind: "phase", sha, phase: "tests", state: "success" },
    ]);

    expect(sent).toHaveLength(1);
    expect(sent[0]?.text).toContain("⏳ tests");
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("✅ <b>Deployed</b>");
    expect(state.data.deploy).toMatchObject({ sha, done: true, messageId: 101 });
  });

  it("collapses a green deploy to a compact two-line summary without the phase checklist", async () => {
    const { router, edited, state } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x", author: "qqjamba" });
    await router.handleDeployEvent({ kind: "phase", sha: "1bd14c3deadbeef", phase: "engine", state: "pending" });
    expect(edited.at(-1)?.text).toContain("🔄 engine");
    await router.handleDeployEvent({ kind: "green", sha: "1bd14c3deadbeef" });
    const last = edited.at(-1)?.text ?? "";
    // Промежуточные фазы после успеха не нужны: ни одной иконки фазы в финале.
    expect(last).toContain("✅ <b>Deployed</b>");
    expect(last).toContain("<i>done in");
    expect(last).toContain("👤 <b>qqjamba</b>");
    expect(last).not.toContain("tests");
    expect(last).not.toContain("🔄");
    expect(last).not.toContain("⏳");
    // Но состояние остаётся правдивым: зелёный watchdog закрывает и pending-фазы.
    expect(state.data.deploy?.phases.engine).toBe("success");
  });

  it("duplicates quarantine to Critical and keeps the checklist for diagnosis", async () => {
    const { router, sent, edited } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "1bd14c3deadbeef", title: "feat: x", author: "qqjamba" });
    await router.handleDeployEvent({ kind: "phase", sha: "1bd14c3deadbeef", phase: "tests", state: "failure" });
    await router.handleDeployEvent({ kind: "quarantine", sha: "1bd14c3deadbeef", phase: "tests" });
    const lastDeploy = edited.at(-1)?.text ?? "";
    expect(lastDeploy).toContain("❌ <b>Deploy failed</b>");
    expect(lastDeploy).toContain("<i>failed in");
    expect(lastDeploy).toContain("failed phase: <b>tests</b>");
    expect(lastDeploy).toContain("❌ tests");
    const criticalMessage = sent.find((message) => message.options.threadId === TOPICS.critical);
    expect(criticalMessage?.text).toContain("quarantined");
    expect(criticalMessage?.text).toContain("tests");
    expect(criticalMessage?.text).toContain("👤 qqjamba");
  });

  it("does not duplicate quarantine when watchdog failure follows a phase failure", async () => {
    const { router, sent } = await makeRouter();
    const sha = "1bd14c3deadbeef";
    await router.handleDeployEvents([
      { kind: "new-sha", sha, title: "feat: x" },
      { kind: "phase", sha, phase: "tests", state: "failure" },
      { kind: "quarantine", sha, phase: "tests" },
      { kind: "quarantine", sha },
    ]);
    expect(sent.filter((message) => message.options.threadId === TOPICS.critical)).toHaveLength(1);
  });

  it("finalizes the previous deploy from tail events after HEAD moves on", async () => {
    const { router, edited, state } = await makeRouter();
    await router.handleDeployEvent({ kind: "new-sha", sha: "111aaaa", title: "feat: old" });
    const oldMessageId = state.data.deploy?.messageId;
    // HEAD уходит на новый SHA до прихода green старого — старый уходит в previousDeploy.
    await router.handleDeployEvent({ kind: "new-sha", sha: "222bbbb", title: "feat: new" });
    expect(state.data.previousDeploy?.sha).toBe("111aaaa");
    // Tail-опрос доизлучает финал старого: правится ЕГО сообщение, а не нового.
    await router.handleDeployEvent({ kind: "green", sha: "111aaaa" });
    const finalEdit = edited.at(-1);
    expect(finalEdit?.messageId).toBe(oldMessageId);
    expect(finalEdit?.text).toContain("✅ <b>Deployed</b>");
    expect(finalEdit?.text).toContain("feat: old");
    expect(state.data.deploy?.sha).toBe("222bbbb");
    expect(state.data.deploy?.done).toBe(false);
    // Поздняя фаза для завершённого деплоя не разворачивает сводку обратно.
    const editsBefore = edited.length;
    await router.handleDeployEvent({ kind: "phase", sha: "111aaaa", phase: "engine", state: "pending" });
    expect(edited).toHaveLength(editsBefore);
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

function chatwootMessage(overrides: Partial<ChatwootIncomingMessage> = {}): ChatwootIncomingMessage {
  return {
    id: "42",
    content: "Hello <script>",
    createdAt: "2026-08-22 12:00:00 UTC",
    conversationId: "15",
    accountId: "1",
    inboxName: "Website",
    name: "Jane Doe",
    email: "jane@example.com",
    attachments: [],
    ...overrides,
  };
}

describe("Router Chatwoot messages", () => {
  it("posts an incoming client message to the Support topic with a dashboard link", async () => {
    const { router, sent } = await makeRouter();
    await router.handleChatwoot(chatwootMessage());
    expect(sent).toHaveLength(1);
    expect(sent[0]?.options.threadId).toBe(TOPICS.support);
    expect(sent[0]?.text).toContain("💬 <b>Support</b> · Website");
    expect(sent[0]?.text).toContain("Jane Doe");
    expect(sent[0]?.text).toContain("диалог #15");
    expect(sent[0]?.text).toContain("Hello &lt;script&gt;");
    expect(sent[0]?.text).toContain("https://support.apitoken.sale/app/accounts/1/conversations/15");
  });

  it("deduplicates Chatwoot retries of the same message id", async () => {
    const { router, sent } = await makeRouter();
    await router.handleChatwoot(chatwootMessage());
    await router.handleChatwoot(chatwootMessage({ content: "retry body" }));
    expect(sent).toHaveLength(1);
  });

  it("skips sending when the Support topic is not provisioned", async () => {
    const { router, sent } = await makeRouter(() => T0, "Asia/Tbilisi", 0, { ...TOPICS, support: 0 });
    await router.handleChatwoot(chatwootMessage());
    expect(sent).toHaveLength(0);
  });
});

function partnerEvent(overrides: Partial<PartnerApplicationEvent> = {}): PartnerApplicationEvent {
  return {
    event: "submitted",
    id: "6a3a0f4b-2f24-4a2e-a0a5-2f1ad2d7f4b1",
    email: "partner@example.com",
    status: "pending",
    message: "I run an agency.",
    createdAt: "2026-08-23T09:00:00.000Z",
    reviewerActor: null,
    reviewerNote: null,
    ...overrides,
  };
}

describe("Router partner applications", () => {
  it("posts a new application to the Partners topic", async () => {
    const { router, sent } = await makeRouter();
    await router.handlePartner(partnerEvent());
    expect(sent).toHaveLength(1);
    expect(sent[0]?.options.threadId).toBe(TOPICS.partners);
    expect(sent[0]?.text).toContain("🤝 <b>New partner access request</b> · pending");
    expect(sent[0]?.text).toContain("partner@example.com");
    expect(sent[0]?.text).toContain("I run an agency.");
    expect(sent[0]?.text).toContain("https://admin.apitoken.sale/partners/applications");
  });

  it("edits the same Telegram message when the applicant refreshes the request", async () => {
    const { router, sent, edited } = await makeRouter();
    await router.handlePartner(partnerEvent());
    await router.handlePartner(partnerEvent({ event: "updated", message: "Updated pitch." }));
    expect(sent).toHaveLength(1);
    expect(edited).toHaveLength(1);
    expect(edited[0]?.text).toContain("Updated pitch.");
  });

  it("skips sending when the Partners topic is not provisioned", async () => {
    const { router, sent } = await makeRouter(() => T0, "Asia/Tbilisi", 0, { ...TOPICS, partners: 0 });
    await router.handlePartner(partnerEvent());
    expect(sent).toHaveLength(0);
  });
});

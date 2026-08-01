import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import {
  buildDigestReport,
  CommandHandler,
  msUntilNext,
  parseDuration,
  type CommandsDeps,
} from "./commands.js";
import { StateStore } from "./state.js";
import type { TelegramBot, TgUpdate } from "./tg.js";

const T0 = new Date("2026-08-01T13:00:00").getTime();
const CHAT_ID = -100999;
const ADMIN_ID = 42;

async function makeHandler(overrides: Partial<CommandsDeps> = {}) {
  const sent: { text: string; options: { threadId?: number } }[] = [];
  const tg = {
    sendMessage: vi.fn(async (_chatId: number, text: string, options: { threadId?: number }) => {
      sent.push({ text, options });
      return 1;
    }),
  };
  const dir = await mkdtemp(path.join(tmpdir(), "devbot-cmd-"));
  const state = new StateStore(path.join(dir, "state.json"));
  await state.load();
  const fetchFn = overrides.fetchFn ?? (async () => new Response("[]", { status: 200 }));
  const handler = new CommandHandler({
    tg: tg as unknown as TelegramBot,
    chatId: CHAT_ID,
    adminIds: new Set([ADMIN_ID]),
    state,
    alertmanagerUrl: "http://127.0.0.1:9093",
    probes: [{ name: "router", url: "http://127.0.0.1:8798/health" }],
    digestTopicId: 6,
    now: () => T0,
    ...overrides,
    fetchFn,
  });
  return { handler, sent, tg, state };
}

function update(text: string, options: { chatId?: number; userId?: number; threadId?: number } = {}): TgUpdate {
  return {
    update_id: 1,
    message: {
      message_id: 10,
      chat: { id: options.chatId ?? CHAT_ID },
      from: { id: options.userId ?? ADMIN_ID },
      text,
      ...(options.threadId !== undefined ? { message_thread_id: options.threadId } : {}),
    },
  };
}

describe("CommandHandler gates", () => {
  it("silently ignores updates from other chats", async () => {
    const { handler, sent } = await makeHandler();
    await handler.handleUpdate(update("/help", { chatId: -555 }));
    expect(sent).toHaveLength(0);
  });

  it("silently ignores non-admin users", async () => {
    const { handler, sent } = await makeHandler();
    await handler.handleUpdate(update("/help", { userId: 777 }));
    expect(sent).toHaveLength(0);
  });

  it("ignores non-command messages", async () => {
    const { handler, sent } = await makeHandler();
    await handler.handleUpdate(update("hello bot"));
    expect(sent).toHaveLength(0);
  });

  it("replies to /help in the invoking thread with the command list", async () => {
    const { handler, sent } = await makeHandler();
    await handler.handleUpdate(update("/help", { threadId: 3 }));
    expect(sent).toHaveLength(1);
    expect(sent[0]?.options.threadId).toBe(3);
    expect(sent[0]?.text).toContain("/status");
    expect(sent[0]?.text).toContain("/silence");
    expect(sent[0]?.text).toContain("/digest");
  });
});

describe("/status", () => {
  it("assembles pipeline, alerts and readiness from mocked backends", async () => {
    const fetchFn: typeof fetch = async (url) => {
      const target = String(url);
      if (target.includes("api.github.com")) {
        return new Response(JSON.stringify({
          state: "success",
          sha: "1bd14c3deadbeef",
          statuses: [{ context: "deploy/watchdog", state: "success" }],
        }), { status: 200 });
      }
      if (target.includes("/api/v2/alerts")) {
        return new Response(JSON.stringify([
          { labels: { alertname: "EngineCircuitBreakerOpen", severity: "critical" }, status: { state: "active" }, startsAt: new Date(T0 - 600_000).toISOString() },
          { labels: { alertname: "HostDiskSpaceLow", severity: "warning" }, status: { state: "active" }, startsAt: new Date(T0 - 300_000).toISOString() },
          { labels: { alertname: "Silenced", severity: "warning" }, status: { state: "suppressed" }, startsAt: new Date(T0).toISOString() },
        ]), { status: 200 });
      }
      return new Response('{"ok":true}', { status: 200 });
    };
    const { handler, sent } = await makeHandler({
      fetchFn,
      github: { token: "ghp_x", repo: "acme/repo" },
    });
    await handler.handleUpdate(update("/status"));
    const text = sent[0]?.text ?? "";
    expect(text).toContain("✅");
    expect(text).toContain("1bd14c3");
    expect(text).toContain("🔴 1");
    expect(text).toContain("🟡 1");
    expect(text).toContain("EngineCircuitBreakerOpen");
    expect(text).not.toContain("Silenced");
    expect(text).toContain("✅ router");
  });

  it("degrades gracefully when every backend is down", async () => {
    const fetchFn: typeof fetch = async () => {
      throw new Error("connection refused");
    };
    const { handler, sent } = await makeHandler({
      fetchFn,
      github: { token: "ghp_x", repo: "acme/repo" },
    });
    await handler.handleUpdate(update("/status"));
    const text = sent[0]?.text ?? "";
    expect(text).toContain("⚠️ github недоступен");
    expect(text).toContain("⚠️ alertmanager недоступен");
    expect(text).toContain("❌ router");
  });
});

describe("/silence", () => {
  it("posts a silence with matcher and parsed duration to Alertmanager", async () => {
    let captured: { url: string; body: Record<string, unknown> } | undefined;
    const fetchFn: typeof fetch = async (url, init) => {
      captured = { url: String(url), body: JSON.parse(String(init?.body)) };
      return new Response(JSON.stringify({ silenceID: "sil-1" }), { status: 200 });
    };
    const { handler, sent } = await makeHandler({ fetchFn });
    await handler.handleUpdate(update("/silence HostDiskSpaceLow 2h"));
    expect(captured?.url).toBe("http://127.0.0.1:9093/api/v2/silences");
    expect(captured?.body.matchers).toEqual([{ name: "alertname", value: "HostDiskSpaceLow", isRegex: false, isEqual: true }]);
    const starts = Date.parse(String(captured?.body.startsAt));
    const ends = Date.parse(String(captured?.body.endsAt));
    expect(ends - starts).toBe(2 * 3600_000);
    expect(sent[0]?.text).toContain("🔇");
    expect(sent[0]?.text).toContain("sil-1");
  });

  it("rejects a broken duration without calling Alertmanager", async () => {
    const fetchFn: typeof fetch = async () => {
      throw new Error("must not be called");
    };
    const { handler, sent } = await makeHandler({ fetchFn });
    await handler.handleUpdate(update("/silence HostDiskSpaceLow forever"));
    expect(sent[0]?.text).toContain("Не понял длительность");
  });
});

describe("/pool and /settlement without engine keys", () => {
  it("answers 'not configured'", async () => {
    const { handler, sent } = await makeHandler();
    await handler.handleUpdate(update("/pool"));
    expect(sent[0]?.text).toContain("не настроен");
    await handler.handleUpdate(update("/settlement"));
    expect(sent[1]?.text).toContain("не настроен");
  });
});

describe("/digest", () => {
  it("sends the 24h summary to the Digest topic", async () => {
    const { handler, sent, state } = await makeHandler();
    state.recordEvent({ ts: T0 - 3600_000, kind: "alert", name: "HostDiskSpaceLow", severity: "warning" }, T0);
    state.recordEvent({ ts: T0 - 300_000, kind: "alert", name: "HostDiskSpaceLow", severity: "warning" }, T0);
    state.recordEvent({ ts: T0 - 100_000, kind: "deploy", name: "1bd14c3", severity: "success" }, T0);
    await handler.handleUpdate(update("/digest"));
    expect(sent[0]?.options.threadId).toBe(6);
    expect(sent[0]?.text).toContain("Digest за 24 ч");
    expect(sent[0]?.text).toContain("✅ 1");
    expect(sent[0]?.text).toContain("HostDiskSpaceLow ×2");
  });
});

describe("helpers", () => {
  it("parseDuration parses s/m/h/d and rejects garbage", () => {
    expect(parseDuration("30m")).toBe(1_800_000);
    expect(parseDuration("2h")).toBe(7_200_000);
    expect(parseDuration("1d")).toBe(86_400_000);
    expect(parseDuration("45s")).toBe(45_000);
    expect(parseDuration("forever")).toBeNull();
    expect(parseDuration("8d")).toBeNull();
    expect(parseDuration("0h")).toBeNull();
  });

  it("buildDigestReport counts only the last 24h", () => {
    const events = [
      { ts: T0 - 25 * 3600_000, kind: "alert" as const, name: "Old", severity: "warning" },
      { ts: T0 - 1000, kind: "alert" as const, name: "New", severity: "critical" },
      { ts: T0 - 1000, kind: "deploy" as const, name: "abc", severity: "quarantine" },
    ];
    const report = buildDigestReport(events, T0);
    expect(report).toContain("🔴 1");
    expect(report).toContain("карантин 🚨 1");
    expect(report).not.toContain("Old");
  });

  it("msUntilNext targets the next local hh:mm", () => {
    const morning = new Date("2026-08-01T09:00:00").getTime();
    expect(msUntilNext(10, 0, morning)).toBe(3600_000);
    const evening = new Date("2026-08-01T23:00:00").getTime();
    expect(msUntilNext(10, 0, evening)).toBe(11 * 3600_000);
  });
});

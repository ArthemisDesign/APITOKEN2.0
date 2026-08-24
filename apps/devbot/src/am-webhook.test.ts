import { createHmac } from "node:crypto";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { AddressInfo } from "node:net";
import { describe, expect, it } from "vitest";
import { createAmServer, mapAmPayload, MetricsRegistry, writeHeartbeatFile } from "./am-webhook.js";
import { CHATWOOT_WEBHOOK_PREFIX } from "./chatwoot.js";
import type { AlertInstance, ChatwootIncomingMessage, PartnerApplicationEvent } from "./events.js";
import { PARTNER_WEBHOOK_PREFIX } from "./partner-application.js";
import { Logger } from "./log.js";

const SECRET = "test-am-secret-128bit";

function samplePayload() {
  return {
    version: "4",
    status: "firing",
    alerts: [
      {
        status: "firing",
        fingerprint: "fp1234567890",
        labels: { alertname: "EngineCircuitBreakerOpen", severity: "critical", component: "claude-engine" },
        annotations: { summary: "Breaker open", description: "Circuit breaker open for >1m" },
        startsAt: "2026-08-01T13:44:00Z",
      },
      {
        status: "resolved",
        fingerprint: "fp999",
        labels: { alertname: "HostDiskSpaceLow", severity: "warning" },
        annotations: {},
        startsAt: "2026-08-01T12:00:00Z",
        endsAt: "2026-08-01T13:00:00Z",
      },
    ],
  };
}

describe("mapAmPayload", () => {
  it("maps a v4 grouped payload to alert instances", () => {
    const alerts = mapAmPayload(samplePayload());
    expect(alerts).toHaveLength(2);
    expect(alerts[0]).toMatchObject({
      fingerprint: "fp1234567890",
      status: "firing",
      alertname: "EngineCircuitBreakerOpen",
      severity: "critical",
      component: "claude-engine",
      summary: "Breaker open",
    });
    expect(alerts[1]).toMatchObject({ status: "resolved", alertname: "HostDiskSpaceLow", severity: "warning" });
  });

  it("rejects payloads without an alerts array", () => {
    expect(() => mapAmPayload({ version: "4" })).toThrow();
    expect(() => mapAmPayload("nope")).toThrow();
  });
});

describe("am-webhook server", () => {
  async function withServer(run: (base: string, received: AlertInstance[], metrics: MetricsRegistry) => Promise<void>) {
    const metrics = new MetricsRegistry();
    const received: AlertInstance[] = [];
    const am = createAmServer({
      port: 0,
      secret: SECRET,
      metrics,
      onAlerts: (alerts) => received.push(...alerts),
      heartbeatRefreshMs: 3_600_000,
      heartbeatFileMs: 3_600_000,
    });
    await new Promise<void>((resolve) => am.server.listen(0, "127.0.0.1", () => resolve()));
    const { port } = am.server.address() as AddressInfo;
    try {
      await run(`http://127.0.0.1:${port}`, received, metrics);
    } finally {
      await am.close();
    }
  }

  it("accepts the secret path and routes parsed alerts", async () => {
    await withServer(async (base, received) => {
      const response = await fetch(`${base}/alerts/${SECRET}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(samplePayload()),
      });
      expect(response.status).toBe(200);
      expect(received).toHaveLength(2);
      expect(received[0]?.alertname).toBe("EngineCircuitBreakerOpen");
    });
  });

  it("returns 404 for unknown paths, including near-miss secrets", async () => {
    await withServer(async (base, received) => {
      for (const url of [`${base}/alerts/wrong`, `${base}/alerts`, `${base}/`, `${base}/metrics2`]) {
        const response = await fetch(url, { method: "POST" });
        expect(response.status).toBe(404);
      }
      expect(received).toHaveLength(0);
    });
  });

  it("returns 400 on malformed JSON without routing anything", async () => {
    await withServer(async (base, received) => {
      const response = await fetch(`${base}/alerts/${SECRET}`, { method: "POST", body: "{not json" });
      expect(response.status).toBe(400);
      expect(received).toHaveLength(0);
    });
  });

  it("serves /health for the deploy health gate", async () => {
    await withServer(async (base) => {
      const response = await fetch(`${base}/health`);
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({ ok: true });
    });
  });

  it("serves /metrics with heartbeat, event counters and failure counter", async () => {
    await withServer(async (base, _received, metrics) => {
      metrics.incEvent("critical", "alert");
      metrics.incEvent("critical", "alert");
      metrics.incEvent("deploys", "green");
      metrics.incTelegramFailure();
      const response = await fetch(`${base}/metrics`);
      expect(response.status).toBe(200);
      const text = await response.text();
      expect(text).toContain("devbot_heartbeat_timestamp_seconds ");
      expect(text).toContain('devbot_events_total{topic="critical",kind="alert"} 2');
      expect(text).toContain('devbot_events_total{topic="deploys",kind="green"} 1');
      expect(text).toContain("devbot_telegram_send_failures_total 1");
      const webhookTs = /devbot_last_webhook_seconds (\d+)/.exec(text);
      expect(webhookTs).not.toBeNull();
      expect(Number(webhookTs?.[1])).toBeGreaterThan(0);
      expect(text).toContain("devbot_last_chatwoot_seconds 0");
    });
  });

  it("records the last accepted webhook delivery in /metrics", async () => {
    await withServer(async (base) => {
      const before = Math.floor(Date.now() / 1000);
      const response = await fetch(`${base}/alerts/${SECRET}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(samplePayload()),
      });
      expect(response.status).toBe(200);
      const text = await fetch(`${base}/metrics`).then((r) => r.text());
      const match = /devbot_last_webhook_seconds (\d+)/.exec(text);
      expect(match).not.toBeNull();
      expect(Number(match?.[1])).toBeGreaterThanOrEqual(before);
    });
  });

  it("does not count rejected payloads as webhook deliveries", async () => {
    await withServer(async (base, received) => {
      const beforeText = await fetch(`${base}/metrics`).then((r) => r.text());
      const beforeMatch = /devbot_last_webhook_seconds (\d+)/.exec(beforeText);
      expect(beforeMatch).not.toBeNull();
      const seeded = Number(beforeMatch?.[1]);
      expect(seeded).toBeGreaterThan(0);
      await fetch(`${base}/alerts/${SECRET}`, { method: "POST", body: "{not json" });
      await fetch(`${base}/alerts/wrong`, { method: "POST", body: "{}" });
      expect(received).toHaveLength(0);
      const text = await fetch(`${base}/metrics`).then((r) => r.text());
      const afterMatch = /devbot_last_webhook_seconds (\d+)/.exec(text);
      expect(Number(afterMatch?.[1])).toBe(seeded);
      expect(text).toContain("devbot_last_chatwoot_seconds 0");
    });
  });

  it("seeds last-webhook gauge to process start instead of Unix epoch", () => {
    const before = Math.floor(Date.now() / 1000);
    const metrics = new MetricsRegistry();
    const after = Math.floor(Date.now() / 1000);
    const match = /devbot_last_webhook_seconds (\d+)/.exec(metrics.render());
    expect(match).not.toBeNull();
    const value = Number(match?.[1]);
    expect(value).toBeGreaterThanOrEqual(before);
    expect(value).toBeLessThanOrEqual(after);
    expect(value).not.toBe(0);
  });
});

describe("chatwoot webhook server", () => {
  const CW_SECRET = "chatwoot-path-secret";
  const HMAC = "chatwoot-hmac-secret";
  const incoming = {
    event: "message_created",
    id: 42,
    content: "help",
    message_type: "incoming",
    conversation: { display_id: 15 },
    account: { id: 1 },
    sender: { name: "Jane" },
  };

  async function withChatwoot(
    hmac: string | undefined,
    run: (base: string, received: ChatwootIncomingMessage[]) => Promise<void>,
  ) {
    const received: ChatwootIncomingMessage[] = [];
    const am = createAmServer({
      port: 0,
      secret: SECRET,
      metrics: new MetricsRegistry(),
      onAlerts: () => undefined,
      heartbeatRefreshMs: 3_600_000,
      heartbeatFileMs: 3_600_000,
      chatwoot: {
        secret: CW_SECRET,
        ...(hmac ? { hmacSecret: hmac, nowSec: () => 1_700_000_000 } : {}),
        onMessage: (message) => received.push(message),
      },
    });
    await new Promise<void>((resolve) => am.server.listen(0, "127.0.0.1", () => resolve()));
    const { port } = am.server.address() as AddressInfo;
    try {
      await run(`http://127.0.0.1:${port}`, received);
    } finally {
      await am.close();
    }
  }

  it("routes an incoming Chatwoot message on the secret path", async () => {
    await withChatwoot(undefined, async (base, received) => {
      const response = await fetch(`${base}${CHATWOOT_WEBHOOK_PREFIX}${CW_SECRET}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(incoming),
      });
      expect(response.status).toBe(200);
      expect(received).toHaveLength(1);
      expect(received[0]?.id).toBe("42");
    });
  });

  it("acknowledges outgoing messages without routing them", async () => {
    await withChatwoot(undefined, async (base, received) => {
      const response = await fetch(`${base}${CHATWOOT_WEBHOOK_PREFIX}${CW_SECRET}`, {
        method: "POST",
        body: JSON.stringify({ ...incoming, message_type: "outgoing" }),
      });
      expect(response.status).toBe(200);
      expect(received).toHaveLength(0);
    });
  });

  it("returns 404 for a near-miss Chatwoot secret", async () => {
    await withChatwoot(undefined, async (base, received) => {
      const response = await fetch(`${base}${CHATWOOT_WEBHOOK_PREFIX}wrong`, {
        method: "POST",
        body: JSON.stringify(incoming),
      });
      expect(response.status).toBe(404);
      expect(received).toHaveLength(0);
    });
  });

  it("requires a valid HMAC when a Chatwoot HMAC secret is configured", async () => {
    await withChatwoot(HMAC, async (base, received) => {
      const body = JSON.stringify(incoming);
      const timestamp = "1700000000";
      const signature = `sha256=${createHmac("sha256", HMAC).update(`${timestamp}.${body}`).digest("hex")}`;
      const unauthorized = await fetch(`${base}${CHATWOOT_WEBHOOK_PREFIX}${CW_SECRET}`, {
        method: "POST",
        body,
      });
      expect(unauthorized.status).toBe(401);
      const authorized = await fetch(`${base}${CHATWOOT_WEBHOOK_PREFIX}${CW_SECRET}`, {
        method: "POST",
        headers: {
          "x-chatwoot-timestamp": timestamp,
          "x-chatwoot-signature": signature,
        },
        body,
      });
      expect(authorized.status).toBe(200);
      expect(received).toHaveLength(1);
    });
  });
});

describe("partner application webhook server", () => {
  const PARTNER_SECRET = "partner-path-secret";
  const incoming = {
    event: "submitted",
    id: "6a3a0f4b-2f24-4a2e-a0a5-2f1ad2d7f4b1",
    email: "partner@example.com",
    status: "pending",
    message: "I run an agency.",
    createdAt: "2026-08-23T09:00:00.000Z",
    reviewerActor: null,
    reviewerNote: null,
  };

  async function withPartners(run: (base: string, received: PartnerApplicationEvent[]) => Promise<void>) {
    const received: PartnerApplicationEvent[] = [];
    const am = createAmServer({
      port: 0,
      secret: SECRET,
      metrics: new MetricsRegistry(),
      onAlerts: () => undefined,
      heartbeatRefreshMs: 3_600_000,
      heartbeatFileMs: 3_600_000,
      partners: {
        secret: PARTNER_SECRET,
        onEvent: (event) => received.push(event),
      },
    });
    await new Promise<void>((resolve) => am.server.listen(0, "127.0.0.1", () => resolve()));
    const { port } = am.server.address() as AddressInfo;
    try {
      await run(`http://127.0.0.1:${port}`, received);
    } finally {
      await am.close();
    }
  }

  it("routes a partner application on the secret path", async () => {
    await withPartners(async (base, received) => {
      const response = await fetch(`${base}${PARTNER_WEBHOOK_PREFIX}${PARTNER_SECRET}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(incoming),
      });
      expect(response.status).toBe(200);
      expect(received).toHaveLength(1);
      expect(received[0]?.id).toBe(incoming.id);
      expect(received[0]?.event).toBe("submitted");
    });
  });

  it("returns 400 for a malformed partner payload", async () => {
    await withPartners(async (base, received) => {
      const response = await fetch(`${base}${PARTNER_WEBHOOK_PREFIX}${PARTNER_SECRET}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ event: "ping" }),
      });
      expect(response.status).toBe(400);
      expect(received).toHaveLength(0);
    });
  });

  it("returns 404 for a near-miss partner secret", async () => {
    await withPartners(async (base, received) => {
      const response = await fetch(`${base}${PARTNER_WEBHOOK_PREFIX}wrong`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(incoming),
      });
      expect(response.status).toBe(404);
      expect(received).toHaveLength(0);
    });
  });
});

describe("writeHeartbeatFile", () => {
  it("writes the gauge atomically in node-exporter textfile format", async () => {
    const dir = await mkdtemp(path.join(tmpdir(), "devbot-hb-"));
    const file = path.join(dir, "nested", "devbot.prom");
    // Юнит работает с UMask=0077: без явного chmod heartbeat останется 0600 и node-exporter
    // (user nobody) не сможет его прочитать. Воспроизводим umask юнита вокруг записи.
    const previousUmask = process.umask(0o077);
    try {
      await writeHeartbeatFile(file, 1_700_000_000);
    } finally {
      process.umask(previousUmask);
    }
    const { readFile, stat } = await import("node:fs/promises");
    const content = await readFile(file, "utf8");
    expect(content).toContain("# TYPE devbot_heartbeat_timestamp_seconds gauge");
    expect(content).toContain("devbot_heartbeat_timestamp_seconds 1700000000");
    const mode = (await stat(file)).mode & 0o777;
    expect(mode).toBe(0o644);
  });

  it("tolerates unwritable locations with a warning, not a crash", async () => {
    // Нельзя использовать /proc как «незаписываемое» место: на Linux mkdir recursive
    // под procfs зависает вместо быстрой ошибки. Родитель-файл даёт детерминированный
    // ENOTDIR на любой платформе.
    const blocker = path.join(await mkdtemp(path.join(tmpdir(), "devbot-hb-blocked-")), "file");
    await writeFile(blocker, "not a directory", "utf8");
    const logger = new Logger("error");
    await expect(writeHeartbeatFile(path.join(blocker, "x.prom"), 1, logger)).resolves.toBeUndefined();
  });
});

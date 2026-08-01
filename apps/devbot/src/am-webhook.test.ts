import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { AddressInfo } from "node:net";
import { describe, expect, it } from "vitest";
import { createAmServer, mapAmPayload, MetricsRegistry, writeHeartbeatFile } from "./am-webhook.js";
import type { AlertInstance } from "./events.js";
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
    });
  });
});

describe("writeHeartbeatFile", () => {
  it("writes the gauge atomically in node-exporter textfile format", async () => {
    const dir = await mkdtemp(path.join(tmpdir(), "devbot-hb-"));
    const file = path.join(dir, "nested", "devbot.prom");
    await writeHeartbeatFile(file, 1_700_000_000);
    const { readFile } = await import("node:fs/promises");
    const content = await readFile(file, "utf8");
    expect(content).toContain("# TYPE devbot_heartbeat_timestamp_seconds gauge");
    expect(content).toContain("devbot_heartbeat_timestamp_seconds 1700000000");
  });

  it("tolerates unwritable locations with a warning, not a crash", async () => {
    const logger = new Logger("error");
    await expect(writeHeartbeatFile("/proc/devbot-definitely-not-writable/x.prom", 1, logger)).resolves.toBeUndefined();
  });
});

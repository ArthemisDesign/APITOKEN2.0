import http from "node:http";
import { promises as fs } from "node:fs";
import path from "node:path";
import type { AlertInstance } from "./events.js";
import { errorMessage, type Logger } from "./log.js";
import type { TopicKey } from "./state.js";

const MAX_BODY_BYTES = 1024 * 1024;

/** Метрики самого бота: devbot_events_total{topic,kind}, heartbeat, сбои отправки. */
export class MetricsRegistry {
  heartbeatTs = Math.floor(Date.now() / 1000);
  telegramSendFailures = 0;
  private readonly events = new Map<string, number>();

  incEvent(topic: TopicKey, kind: string): void {
    const key = `${topic}|${kind}`;
    this.events.set(key, (this.events.get(key) ?? 0) + 1);
  }

  incTelegramFailure(): void {
    this.telegramSendFailures += 1;
  }

  render(): string {
    const lines = [
      "# HELP devbot_heartbeat_timestamp_seconds Unix timestamp of the last devbot heartbeat.",
      "# TYPE devbot_heartbeat_timestamp_seconds gauge",
      `devbot_heartbeat_timestamp_seconds ${this.heartbeatTs}`,
      "# HELP devbot_events_total Events routed by devbot.",
      "# TYPE devbot_events_total counter",
    ];
    for (const [key, value] of [...this.events.entries()].sort()) {
      const [topic, kind] = key.split("|");
      lines.push(`devbot_events_total{topic="${topic}",kind="${kind}"} ${value}`);
    }
    lines.push(
      "# HELP devbot_telegram_send_failures_total Telegram send/edit attempts dropped after retries.",
      "# TYPE devbot_telegram_send_failures_total counter",
      `devbot_telegram_send_failures_total ${this.telegramSendFailures}`,
    );
    return `${lines.join("\n")}\n`;
  }
}

/** Разбор webhook payload Alertmanager v4 (сгруппированные нотификации). */
export function mapAmPayload(body: unknown): AlertInstance[] {
  if (typeof body !== "object" || body === null) {
    throw new Error("payload must be an object");
  }
  const alerts = (body as { alerts?: unknown }).alerts;
  if (!Array.isArray(alerts)) {
    throw new Error("payload.alerts must be an array");
  }
  return alerts.map((item) => {
    const alert = item as {
      status?: string;
      fingerprint?: string;
      labels?: Record<string, string>;
      annotations?: Record<string, string>;
      startsAt?: string;
      endsAt?: string;
    };
    const labels = alert.labels ?? {};
    const annotations = alert.annotations ?? {};
    const result: AlertInstance = {
      fingerprint: String(alert.fingerprint ?? labels.alertname ?? "unknown"),
      status: alert.status === "resolved" ? "resolved" : "firing",
      alertname: String(labels.alertname ?? "unknown"),
      severity: String(labels.severity ?? "warning"),
      startsAt: String(alert.startsAt ?? ""),
    };
    if (labels.component) result.component = labels.component;
    if (annotations.summary) result.summary = annotations.summary;
    if (annotations.description) result.description = annotations.description;
    if (alert.endsAt) result.endsAt = alert.endsAt;
    return result;
  });
}

export interface AmServerDeps {
  port: number;
  secret: string;
  metrics: MetricsRegistry;
  onAlerts: (alerts: AlertInstance[]) => void;
  logger?: Logger;
  heartbeatFile?: string;
  /** Интервал обновления heartbeat-метрики (30 с по дизайну). */
  heartbeatRefreshMs?: number;
  /** Интервал записи textfile для node-exporter (60 с по дизайну). */
  heartbeatFileMs?: number;
}

export interface AmServer {
  server: http.Server;
  close: () => Promise<void>;
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        reject(new Error("body too large"));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

/** Атомарная запись heartbeat-файла (tmp + rename); EACCES/нет каталога — warning, не краш. */
export async function writeHeartbeatFile(filePath: string, timestampSec: number, logger?: Logger): Promise<void> {
  const content = [
    "# HELP devbot_heartbeat_timestamp_seconds Unix timestamp of the last devbot heartbeat.",
    "# TYPE devbot_heartbeat_timestamp_seconds gauge",
    `devbot_heartbeat_timestamp_seconds ${timestampSec}`,
    "",
  ].join("\n");
  try {
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    const tmp = `${filePath}.tmp-${process.pid}`;
    await fs.writeFile(tmp, content, "utf8");
    // node-exporter scrapes the textfile as user nobody; the unit's UMask=0077 would otherwise
    // leave the heartbeat 0600 and unreadable, false-firing DevBotHeartbeatMissing.
    await fs.chmod(tmp, 0o644);
    await fs.rename(tmp, filePath);
  } catch (error) {
    logger?.warn(`heartbeat: cannot write ${filePath}: ${errorMessage(error)}`);
  }
}

export function createAmServer(deps: AmServerDeps): AmServer {
  const alertPath = `/alerts/${deps.secret}`;

  const server = http.createServer((req, res) => {
    void (async () => {
      const url = new URL(req.url ?? "/", "http://127.0.0.1");
      if (req.method === "GET" && url.pathname === "/health") {
        res.writeHead(200, { "content-type": "application/json" }).end('{"ok":true}');
        return;
      }
      if (req.method === "GET" && url.pathname === "/metrics") {
        res.writeHead(200, { "content-type": "text/plain; version=0.0.4" }).end(deps.metrics.render());
        return;
      }
      if (req.method === "POST" && url.pathname === alertPath) {
        try {
          const alerts = mapAmPayload(JSON.parse(await readBody(req)));
          deps.onAlerts(alerts);
          res.writeHead(200, { "content-type": "application/json" }).end('{"ok":true}');
        } catch (error) {
          deps.logger?.warn(`am-webhook: bad payload: ${errorMessage(error)}`);
          res.writeHead(400, { "content-type": "application/json" }).end('{"ok":false}');
        }
        return;
      }
      res.writeHead(404, { "content-type": "application/json" }).end('{"ok":false}');
    })().catch((error) => {
      deps.logger?.error(`am-webhook: handler crashed: ${errorMessage(error)}`);
      if (!res.headersSent) res.writeHead(500).end();
    });
  });

  const heartbeatTimer = setInterval(() => {
    deps.metrics.heartbeatTs = Math.floor(Date.now() / 1000);
  }, deps.heartbeatRefreshMs ?? 30_000);
  heartbeatTimer.unref();

  let fileTimer: NodeJS.Timeout | undefined;
  if (deps.heartbeatFile) {
    const file = deps.heartbeatFile;
    fileTimer = setInterval(() => {
      void writeHeartbeatFile(file, Math.floor(Date.now() / 1000), deps.logger);
    }, deps.heartbeatFileMs ?? 60_000);
    fileTimer.unref();
  }

  return {
    server,
    close: () => new Promise((resolve) => {
      clearInterval(heartbeatTimer);
      if (fileTimer) clearInterval(fileTimer);
      server.close(() => resolve());
    }),
  };
}

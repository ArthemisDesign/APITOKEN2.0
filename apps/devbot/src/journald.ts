import { spawn, type ChildProcess } from "node:child_process";
import type { JournalEvent } from "./events.js";
import { errorMessage, type Logger } from "./log.js";

/** Syslog-идентификаторы/префиксы deploy-скриптов (DEVBOT.md §2.1). */
const SOURCES = ["watchdog", "agent-merge", "admin-deploy", "sales-deploy", "openkeys-deploy"];

const SOURCE_RE = /^\[(watchdog|agent-merge|admin-deploy|sales-deploy|openkeys-deploy)\]/;

/**
 * Парсер строки `journalctl -o json`. Возвращает событие только для известных
 * источников и только для значимых паттернов: «rolled back» (warning),
 * «manual intervention required» (critical), «retry» (info), запуск rollback.sh.
 * Остальной шум watchdog'а молча пропускается.
 */
export function parseJournalLine(line: string): JournalEvent | null {
  let record: { MESSAGE?: string; SYSLOG_IDENTIFIER?: string };
  try {
    record = JSON.parse(line) as typeof record;
  } catch {
    return null;
  }
  const message = typeof record.MESSAGE === "string" ? record.MESSAGE.trim() : "";
  if (!message) return null;
  const identifier = record.SYSLOG_IDENTIFIER ?? "";
  const prefixMatch = SOURCE_RE.exec(message);
  const source = prefixMatch?.[1] ?? (SOURCES.includes(identifier) ? identifier : null);
  if (!source) return null;
  if (/manual intervention required/i.test(message)) {
    return { source, severity: "critical", text: message };
  }
  if (/rolled back|rollback\.sh/i.test(message)) {
    return { source, severity: "warning", text: message };
  }
  if (/\bretry\b/i.test(message)) {
    return { source, severity: "info", text: message };
  }
  return null;
}

export interface JournaldTailDeps {
  onEvent: (event: JournalEvent) => void;
  logger?: Logger;
  spawnFn?: typeof spawn;
}

/**
 * Tail `journalctl -f -o json` (этап 3). Отсутствие journald (не-linux, dev-машина)
 * — warning при старте и тихая жизнь без краша; при обрыве перезапуск с backoff.
 */
export class JournaldTail {
  private child: ChildProcess | undefined;
  private stopped = false;
  private buffer = "";

  constructor(private readonly deps: JournaldTailDeps) {}

  start(): void {
    const spawnFn = this.deps.spawnFn ?? spawn;
    let child: ChildProcess;
    try {
      child = spawnFn("journalctl", ["-f", "-o", "json", "--since", "now"], { stdio: ["ignore", "pipe", "pipe"] });
    } catch (error) {
      this.deps.logger?.warn(`journald: cannot spawn journalctl: ${errorMessage(error)} — tail disabled`);
      return;
    }
    this.child = child;
    child.stdout?.on("data", (chunk: Buffer) => this.onData(chunk));
    child.stderr?.on("data", (chunk: Buffer) => {
      this.deps.logger?.debug(`journald: stderr: ${chunk.toString("utf8").trim()}`);
    });
    child.on("error", (error) => {
      this.deps.logger?.warn(`journald: ${errorMessage(error)} — tail disabled`);
    });
    child.on("exit", (code) => {
      if (this.stopped) return;
      this.deps.logger?.warn(`journald: journalctl exited with code ${code} — restarting in 30s`);
      setTimeout(() => {
        if (!this.stopped) this.start();
      }, 30_000).unref();
    });
  }

  private onData(chunk: Buffer): void {
    this.buffer += chunk.toString("utf8");
    let index = this.buffer.indexOf("\n");
    while (index >= 0) {
      const line = this.buffer.slice(0, index);
      this.buffer = this.buffer.slice(index + 1);
      const event = parseJournalLine(line);
      if (event) this.deps.onEvent(event);
      index = this.buffer.indexOf("\n");
    }
  }

  stop(): void {
    this.stopped = true;
    this.child?.kill("SIGTERM");
    this.child = undefined;
  }
}

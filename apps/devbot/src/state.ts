import { promises as fs } from "node:fs";
import path from "node:path";
import { errorMessage, type Logger } from "./log.js";

export type TopicKey = "critical" | "deploys" | "warnings" | "commerce" | "ci" | "digest";

export type PhaseState = "pending" | "success" | "failure";

export interface FingerprintEntry {
  messageId: number;
  topic: TopicKey;
  count: number;
  firstAt: number;
  lastAt: number;
  lastEditAt: number;
  resolved: boolean;
}

export interface DeployState {
  sha: string;
  title: string;
  /** Git author name (+ @login, если коммит привязан к GitHub-аккаунту). */
  author?: string;
  messageId: number | null;
  startedAt: number;
  phases: Record<string, PhaseState>;
  failedPhase?: string;
  done: boolean;
}

/** Снимок GitHub-поллера — diff строится против предыдущего снимка. */
export interface GithubSnapshot {
  sha: string;
  title: string;
  author?: string;
  statuses: Record<string, PhaseState>;
  deployments: Record<string, { id: number; state: PhaseState; sha?: string }>;
}

export interface DigestEvent {
  ts: number;
  kind: "alert" | "deploy";
  name: string;
  severity?: string;
}

export interface DevbotState {
  version: 1;
  lastProcessedSha: string | null;
  github: GithubSnapshot | null;
  deploy: DeployState | null;
  fingerprints: Record<string, FingerprintEntry>;
  events: DigestEvent[];
}

export function emptyState(): DevbotState {
  return {
    version: 1,
    lastProcessedSha: null,
    github: null,
    deploy: null,
    fingerprints: {},
    events: [],
  };
}

/**
 * JSON state-файл: последний SHA и его деплой-сообщение, fingerprint-store с TTL,
 * счётчики для дайджеста. Запись атомарная (tmp + rename); битый файл — старт
 * с чистого состояния с warning (потеря state = переотправка статуса, не катастрофа).
 */
export class StateStore {
  data: DevbotState = emptyState();
  private writing: Promise<void> = Promise.resolve();

  constructor(
    readonly filePath: string,
    private readonly logger?: Logger,
  ) {}

  async load(): Promise<void> {
    let raw: string;
    try {
      raw = await fs.readFile(this.filePath, "utf8");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
        this.logger?.warn(`state: cannot read ${this.filePath}: ${errorMessage(error)} — starting fresh`);
      }
      this.data = emptyState();
      return;
    }
    try {
      const parsed = JSON.parse(raw) as Partial<DevbotState>;
      this.data = {
        ...emptyState(),
        ...parsed,
        fingerprints: parsed.fingerprints ?? {},
        events: Array.isArray(parsed.events) ? parsed.events : [],
      };
    } catch {
      this.logger?.warn(`state: ${this.filePath} is corrupt — starting fresh`);
      this.data = emptyState();
    }
  }

  /** Атомарная запись через tmp + rename; запросы на запись выстраиваются в цепочку. */
  save(): Promise<void> {
    this.writing = this.writing.then(() => this.writeNow()).catch(() => undefined);
    return this.writing;
  }

  private async writeNow(): Promise<void> {
    try {
      await fs.mkdir(path.dirname(this.filePath), { recursive: true });
      const tmp = `${this.filePath}.tmp-${process.pid}`;
      await fs.writeFile(tmp, JSON.stringify(this.data), "utf8");
      await fs.rename(tmp, this.filePath);
    } catch (error) {
      this.logger?.warn(`state: cannot persist ${this.filePath}: ${errorMessage(error)}`);
    }
  }

  /** Журнал событий для /digest — храним 48 ч, сводка строится за 24 ч. */
  recordEvent(event: DigestEvent, now: number): void {
    const cutoff = now - 48 * 3600 * 1000;
    this.data.events = this.data.events.filter((item) => item.ts >= cutoff);
    this.data.events.push(event);
  }
}

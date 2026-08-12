import {
  Inject,
  Injectable,
  Logger,
  type MessageEvent,
  OnApplicationShutdown,
  OnModuleInit,
} from "@nestjs/common";
import { concat, interval, map, merge, Observable, of, Subject } from "rxjs";
import type { Database } from "@claude-api/db";
import { DATABASE } from "./infrastructure.module.js";

const CHANNEL = "commerce_admin_changes";
const HEARTBEAT_MS = 25_000;
const MAX_RECONNECT_MS = 30_000;

type ListenerClient = {
  query(text: string): Promise<unknown>;
  on(event: "notification", listener: (message: { channel: string; payload?: string }) => void): unknown;
  on(event: "error" | "end", listener: (error?: Error) => void): unknown;
  removeAllListeners(): unknown;
  release(destroy?: boolean | Error): void;
};

export type AdminChangeEvent = {
  source: "commerce";
  resources: string[];
  table?: string;
  resync?: true;
};

const TABLE_RESOURCES: Readonly<Record<string, readonly string[]>> = {
  users: ["/admin/users", "/admin/dashboard", "/admin/finance"],
  customer_profiles: ["/admin/users", "/admin/dashboard", "/admin/finance"],
  business_invites: ["/admin/business-invites", "/admin/dashboard"],
  signup_profiles: ["/admin/users", "/admin/dashboard", "/admin/finance/funnel"],
  engine_accounts: ["/admin/users", "/admin/dashboard", "/admin/finance"],
  engine_pricing_jobs: ["/admin/pipeline-health", "/admin/dashboard"],
  customer_provider_discounts: ["/admin/users", "/admin/business", "/admin/finance"],
  payments: ["/admin/topups", "/admin/dashboard", "/admin/finance", "/admin/users"],
  checkout_sessions: ["/admin/topups", "/admin/refunds", "/admin/dashboard", "/admin/finance"],
  engine_credits: ["/admin/users", "/admin/dashboard", "/admin/finance"],
  engine_adjustments: ["/admin/users", "/admin/dashboard", "/admin/finance"],
  webhook_events: ["/admin/pipeline-health", "/admin/dashboard"],
  email_outbox: ["/admin/pipeline-health", "/admin/dashboard"],
  pricing_usage_events: ["/admin/dashboard", "/admin/finance", "/admin/users"],
  pricing_usage_topups: ["/admin/dashboard", "/admin/finance", "/admin/users"],
  pricing_usage_attributions: ["/admin/dashboard", "/admin/finance", "/admin/users"],
  api_keys: ["/admin/users", "/admin/dashboard"],
  audit_log: ["/admin/audit"],
  admin_accounts: ["/admin/admin-accounts"],
  admin_account_domains: ["/admin/admin-accounts"],
};

const ALL_RESOURCES = [...new Set(Object.values(TABLE_RESOURCES).flat())];

export function commerceChangeForTable(table: string): AdminChangeEvent {
  return {
    source: "commerce",
    table,
    resources: [...(TABLE_RESOURCES[table] ?? ALL_RESOURCES)],
  };
}

function resyncEvent(): AdminChangeEvent {
  return { source: "commerce", resources: ALL_RESOURCES, resync: true };
}

@Injectable()
export class AdminEventsService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(AdminEventsService.name);
  private readonly changes = new Subject<AdminChangeEvent>();
  private listener: ListenerClient | undefined;
  private connecting: Promise<void> | undefined;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private reconnectAttempt = 0;
  private generation = 0;
  private stopping = false;

  constructor(@Inject(DATABASE) private readonly database: Database) {}

  async onModuleInit(): Promise<void> {
    await this.ensureConnected();
  }

  stream(): Observable<MessageEvent> {
    return concat(
      of({ type: "resync", data: resyncEvent() }),
      merge(
        this.changes.pipe(map((data) => ({ type: "change", data }))),
        interval(HEARTBEAT_MS).pipe(
          map(() => ({ type: "heartbeat", data: { source: "commerce" } })),
        ),
      ),
    );
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopping = true;
    this.generation += 1;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = undefined;
    await this.connecting?.catch(() => undefined);
    await this.disconnect();
    this.changes.complete();
  }

  private async ensureConnected(): Promise<void> {
    if (this.stopping || this.listener) return;
    if (this.connecting) return this.connecting;
    const connecting = this.connect();
    this.connecting = connecting;
    try {
      await connecting;
    } finally {
      if (this.connecting === connecting) this.connecting = undefined;
    }
  }

  private async connect(): Promise<void> {
    if (this.stopping || this.listener) return;
    const generation = ++this.generation;
    let listener: ListenerClient | undefined;
    try {
      listener = (await this.database.pool.connect()) as ListenerClient;
      if (this.stopping || generation !== this.generation) {
        listener.release();
        return;
      }
      this.listener = listener;
      const current = listener;
      listener.on("notification", (message) => {
        if (message.channel === CHANNEL && message.payload) {
          this.changes.next(commerceChangeForTable(message.payload));
        }
      });
      listener.on("error", () => this.disconnected(current, generation));
      listener.on("end", () => this.disconnected(current, generation));
      await listener.query(`LISTEN ${CHANNEL}`);
      if (this.stopping || generation !== this.generation) {
        this.listener = undefined;
        listener.removeAllListeners();
        listener.release(true);
        return;
      }
      this.reconnectAttempt = 0;
      this.changes.next(resyncEvent());
    } catch (error) {
      if (listener && this.listener === listener) {
        this.listener = undefined;
        listener.removeAllListeners();
        listener.release(true);
      }
      if (!this.stopping && generation === this.generation) {
        this.logger.warn(
          `admin event listener unavailable: ${error instanceof Error ? error.message : "unknown error"}`,
        );
        this.scheduleReconnect();
      }
    }
  }

  private disconnected(listener: ListenerClient, generation: number): void {
    if (generation !== this.generation || this.listener !== listener) return;
    this.listener = undefined;
    listener.removeAllListeners();
    listener.release(true);
    if (!this.stopping) this.scheduleReconnect();
  }

  private async disconnect(): Promise<void> {
    this.generation += 1;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = undefined;
    this.reconnectAttempt = 0;
    const listener = this.listener;
    this.listener = undefined;
    if (!listener) return;
    listener.removeAllListeners();
    await listener.query(`UNLISTEN ${CHANNEL}`).catch(() => undefined);
    listener.release();
  }

  private scheduleReconnect(): void {
    if (this.stopping || this.reconnectTimer) return;
    const delay = Math.min(1_000 * 2 ** this.reconnectAttempt, MAX_RECONNECT_MS);
    this.reconnectAttempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      void this.ensureConnected();
    }, delay);
    this.reconnectTimer.unref();
  }
}

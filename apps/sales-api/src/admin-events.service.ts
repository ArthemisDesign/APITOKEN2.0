import {
  Inject,
  Injectable,
  Logger,
  type MessageEvent,
  OnApplicationShutdown,
  OnModuleInit,
} from "@nestjs/common";
import { concat, interval, map, merge, Observable, of, Subject } from "rxjs";
import type { SalesDatabase } from "@claude-api/sales-db";
import { SALES_DATABASE } from "./infrastructure.module.js";

const CHANNEL = "sales_admin_changes";
const HEARTBEAT_MS = 25_000;
const MAX_RECONNECT_MS = 30_000;

type ListenerClient = {
  query(text: string): Promise<unknown>;
  on(event: "notification", listener: (message: { channel: string; payload?: string }) => void): unknown;
  on(event: "error" | "end", listener: (error?: Error) => void): unknown;
  removeAllListeners(): unknown;
  release(destroy?: boolean | Error): void;
};

export type SalesAdminChangeEvent = {
  source: "sales";
  resources: string[];
  table?: string;
  resync?: true;
};

const TABLE_RESOURCES: Readonly<Record<string, readonly string[]>> = {
  partners: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/partners",
  ],
  partner_applications: ["/partner-admin/overview", "/partner-admin/applications"],
  partner_invites: ["/partner-admin/invites", "/partner-admin/overview"],
  partner_discount_links: ["/partner-admin/partner-analytics"],
  promo_codes: ["/partner-admin/partner-analytics"],
  referred_users: ["/partner-admin/overview", "/partner-admin/partner-analytics"],
  partner_usage_events: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  partner_usage_events_v2: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  referred_topups: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  commission_entries: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  commission_entries_v2: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  partner_commission_adjustments: [
    "/partner-admin/overview",
    "/partner-admin/partner-analytics",
    "/partner-admin/payout-list",
  ],
  payout_batches: [
    "/partner-admin/payouts",
    "/partner-admin/payouts/batches",
    "/partner-admin/payouts/engine",
  ],
  payouts: ["/partner-admin/overview", "/partner-admin/payouts", "/partner-admin/payout-list"],
  sales_audit_log: ["/partner-admin/partner-analytics"],
};

const ALL_RESOURCES = [...new Set(Object.values(TABLE_RESOURCES).flat())];

export function salesChangeForTable(table: string): SalesAdminChangeEvent {
  return {
    source: "sales",
    table,
    resources: [...(TABLE_RESOURCES[table] ?? ALL_RESOURCES)],
  };
}

function resyncEvent(): SalesAdminChangeEvent {
  return { source: "sales", resources: ALL_RESOURCES, resync: true };
}

@Injectable()
export class AdminEventsService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(AdminEventsService.name);
  private readonly changes = new Subject<SalesAdminChangeEvent>();
  private listener: ListenerClient | undefined;
  private connecting: Promise<void> | undefined;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private reconnectAttempt = 0;
  private generation = 0;
  private stopping = false;

  constructor(@Inject(SALES_DATABASE) private readonly database: SalesDatabase) {}

  async onModuleInit(): Promise<void> {
    await this.ensureConnected();
  }

  stream(): Observable<MessageEvent> {
    return concat(
      of({ type: "resync", data: resyncEvent() }),
      merge(
        this.changes.pipe(map((data) => ({ type: "change", data }))),
        interval(HEARTBEAT_MS).pipe(
          map(() => ({ type: "heartbeat", data: { source: "sales" } })),
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
      listener.on("notification", (message) => {
        if (message.channel === CHANNEL && message.payload) {
          this.changes.next(salesChangeForTable(message.payload));
        }
      });
      const current = listener;
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

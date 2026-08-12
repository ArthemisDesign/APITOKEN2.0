import "server-only";
import type { PoolClient } from "pg";
import { getDatabase } from "./db";

const CHANNEL = "openkeys_admin_changes";
const MAX_RECONNECT_MS = 30_000;

export type OpenkeysAdminChangeEvent = {
  source: "openkeys";
  resources: string[];
  table?: string;
  resync?: true;
};

const TABLE_RESOURCES: Readonly<Record<string, readonly string[]>> = {
  openkeys_batches: [
    "/openkeys-admin/keys",
    "/openkeys-admin/sellers",
    "/openkeys-admin/paying-keys",
    "/openkeys-admin/lookup",
  ],
  openkeys_keys: [
    "/openkeys-admin/keys",
    "/openkeys-admin/sellers",
    "/openkeys-admin/paying-keys",
    "/openkeys-admin/lookup",
  ],
  openkeys_issuance_jobs: ["/openkeys-admin/keys", "/openkeys-admin/sellers"],
};

const ALL_RESOURCES = [...new Set(Object.values(TABLE_RESOURCES).flat())];

export function openkeysChangeForTable(table: string): OpenkeysAdminChangeEvent {
  return {
    source: "openkeys",
    table,
    resources: [...(TABLE_RESOURCES[table] ?? ALL_RESOURCES)],
  };
}

function resyncEvent(): OpenkeysAdminChangeEvent {
  return { source: "openkeys", resources: ALL_RESOURCES, resync: true };
}

type Subscriber = (event: OpenkeysAdminChangeEvent) => void;

export class OpenkeysAdminChangeFeed {
  private readonly subscribers = new Set<Subscriber>();
  private listener: PoolClient | undefined;
  private connecting: Promise<void> | undefined;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private reconnectAttempt = 0;
  private generation = 0;
  private needsRecoveryResync = false;

  constructor(private readonly acquireListener = () => getDatabase().pool.connect()) {}

  subscribe(subscriber: Subscriber): () => void {
    let active = true;
    this.subscribers.add(subscriber);
    void this.ensureConnected().then(() => {
      if (active) subscriber(resyncEvent());
    });
    return () => {
      active = false;
      this.subscribers.delete(subscriber);
    };
  }

  private publish(event: OpenkeysAdminChangeEvent): void {
    for (const subscriber of this.subscribers) subscriber(event);
  }

  private async ensureConnected(): Promise<void> {
    if (this.listener) return;
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
    if (this.listener) return;
    const generation = ++this.generation;
    let listener: PoolClient | undefined;
    try {
      listener = await this.acquireListener();
      if (generation !== this.generation) {
        listener.release();
        return;
      }
      this.listener = listener;
      listener.on("notification", (message) => {
        if (message.channel === CHANNEL && message.payload) {
          this.publish(openkeysChangeForTable(message.payload));
        }
      });
      const current = listener;
      listener.on("error", () => this.disconnected(current, generation));
      listener.on("end", () => this.disconnected(current, generation));
      await listener.query(`LISTEN ${CHANNEL}`);
      if (generation !== this.generation) {
        this.listener = undefined;
        listener.removeAllListeners();
        listener.release(true);
        return;
      }
      this.reconnectAttempt = 0;
      if (this.needsRecoveryResync) {
        this.needsRecoveryResync = false;
        this.publish(resyncEvent());
      }
    } catch {
      if (listener && this.listener === listener) {
        this.listener = undefined;
        listener.removeAllListeners();
        listener.release(true);
      }
      if (generation === this.generation) {
        this.needsRecoveryResync = true;
        this.scheduleReconnect();
      }
    }
  }

  private disconnected(listener: PoolClient, generation: number): void {
    if (generation !== this.generation || this.listener !== listener) return;
    this.listener = undefined;
    this.needsRecoveryResync = true;
    listener.removeAllListeners();
    listener.release(true);
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    const delay = Math.min(1_000 * 2 ** this.reconnectAttempt, MAX_RECONNECT_MS);
    this.reconnectAttempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      void this.ensureConnected();
    }, delay);
    this.reconnectTimer.unref();
  }
}

const globalFeed = globalThis as typeof globalThis & {
  __openkeysAdminChangeFeed?: OpenkeysAdminChangeFeed;
};

export function getOpenkeysAdminChangeFeed(): OpenkeysAdminChangeFeed {
  globalFeed.__openkeysAdminChangeFeed ??= new OpenkeysAdminChangeFeed();
  return globalFeed.__openkeysAdminChangeFeed;
}

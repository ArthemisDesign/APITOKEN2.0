import { Client } from "pg";

/**
 * LISTEN half of the pricing-control job wakeup (migration 0041 is the NOTIFY half).
 * A dedicated connection listens on `pricing_control_jobs` and calls `onWake` for every
 * committed job insert. LISTEN/NOTIFY is fire-and-forget: a notification emitted while no
 * listener is connected is lost. That is safe here by construction — the periodic worker
 * sweep keeps claiming jobs on its own tick and remains the recovery path, so this
 * listener only removes latency from the common case and changes no durability semantics.
 */

export const PRICING_CONTROL_JOBS_CHANNEL = "pricing_control_jobs";

export interface PricingControlNotifyClient {
  connect(): Promise<void>;
  query(text: string): Promise<unknown>;
  on(event: "notification", listener: (message: { channel: string; payload?: string }) => void): unknown;
  on(event: "error" | "end", listener: (error?: Error) => void): unknown;
  removeAllListeners(): unknown;
  end(): Promise<unknown>;
}

export interface PricingControlNotifyListenerOptions {
  /** Wakeup for every committed pricing-control job insert; the consumer coalesces bursts. */
  onWake: (table: string) => void;
  /** Connection dropped; the periodic sweep covers delivery until the reconnect lands. */
  onError?: (error: Error) => void;
  /** Backoff between reconnect attempts, clamped at the last entry. */
  reconnectDelaysMs?: readonly number[];
  /** Test seam: replaces the real pg Client. */
  clientFactory?: () => PricingControlNotifyClient;
}

const DEFAULT_RECONNECT_DELAYS_MS = [1_000, 5_000, 15_000, 30_000] as const;

export class PricingControlNotifyListener {
  private readonly delays: readonly number[];
  private stopped = false;
  private active: Promise<void> | undefined;
  private client: PricingControlNotifyClient | undefined;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly connectionString: string,
    private readonly options: PricingControlNotifyListenerOptions,
  ) {
    this.delays = options.reconnectDelaysMs ?? DEFAULT_RECONNECT_DELAYS_MS;
  }

  start(): void {
    if (this.active) return;
    this.active = this.loop();
  }

  async stop(): Promise<void> {
    this.stopped = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
    const client = this.client;
    if (client) await client.end().catch(() => undefined);
    await this.active;
  }

  private async loop(): Promise<void> {
    let failures = 0;
    while (!this.stopped) {
      try {
        await this.listenOnce();
        // listenOnce resolves only when the connection ends; treat a clean end like a drop.
      } catch {
        // connection/listen failure — fall through to the backoff below
      }
      if (this.stopped) break;
      const delay = this.delays[Math.min(failures, this.delays.length - 1)];
      failures += 1;
      await new Promise<void>((resolve) => {
        this.reconnectTimer = setTimeout(resolve, delay);
      });
      this.reconnectTimer = undefined;
    }
  }

  private async listenOnce(): Promise<void> {
    const client = this.options.clientFactory?.()
      ?? (new Client({ connectionString: this.connectionString }) as unknown as PricingControlNotifyClient);
    this.client = client;
    try {
      await client.connect();
      client.on("notification", (message) => {
        if (message.channel === PRICING_CONTROL_JOBS_CHANNEL) {
          this.options.onWake(message.payload ?? "");
        }
      });
      await client.query(`LISTEN ${PRICING_CONTROL_JOBS_CHANNEL}`);
      await new Promise<void>((resolve, reject) => {
        client.on("error", (error) => {
          reject(error instanceof Error ? error : new Error("pricing-control listener connection failed"));
        });
        client.on("end", () => resolve());
      });
    } catch (error) {
      // One report per drop, whatever stage failed; the sweep covers delivery until reconnect.
      this.options.onError?.(error instanceof Error ? error : new Error(String(error)));
      throw error;
    } finally {
      client.removeAllListeners();
      await client.end().catch(() => undefined);
      if (this.client === client) this.client = undefined;
    }
  }
}

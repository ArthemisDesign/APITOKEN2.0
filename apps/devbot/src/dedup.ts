import type { FingerprintEntry, TopicKey } from "./state.js";

export const FINGERPRINT_TTL_MS = 48 * 3600 * 1000;
export const WARNING_MIN_EDIT_INTERVAL_MS = 5 * 60 * 1000;
export const STORM_WINDOW_MS = 60_000;
export const STORM_THRESHOLD = 5;
export const STORM_QUIET_MS = 10 * 60 * 1000;

export interface StormStatus {
  /** Индивидуальное сообщение подавлено — сворачиваем в сводку бури. */
  suppressed: boolean;
  /** Буря только что началась — нужно отправить первое сводное сообщение. */
  started: boolean;
  names: string[];
  total: number;
}

/**
 * Fingerprint-store с TTL 48 ч + шторм-коалесцинг.
 * Хранилище fingerprint'ов живёт в state-файле; шторм-состояние — в памяти
 * (потеря при рестарте безопасна: буря просто начнётся заново со сводки).
 */
export class Dedup {
  private recentCritical: { name: string; ts: number }[] = [];
  private stormActive = false;
  private stormLastEventAt = 0;
  private stormNames = new Set<string>();
  private stormTotal = 0;

  constructor(private readonly fingerprints: Record<string, FingerprintEntry>) {}

  /** Запись в пределах TTL и не resolved → это повторный firing, а не новый алерт. */
  lookup(fingerprint: string, now: number): FingerprintEntry | undefined {
    const entry = this.fingerprints[fingerprint];
    if (!entry) return undefined;
    if (now - entry.lastAt >= FINGERPRINT_TTL_MS) return undefined;
    return entry;
  }

  register(
    fingerprint: string,
    entry: { messageId: number; topic: TopicKey; now: number },
  ): FingerprintEntry {
    const created: FingerprintEntry = {
      messageId: entry.messageId,
      topic: entry.topic,
      count: 1,
      firstAt: entry.now,
      lastAt: entry.now,
      // Отправка сообщения = последнее обновление; правка не раньше чем через 5 мин.
      lastEditAt: entry.now,
      resolved: false,
    };
    this.fingerprints[fingerprint] = created;
    return created;
  }

  markRepeat(entry: FingerprintEntry, now: number): void {
    entry.count += 1;
    entry.lastAt = now;
    entry.resolved = false;
  }

  markResolved(entry: FingerprintEntry, now: number): void {
    entry.resolved = true;
    entry.lastAt = now;
  }

  /** Telegram лимитит частоту правок: warning-сообщение правим не чаще раза в 5 мин. */
  warningEditAllowed(entry: FingerprintEntry, now: number): boolean {
    return now - entry.lastEditAt >= WARNING_MIN_EDIT_INTERVAL_MS;
  }

  markEdited(entry: FingerprintEntry, now: number): void {
    entry.lastEditAt = now;
  }

  prune(now: number): void {
    for (const [fingerprint, entry] of Object.entries(this.fingerprints)) {
      if (now - entry.lastAt >= FINGERPRINT_TTL_MS) {
        delete this.fingerprints[fingerprint];
      }
    }
  }

  /**
   * Буря: >STORM_THRESHOLD различных critical за STORM_WINDOW_MS — дальше все
   * critical сворачиваются в одно сводное сообщение до STORM_QUIET_MS тишины.
   */
  trackCritical(alertname: string, now: number): StormStatus {
    if (this.stormActive && now - this.stormLastEventAt > STORM_QUIET_MS) {
      this.stormActive = false;
      this.stormNames = new Set();
      this.stormTotal = 0;
    }
    this.recentCritical = this.recentCritical.filter((item) => now - item.ts <= STORM_WINDOW_MS);
    this.recentCritical.push({ name: alertname, ts: now });
    if (this.stormActive) {
      this.stormLastEventAt = now;
      this.stormNames.add(alertname);
      this.stormTotal += 1;
      return { suppressed: true, started: false, names: [...this.stormNames], total: this.stormTotal };
    }
    const distinct = new Set(this.recentCritical.map((item) => item.name));
    if (distinct.size > STORM_THRESHOLD) {
      this.stormActive = true;
      this.stormLastEventAt = now;
      this.stormNames = distinct;
      this.stormTotal = this.recentCritical.length;
      return { suppressed: true, started: true, names: [...distinct], total: this.stormTotal };
    }
    return { suppressed: false, started: false, names: [], total: 0 };
  }
}

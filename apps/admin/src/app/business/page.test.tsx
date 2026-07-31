import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

// next/link вне рантайма Next подменяем обычной ссылкой.
vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string; style?: unknown }) => (
    <a href={props.href} className={props.className} style={props.style as never}>
      {props.children}
    </a>
  ),
}));

import BusinessPage from "./page";
import {
  deliveryPill,
  discountFromMultiplierBp,
  inviteState,
  isInviteActive,
  parseBoundedInteger,
  reuseIdempotencyKey,
  type BusinessInvite,
} from "./utils";

// Минимальный sessionStorage для node-окружения vitest.
function stubSessionStorage() {
  const store = new Map<string, string>();
  vi.stubGlobal("sessionStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, String(value)),
    removeItem: (key: string) => void store.delete(key),
    clear: () => store.clear(),
  });
  return store;
}

const future = new Date(Date.now() + 86_400_000).toISOString();
const past = new Date(Date.now() - 86_400_000).toISOString();

describe("B2B (business page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<BusinessPage />);
    expect(html).toContain("B2B");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("business utils", () => {
  describe("discountFromMultiplierBp", () => {
    it("переводит multiplier_bp в процент скидки", () => {
      expect(discountFromMultiplierBp(3000)).toBe(70);
      expect(discountFromMultiplierBp(10000)).toBe(0);
      expect(discountFromMultiplierBp(500)).toBe(95);
    });
    it("null/undefined → null (страница покажет тире)", () => {
      expect(discountFromMultiplierBp(null)).toBeNull();
      expect(discountFromMultiplierBp(undefined)).toBeNull();
    });
  });

  describe("parseBoundedInteger", () => {
    it("принимает целые в диапазоне", () => {
      expect(parseBoundedInteger("0", 0, 95)).toBe(0);
      expect(parseBoundedInteger("95", 0, 95)).toBe(95);
      expect(parseBoundedInteger("7", 1, 30)).toBe(7);
    });
    it("отклоняет дробные, границы и мусор", () => {
      expect(parseBoundedInteger("70.5", 0, 95)).toBeNull();
      expect(parseBoundedInteger("-1", 0, 95)).toBeNull();
      expect(parseBoundedInteger("96", 0, 95)).toBeNull();
      expect(parseBoundedInteger("0", 1, 30)).toBeNull();
      expect(parseBoundedInteger("abc", 0, 95)).toBeNull();
      // Пустая строка — как в легаси Number("") === 0: валидна для скидки (0–95),
      // невалидна для срока (1–30).
      expect(parseBoundedInteger("", 0, 95)).toBe(0);
      expect(parseBoundedInteger("", 1, 30)).toBeNull();
    });
  });

  describe("inviteState / isInviteActive", () => {
    const base: BusinessInvite = { id: "inv-1", expires_at: future };
    it("использован / отозван / истёк / активен — в порядке приоритета легаси", () => {
      expect(inviteState({ ...base, consumed_at: past })).toEqual({ label: "использован", kind: "ok" });
      expect(inviteState({ ...base, revoked_at: past })).toEqual({ label: "отозван", kind: "bad" });
      expect(inviteState({ ...base, expires_at: past })).toEqual({ label: "истёк", kind: "bad" });
      expect(inviteState(base)).toEqual({ label: "активен", kind: "warn" });
    });
    it("действия доступны только активному инвайту", () => {
      expect(isInviteActive(base)).toBe(true);
      expect(isInviteActive({ ...base, consumed_at: past })).toBe(false);
      expect(isInviteActive({ ...base, revoked_at: past })).toBe(false);
      expect(isInviteActive({ ...base, expires_at: past })).toBe(false);
    });
  });

  describe("deliveryPill", () => {
    it("инвайт без email — copy only", () => {
      expect(deliveryPill({ id: "i1" })).toEqual({ label: "copy only", kind: "info" });
    });
    it("sent → ok, failed → bad, прочее → warn", () => {
      expect(deliveryPill({ id: "i1", email: "a@b.c", delivery_status: "sent" }).kind).toBe("ok");
      expect(deliveryPill({ id: "i1", email: "a@b.c", delivery_status: "failed" }).kind).toBe("bad");
      expect(deliveryPill({ id: "i1", email: "a@b.c", delivery_status: "queued" }).kind).toBe("warn");
    });
  });

  describe("reuseIdempotencyKey", () => {
    beforeEach(() => {
      stubSessionStorage();
    });
    it("новая подпись → новый ключ, записанный в sessionStorage", () => {
      const store = stubSessionStorage();
      const key = reuseIdempotencyKey("k", "sig-1");
      expect(key).toMatch(/^[0-9a-f-]{36}$/);
      expect(JSON.parse(store.get("k")!)).toEqual({ signature: "sig-1", idempotencyKey: key });
    });
    it("та же подпись → переиспользует сохранённый ключ", () => {
      const first = reuseIdempotencyKey("k", "sig-1");
      const second = reuseIdempotencyKey("k", "sig-1");
      expect(second).toBe(first);
    });
    it("другая подпись → новый ключ", () => {
      const first = reuseIdempotencyKey("k", "sig-1");
      const second = reuseIdempotencyKey("k", "sig-2");
      expect(second).not.toBe(first);
    });
  });
});

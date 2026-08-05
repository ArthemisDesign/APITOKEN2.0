import type { ReactNode } from "react";
import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode }) => <a href={props.href}>{props.children}</a>,
}));

import ProxiesPage from "./page";
import {
  createProxyRenewRequest,
  filterProxyInventory,
  projectProxyInventory,
  projectProxyRenew,
  proxyRenewSummary,
  selectableProxyIds,
  type ProxyInventoryItem,
} from "./lib";

const ITEMS: ProxyInventoryItem[] = [
  {
    inventory_id: "11111111-1111-4111-8111-111111111111",
    proxy_hint: "nl-01…",
    order_hint: "ord-…101",
    provider: "iproyal",
    subscription_plan: "google_ai_pro",
    liveness: "live",
    subscription_expires_at: 1_814_390_400,
    proxy_expires_at: 1_800_086_400,
    binding_status: "bound",
    renewable: true,
    renew_block_code: null,
  },
  {
    inventory_id: "22222222-2222-4222-8222-222222222222",
    proxy_hint: "us-02…",
    order_hint: "ord-…202",
    provider: "other",
    subscription_plan: "google_ai_ultra",
    liveness: "degraded",
    subscription_expires_at: null,
    proxy_expires_at: null,
    binding_status: "mismatch",
    renewable: false,
    renew_block_code: "binding_mismatch",
  },
];

function inventoryPayload(): unknown {
  return {
    schema_version: 1,
    observed_at: 1_800_000_000,
    providers: [{ provider: "iproyal", balance_nano_usd: "12500000000", balance_observed_at: 1_800_000_000, auto_extend_enabled: true }],
    items: ITEMS,
  };
}

describe("Прокси (page)", () => {
  it("SSR показывает загрузку и не запрашивает сеть до mount", () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const html = renderToString(<ProxiesPage />);
    expect(html).toContain("Прокси");
    expect(html).toContain("loading-grid");
    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("proxy inventory", () => {
  it("проецирует только bounded контракт, баланс и auto-extend", () => {
    const projected = projectProxyInventory(inventoryPayload());
    expect(projected.providers[0]).toEqual({
      provider: "iproyal",
      balance_nano_usd: "12500000000",
      balance_observed_at: 1_800_000_000,
      auto_extend_enabled: true,
    });
    expect(projected.items).toEqual(ITEMS);
    expect(JSON.stringify(projected)).not.toMatch(/password|credential|proxy_url|email/i);
  });

  it.each(["credentials", "password", "proxy_url", "proxy_host", "email", "subject", "token", "ip"]) (
    "fail-closed отклоняет secret/full identity поле %s",
    (forbidden) => {
      const payload = inventoryPayload() as Record<string, unknown>;
      payload[forbidden] = "must-never-reach-DOM";
      expect(() => projectProxyInventory(payload)).toThrow("запрещённые приватные поля");
    },
  );

  it("фильтрует и выбирает только renewable строки", () => {
    expect(filterProxyInventory(ITEMS, { query: "ord-…101", provider: "", plan: "", liveness: "", binding: "" })).toEqual([ITEMS[0]]);
    expect(filterProxyInventory(ITEMS, { query: "", provider: "other", plan: "google_ai_ultra", liveness: "degraded", binding: "mismatch" })).toEqual([ITEMS[1]]);
    expect(selectableProxyIds(ITEMS)).toEqual([ITEMS[0].inventory_id]);
  });
});

describe("proxy renew", () => {
  it("single и bulk используют UUID idempotency key и sorted unique IDs", () => {
    const key = "33333333-3333-4333-8333-333333333333";
    expect(createProxyRenewRequest([ITEMS[1].inventory_id, ITEMS[0].inventory_id, ITEMS[1].inventory_id], key)).toEqual({
      idempotency_key: key,
      inventory_ids: [ITEMS[0].inventory_id, ITEMS[1].inventory_id],
    });
    expect(createProxyRenewRequest([ITEMS[0].inventory_id], key).inventory_ids).toHaveLength(1);
    expect(() => createProxyRenewRequest([], key)).toThrow("от 1 до 100");
    expect(() => createProxyRenewRequest([ITEMS[0].inventory_id], "not-a-uuid")).toThrow("должен быть UUID");
  });

  it("сохраняет partial и uncertain по каждой строке", () => {
    const response = projectProxyRenew({
      schema_version: 1,
      idempotency_key: "33333333-3333-4333-8333-333333333333",
      idempotent_replay: false,
      status: "uncertain",
      observed_at: 1_800_000_100,
      results: [
        { inventory_id: ITEMS[0].inventory_id, status: "renewed", proxy_expires_at: 1_802_592_000, result_code: null },
        { inventory_id: ITEMS[1].inventory_id, status: "uncertain", proxy_expires_at: null, result_code: "provider_timeout" },
      ],
    });
    expect(response.status).toBe("uncertain");
    expect(response.results.map((item) => item.status)).toEqual(["renewed", "uncertain"]);
    expect(proxyRenewSummary(response)).toBe("Продлено: 1. Ошибки: 0. Неопределённо: 1.");
  });

  it("не рендерит raw upstream error и отклоняет secrets в renew response", () => {
    expect(() => projectProxyRenew({
      status: "failed",
      credentials: "secret",
      results: [],
    })).toThrow("запрещённые приватные поля");
  });
});

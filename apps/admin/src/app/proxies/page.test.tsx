import { readFileSync } from "node:fs";
import type { ReactNode } from "react";
import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode }) => <a href={props.href}>{props.children}</a>,
}));

import ProxiesPage from "./page";
import {
  classifyExpiryWarning,
  createProxyRenewRequest,
  EXPIRY_WARNING_WINDOW_SECONDS,
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
    account_email: "Owner+nl@example.com",
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
    account_email: "ops_us@example.com",
    proxy_hint: "us-02…",
    order_hint: "ord-…202",
    provider: "other",
    subscription_plan: "google_ai_ultra",
    liveness: "degraded",
    subscription_expires_at: null,
    proxy_expires_at: null,
    binding_status: "bound",
    renewable: false,
    renew_block_code: "liveness_degraded",
  },
];

function inventoryPayload(items: unknown[] = ITEMS): Record<string, unknown> {
  return {
    schema_version: 1,
    observed_at: 1_800_000_000,
    providers: [{ provider: "iproyal", balance_nano_usd: "12500000000", balance_observed_at: 1_800_000_000, auto_extend_enabled: true }],
    items,
  };
}

function inventoryItem(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return { ...ITEMS[0], ...overrides };
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

  it("исходник связывает отдельные warning-классы, аккаунт и 11 колонок", () => {
    const source = readFileSync(new URL("./page.tsx", import.meta.url), "utf8");
    expect(source).toContain('className="left mono proxy-account-email"');
    expect(source).toContain('"proxy-expiry-warning subscription"');
    expect(source).toContain('"proxy-expiry-warning proxy"');
    expect(source).toContain("<EmptyRow columns={11}");
    expect(source).not.toContain('aria-label="Binding"');
    expect(source).not.toContain('<option value="dead">');
    const styles = readFileSync(new URL("../globals.css", import.meta.url), "utf8");
    expect(styles).toContain(".proxy-expiry-warning.subscription");
    expect(styles).toContain(".proxy-expiry-warning.proxy");
    expect(styles).toContain("tr:hover td.proxy-expiry-warning");
    expect(styles).toContain("tr.selected td.proxy-expiry-warning");
    expect(styles).toContain("box-shadow:inset 4px 0 0");
  });
});

describe("proxy inventory", () => {
  it("проецирует bounded контракт с полным account_email, балансом и auto-extend", () => {
    const projected = projectProxyInventory(inventoryPayload());
    expect(projected.providers[0]).toEqual({
      provider: "iproyal",
      balance_nano_usd: "12500000000",
      balance_observed_at: 1_800_000_000,
      auto_extend_enabled: true,
    });
    expect(projected.items).toEqual(ITEMS);
    expect(projected.items[0].account_email).toBe("Owner+nl@example.com");
    expect(JSON.stringify(projected)).not.toMatch(/password|credential|proxy_url/);
  });

  it.each([
    "owner@example.com",
    "OPS_100%+tag-name@sub-domain.example",
    "!#$%&'*+/=?^_`{|}~-@example.com",
    `${"a".repeat(64)}@example.com`,
    `a@${"b".repeat(63)}.example`,
    `${"a".repeat(64)}@${"b".repeat(63)}.${"c".repeat(63)}.${"d".repeat(61)}`,
  ])("принимает producer account_email %s", (account_email) => {
    expect(projectProxyInventory(inventoryPayload([inventoryItem({ account_email })])).items[0].account_email).toBe(account_email);
  });

  it.each([
    "",
    "owner",
    "@example.com",
    "owner@",
    "owner@@example.com",
    ".owner@example.com",
    "owner.@example.com",
    "own..er@example.com",
    "owner name@example.com",
    "owner,tag@example.com",
    "owner@example..com",
    "owner@-example.com",
    "owner@example-.com",
    "owner@example_com",
    "owner@éxample.com",
    "öwner@example.com",
    " owner@example.com",
    "owner@example.com\n",
    `${"a".repeat(65)}@example.com`,
    `a@${"b".repeat(64)}.example`,
    `${"a".repeat(64)}@${"b".repeat(63)}.${"c".repeat(63)}.${"d".repeat(62)}`,
  ])("fail-closed отклоняет account_email %s", (account_email) => {
    expect(() => projectProxyInventory(inventoryPayload([inventoryItem({ account_email })]))).toThrow("некорректный account_email");
  });

  it.each(["credentials", "password", "project", "proxy_url", "proxy_host", "email", "subject", "token", "ip"]) (
    "fail-closed отклоняет secret/full identity поле %s",
    (forbidden) => {
      const payload = inventoryPayload();
      payload[forbidden] = "must-never-reach-DOM";
      expect(() => projectProxyInventory(payload)).toThrow("запрещённые приватные поля");
    },
  );

  it("разрешает account_email только как точный item-level ключ", () => {
    expect(() => projectProxyInventory({ ...inventoryPayload(), account_email: "owner@example.com" })).toThrow("запрещённые приватные поля");
    expect(() => projectProxyInventory(inventoryPayload([inventoryItem({ metadata: { account_email: "owner@example.com" } })]))).toThrow("запрещённые приватные поля");
    expect(() => projectProxyInventory(inventoryPayload([inventoryItem({ Account_Email: "owner@example.com" })]))).toThrow("запрещённые приватные поля");
  });

  it.each([
    { liveness: "dead", binding_status: "bound" },
    { liveness: " dead ", binding_status: "bound" },
    { liveness: "invalid", binding_status: "bound" },
    { liveness: "live", binding_status: "unbound" },
    { liveness: "live", binding_status: "mismatch" },
    { liveness: "live", binding_status: "unknown" },
    { liveness: "live", binding_status: " bound " },
    { liveness: "live", binding_status: "\tbound" },
  ])("consumer скрывает producer regression $liveness/$binding_status", (overrides) => {
    expect(projectProxyInventory(inventoryPayload([inventoryItem(overrides)])).items).toEqual([]);
  });

  it("ищет по email и фильтрует только provider/plan", () => {
    expect(filterProxyInventory(ITEMS, { query: "owner+NL@EXAMPLE", provider: "", plan: "" })).toEqual([ITEMS[0]]);
    expect(filterProxyInventory(ITEMS, { query: "ord-…101", provider: "", plan: "" })).toEqual([ITEMS[0]]);
    expect(filterProxyInventory(ITEMS, { query: "", provider: "other", plan: "google_ai_ultra" })).toEqual([ITEMS[1]]);
    expect(selectableProxyIds(ITEMS)).toEqual([ITEMS[0].inventory_id]);
  });
});

describe("proxy expiry warnings", () => {
  const observedAt = 1_800_000_000;

  it("включает exact 72h boundary, внутри окна и expired", () => {
    expect(classifyExpiryWarning(observedAt + EXPIRY_WARNING_WINDOW_SECONDS, observedAt)).toBe("warning");
    expect(classifyExpiryWarning(observedAt + EXPIRY_WARNING_WINDOW_SECONDS - 1, observedAt)).toBe("warning");
    expect(classifyExpiryWarning(observedAt - 1, observedAt)).toBe("warning");
  });

  it("не включает just outside и null", () => {
    expect(classifyExpiryWarning(observedAt + EXPIRY_WARNING_WINDOW_SECONDS + 1, observedAt)).toBe("none");
    expect(classifyExpiryWarning(null, observedAt)).toBe("none");
  });

  it("использует Date.now fallback только при невалидном observed_at", () => {
    expect(classifyExpiryWarning(1_800_000_000 + EXPIRY_WARNING_WINDOW_SECONDS, null, 1_800_000_000_000)).toBe("warning");
    expect(classifyExpiryWarning(1_800_000_000 + EXPIRY_WARNING_WINDOW_SECONDS + 1, 0, 1_800_000_000_000)).toBe("none");
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

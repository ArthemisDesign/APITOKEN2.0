// @vitest-environment jsdom

import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { api, type AccountView } from "@/lib/api";
import { Credits } from "./credits";

vi.mock("@/components/i18n-provider", () => ({ useI18n: () => ({ language: "en" }) }));
vi.mock("@/lib/api", () => ({ api: { createCheckout: vi.fn().mockResolvedValue({ id: "test", provider: "platega", status: "pending", checkoutUrl: null }) } }));
vi.mock("@/lib/product-analytics", () => ({ checkoutAmountBucket: vi.fn(), trackFirstProductEvent: vi.fn(), trackProductEvent: vi.fn() }));

it("sends the exact USD amount for every payment method and rejects fractional input", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const account = { balanceNano: "0", spentNano: "0", pricing: null } as AccountView;
  try {
    await act(async () => root.render(createElement(Credits, { account, ledger: [], ledgerAvailable: true })));
    const input = container.querySelector<HTMLInputElement>('input[name="topup-amount"]')!;
    const submit = container.querySelector<HTMLButtonElement>(".topup-simple-footer button")!;
    const setInput = async (value: string) => act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(input.value).toBe("100");
    expect(container.querySelector(".tc-currency")).toBeNull();
    expect(container.querySelector(".currency-prefix")?.textContent).toBe("$");
    await setInput("137");
    const methods = container.querySelectorAll<HTMLInputElement>('input[name="topup-payment-method"]');
    expect(methods).toHaveLength(4);
    for (const [index, method] of [2, 11, 12, 13].entries()) {
      await act(async () => methods[index]!.click());
      expect(container.querySelector(".topup-simple-footer strong")?.textContent).toBe("$137");
      await act(async () => submit.click());
      expect(api.createCheckout).toHaveBeenLastCalledWith("137", method);
    }
    for (const invalid of ["", "0", "1.5", "01", "-5", "1e3"]) {
      await setInput(invalid);
      expect(submit.disabled).toBe(true);
      expect(container.querySelector(".topup-simple-footer strong")?.textContent).toBe("—");
    }
    expect(api.createCheckout).toHaveBeenCalledTimes(4);
  } finally {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});

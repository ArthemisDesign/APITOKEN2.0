// Demo-режим для ПРЕВЬЮ (Vercel). Скрипт всегда попадает в бандл, но САМОГЕЙТИТСЯ по хостнейму:
// патчит fetch и показывает дашборд с детерминированными фикстурами ТОЛЬКО на превью-доменах
// (`*.vercel.app`) — на реальном проде `apitoken.sale` он НИЧЕГО не делает (ранний return).
// Это позволяет открыть дашборд/редизайн на превью без бэкенда и без его single-origin CORS.
// Данные — те же, что использует визуальный аудит (тариф Starter 60%, nextTier Builder).
export const DEMO_FIXTURE_SCRIPT = `(() => {
  var forced = ${process.env.NEXT_PUBLIC_DEMO === "1" ? "true" : "false"};
  var onPreview = /(^|\\.)vercel\\.app$/.test(location.hostname);
  if (!forced && !onPreview) return; // на apitoken.sale — не активируется
  const originalFetch = window.fetch.bind(window);
  const apiBase = (${JSON.stringify(process.env.NEXT_PUBLIC_BACKEND_URL ?? "https://backend.apitoken.sale/v1")}).replace(/\\/$/, "");
  const json = (body, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
  const user = { id: "demo-0000-0000-0000-000000000000", email: "demo@apitoken.sale", emailVerified: true, passwordEnabled: false, engineAccountStatus: "active", customerType: "b2c" };
  const account = {
    balanceNano: "4000000000", reservedNano: "0", spentNano: "12000000000", balanceUsd: "4.00", markupBasisPoints: 4000, status: "active",
    pricing: { customerType: "b2c", pricingMode: "progressive", monthStart: "2026-07-01T00:00:00.000Z", tier: "starter", discountPercent: 60, multiplierBp: 4000, spentNano: "12000000000", retentionSpendNano: "0",
      nextTier: { tier: "builder", discountPercent: 65, spendThresholdNano: "25000000000", remainingNano: "13000000000", visibleOfficialUsageUsd: "70.00" } },
  };
  const keys = [{ id: "demo-key", label: "Production", keyMasked: "sk-pool-a5b5\\u2022\\u2022\\u2022\\u2022eeb", status: "active", spentNano: "12000000000", spentUsd: "12.00", createdAt: "2026-07-15T08:30:00.000Z" }];
  const entries = [
    { id: "l1", kind: "charge", amountNano: "185000000", amountUsd: "0.185", keyMasked: "sk-pool-a5b5\\u2022\\u2022\\u2022\\u2022eeb", reference: "req_01K0", balanceAfterNano: "4000000000", timestamp: "1784109600" },
    { id: "l2", kind: "topup", amountNano: "4000000000", amountUsd: "4.00", keyMasked: null, reference: "welcome_credit", balanceAfterNano: "4185000000", timestamp: "1784106000" },
  ];
  window.fetch = (input, init = {}) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (!url || !url.startsWith(apiBase)) return originalFetch(input, init);
    const path = new URL(url).pathname.slice(new URL(apiBase).pathname.length);
    if (path === "/auth/providers") return json({ email: { password: true, verificationRequired: false }, google: { configured: false, enabled: false }, github: { configured: false, enabled: false, emailScope: "" } });
    if (path === "/auth/me") return json({ user });
    if (path === "/account") return json(account);
    if (path === "/api-keys") return json({ keys });
    if (path === "/account/ledger") return json({ entries });
    if (path === "/auth/logout") return Promise.resolve(new Response(null, { status: 204 }));
    return json({ message: "demo: route not mocked" }, 404);
  };
  const mark = () => {
    if (document.getElementById("demo-flag")) return;
    const el = document.createElement("div");
    el.id = "demo-flag";
    el.textContent = "DEMO · образец данных";
    el.style.cssText = "position:fixed;z-index:9999;right:10px;bottom:10px;font:600 11px/1 ui-monospace,monospace;letter-spacing:.04em;color:#fff;background:#3767f0;padding:6px 10px;border-radius:99px;opacity:.85;pointer-events:none";
    document.body.appendChild(el);
  };
  if (document.body) mark(); else window.addEventListener("DOMContentLoaded", mark);
})();`;

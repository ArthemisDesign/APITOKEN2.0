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
  const K = "sk-pool-a5b5\\u2022\\u2022\\u2022\\u2022eeb";
  const nowSec = Math.floor(Date.now() / 1000), DAY = 86400;
  // Списания по дням (для графика Usage over time и разбивки по ключам).
  const plan = [[0,"1.10"],[0,"0.42"],[1,"0.95"],[1,"0.70"],[2,"0.60"],[2,"0.05"],[3,"1.30"],[3,"0.09"],[4,"0.55"],[5,"0.80"],[5,"0.65"],[6,"0.08"],[7,"0.45"],[8,"1.05"],[9,"0.06"]];
  const entries = plan.map((p, i) => ({ id: "c" + i, kind: "charge", amountNano: String(Math.round(parseFloat(p[1]) * 1e9)), amountUsd: p[1], keyMasked: K, reference: "req_" + (1000 + i), balanceAfterNano: null, timestamp: String(nowSec - p[0] * DAY - i * 137) }));
  // Пополнения с discountPercent — для истории пополнений (топап-тиры).
  entries.push(
    { id: "t1", kind: "topup", amountNano: "500000000000", amountUsd: "500.00", keyMasked: null, reference: "cryptomus_9f2c1a", balanceAfterNano: "500000000000", timestamp: String(nowSec - 3 * DAY), discountPercent: 80 },
    { id: "t2", kind: "topup", amountNano: "75000000000", amountUsd: "75.00", keyMasked: null, reference: "cryptomus_4a1e77", balanceAfterNano: "80000000000", timestamp: String(nowSec - 20 * DAY), discountPercent: 70 },
    { id: "t3", kind: "topup", amountNano: "20000000000", amountUsd: "20.00", keyMasked: null, reference: "cryptomus_1c8b90", balanceAfterNano: "22000000000", timestamp: String(nowSec - 38 * DAY), discountPercent: 60 },
    { id: "t4", kind: "topup", amountNano: "4000000000", amountUsd: "4.00", keyMasked: null, reference: "welcome_credit", balanceAfterNano: "4000000000", timestamp: String(nowSec - 45 * DAY), discountPercent: 60 },
  );
  // Полная разбивка токенов по моделям — числа согласованы с корзинами и с KPI (≈$29 офиц / ≈$12 списано).
  const usage = {
    window: "30d", requests: 171, totalOfficialNano: "29250000000", totalChargedNano: "11700000000",
    buckets: {
      input: { tokens: 2500000, officialNano: "9100000000" },
      output: { tokens: 760000, officialNano: "14000000000" },
      cacheRead: { tokens: 7400000, officialNano: "2960000000" },
      cacheWrite: { tokens: 600000, officialNano: "3150000000" },
      webSearch: { requests: 4, officialNano: "40000000" },
    },
    models: [
      { model: "claude-opus-4-8", requests: 42, inputTokens: 1200000, outputTokens: 380000, cacheReadTokens: 4500000, cacheWrite5mTokens: 320000, cacheWrite1hTokens: 40000, webSearchRequests: 3, officialNano: "20180000000", chargedNano: "8072000000" },
      { model: "claude-sonnet-5", requests: 71, inputTokens: 900000, outputTokens: 260000, cacheReadTokens: 2100000, cacheWrite5mTokens: 180000, cacheWrite1hTokens: 0, webSearchRequests: 1, officialNano: "7915000000", chargedNano: "3166000000" },
      { model: "claude-haiku-4-5", requests: 58, inputTokens: 400000, outputTokens: 120000, cacheReadTokens: 800000, cacheWrite5mTokens: 60000, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "1155000000", chargedNano: "462000000" },
    ],
  };
  window.fetch = (input, init = {}) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (!url || !url.startsWith(apiBase)) return originalFetch(input, init);
    const path = new URL(url).pathname.slice(new URL(apiBase).pathname.length);
    if (path === "/auth/providers") return json({ email: { password: true, verificationRequired: false }, google: { configured: false, enabled: false }, github: { configured: false, enabled: false, emailScope: "" } });
    if (path === "/auth/me") return json({ user });
    if (path === "/account") return json(account);
    if (path === "/api-keys") return json({ keys });
    if (path === "/account/ledger") return json({ entries });
    if (path === "/account/usage") return json(usage);
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

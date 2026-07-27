import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const baseUrl = process.env.SITE_URL ?? "http://localhost:3001";
const outputDirectory = path.resolve(process.env.SCREENSHOT_DIR ?? ".artifacts/site-audit");
const auditScope = process.env.AUDIT_SCOPE ?? "site";
const auditFilter = new Set((process.env.AUDIT_FILTER ?? "").split(",").map((value) => value.trim()).filter(Boolean));
const chromeCandidates = [
  process.env.CHROME_PATH,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Chromium.app/Contents/MacOS/Chromium",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

const siteCaptures = [
  ["header-wide-light", "/", 1920, 1080, "light"],
  ["header-laptop-light", "/", 1280, 800, "light"],
  ["header-laptop-authenticated-dark", "/?audit-auth=1", 1280, 800, "dark"],
  ["header-laptop-russian-light", "/", 1280, 800, "light", "ru"],
  ["header-collapse-boundary-light", "/", 1240, 800, "light"],
  ["header-tablet-dark", "/", 768, 1024, "dark"],
  ["header-tablet-menu-open-dark", "/", 768, 1024, "dark", "en", "menu-open"],
  ["header-mobile-light", "/", 390, 844, "light"],
  ["header-mobile-menu-open-light", "/", 390, 844, "light", "en", "menu-open"],
  ["header-mobile-narrow-dark", "/", 320, 700, "dark"],
  ["header-mobile-narrow-menu-open-dark", "/", 320, 700, "dark", "en", "menu-open"],
  ["home-desktop", "/", 1440, 1000, "light"],
  ["home-mobile", "/", 390, 844, "light"],
  ["home-dark", "/", 1440, 1000, "dark"],
  ["home-russian", "/", 1440, 1000, "dark", "ru"],
  ["home-russian-light", "/", 1440, 1000, "light", "ru"],
  ["home-mobile-dark", "/", 390, 844, "dark"],
  ["home-mobile-russian-light", "/", 390, 844, "light", "ru"],
  ["home-mobile-russian-dark", "/", 390, 844, "dark", "ru"],
  ["home-authenticated", "/?audit-auth=1", 1440, 1000, "light"],
  ["plans-desktop", "/plans", 1440, 1000, "light"],
  ["plans-mobile", "/plans", 390, 844, "light"],
  ["plans-dark", "/plans", 1440, 1000, "dark"],
  ["plans-russian", "/plans", 1440, 1000, "light", "ru"],
  ["pricing-cards-light", "/plans", 1440, 1000, "light"],
  ["pricing-cards-dark", "/plans", 1440, 1000, "dark"],
  ["pricing-cards-russian-light", "/plans", 1440, 1000, "light", "ru"],
  ["pricing-cards-russian-dark", "/plans", 1440, 1000, "dark", "ru"],
  ["pricing-cards-mobile-light", "/plans", 390, 844, "light"],
  ["pricing-cards-mobile-dark", "/plans", 390, 844, "dark"],
  ["pricing-cards-mobile-russian-light", "/plans", 390, 844, "light", "ru"],
  ["pricing-cards-mobile-russian-dark", "/plans", 390, 844, "dark", "ru"],
  ["models-desktop", "/models", 1440, 1000, "light"],
  ["models-dark", "/models", 1440, 1000, "dark"],
  ["docs-desktop", "/docs", 1440, 1000, "light"],
  ["docs-dark", "/docs", 1440, 1000, "dark"],
  ["docs-mobile", "/docs", 390, 844, "light"],
  ["docs-mobile-dark", "/docs", 390, 844, "dark"],
  ["docs-mobile-russian", "/docs", 390, 844, "light", "ru"],
  ["integrations-desktop", "/integrations", 1440, 1000, "light"],
  ["integration-guide-desktop", "/int-claude-code", 1440, 1000, "light"],
  ["login-desktop", "/login", 1440, 1000, "light"],
  ["register-desktop", "/register", 1440, 1000, "light"],
  ["register-dark", "/register", 1440, 1000, "dark"],
  ["terms-desktop", "/terms", 1440, 1000, "light"],
  ["terms-dark", "/terms", 1440, 1000, "dark"],
  ["terms-russian", "/terms", 1440, 1000, "light", "ru"],
  ["privacy-desktop", "/privacy", 1440, 1000, "light"],
  ["privacy-dark", "/privacy", 1440, 1000, "dark"],
  ["privacy-russian", "/privacy", 1440, 1000, "light", "ru"],
  ["support-desktop", "/support", 1440, 1000, "light"],
  ["support-mobile", "/support", 390, 844, "light"],
  ["support-dark", "/support", 1440, 1000, "dark"],
  ["support-russian", "/support", 1440, 1000, "light", "ru"],
  ["about-desktop", "/about", 1440, 1000, "light"],
  ["about-mobile-dark", "/about", 390, 844, "dark"],
  ["contacts-desktop", "/contacts", 1440, 1000, "light"],
  ["contacts-mobile-dark", "/contacts", 390, 844, "dark"],
  ["changelog-desktop", "/changelog", 1440, 1000, "light"],
  ["changelog-mobile-dark", "/changelog", 390, 844, "dark"],
  ["blog-desktop", "/blog", 1440, 1000, "light"],
  ["blog-mobile-dark", "/blog", 390, 844, "dark"],
  ["status-desktop", "/status", 1440, 1000, "light"],
  ["status-mobile-dark", "/status", 390, 844, "dark"],
  ["calculator-desktop", "/tools/claude-api-cost-calculator", 1440, 1000, "light"],
  ["calculator-mobile-dark", "/tools/claude-api-cost-calculator", 390, 844, "dark"],
  ["calculator-mobile-language-disabled", "/tools/claude-api-cost-calculator", 390, 844, "light"],
  ["model-detail-desktop", "/models/claude-opus-4-8", 1440, 1000, "light"],
  ["model-detail-mobile-dark", "/models/claude-opus-4-8", 390, 844, "dark"],
  ["integrations-mobile-dark", "/integrations", 390, 844, "dark"],
  ["integration-guide-mobile-dark", "/int-claude-code", 390, 844, "dark"],
  ["login-mobile-dark", "/login", 390, 844, "dark"],
  ["login-mobile-russian", "/login", 390, 844, "light", "ru"],
  ["forgot-password-desktop", "/forgot-password", 1440, 1000, "light"],
  ["forgot-password-mobile-dark", "/forgot-password", 390, 844, "dark"],
  ["reset-password-desktop", "/reset-password", 1440, 1000, "light"],
  ["reset-password-mobile-dark", "/reset-password", 390, 844, "dark"],
  ["verify-email-desktop", "/verify-email", 1440, 1000, "light"],
  ["verify-email-mobile-dark", "/verify-email", 390, 844, "dark"],
  ["oauth-callback-desktop", "/auth/callback", 1440, 1000, "light"],
  ["oauth-callback-mobile-dark", "/auth/callback", 390, 844, "dark"],
  ["learn-index-desktop", "/docs/learn", 1440, 1000, "light"],
  ["learn-index-mobile-dark", "/docs/learn", 390, 844, "dark"],
  ["learn-index-russian-mobile", "/ru/docs/learn", 390, 844, "light", "ru"],
  ["learn-index-korean-mobile", "/ko/docs/learn", 390, 844, "light", "ko"],
  ["learn-index-chinese-mobile", "/zh/docs/learn", 390, 844, "light", "zh-CN"],
  ["learn-article-desktop", "/docs/learn/how-to-buy-claude-api-key", 1440, 1000, "light"],
  ["learn-article-mobile-dark", "/docs/learn/how-to-buy-claude-api-key", 390, 844, "dark"],
  ["learn-article-russian-mobile", "/ru/docs/learn/how-to-buy-claude-api-key", 390, 844, "light", "ru"],
  ["learn-article-korean-mobile", "/ko/docs/learn/how-to-buy-claude-api-key", 390, 844, "light", "ko"],
  ["learn-article-chinese-mobile", "/zh/docs/learn/how-to-buy-claude-api-key", 390, 844, "light", "zh-CN"],
  ["not-found-desktop", "/missing-audit-route", 1440, 1000, "light"],
  ["not-found-mobile-dark", "/missing-audit-route", 390, 844, "dark"],
];

const dashboardCaptures = [
  ["dashboard-overview-wide-light", "/dashboard", 1728, 996, "light"],
  ["dashboard-overview-wide-dark", "/dashboard", 1728, 996, "dark"],
  ["dashboard-overview-wide-russian-light", "/dashboard", 1728, 996, "light", "ru"],
  ["dashboard-overview-wide-russian-dark", "/dashboard", 1728, 996, "dark", "ru"],
  ["dashboard-overview-light", "/dashboard", 1440, 1000, "light"],
  ["dashboard-overview-dark", "/dashboard", 1440, 1000, "dark"],
  ["dashboard-overview-russian-light", "/dashboard", 1440, 1000, "light", "ru"],
  ["dashboard-overview-russian", "/dashboard", 1440, 1000, "dark", "ru"],
  ["dashboard-overview-compact-light", "/dashboard", 1180, 900, "light"],
  ["dashboard-overview-compact-russian-dark", "/dashboard", 1180, 900, "dark", "ru"],
  ["dashboard-overview-tablet-dark", "/dashboard", 900, 1000, "dark"],
  ["dashboard-overview-tablet-russian-light", "/dashboard", 900, 1000, "light", "ru"],
  ["dashboard-keys-light", "/dashboard?view=keys", 1440, 1000, "light"],
  ["dashboard-keys-dark", "/dashboard?view=keys", 1440, 1000, "dark"],
  ["dashboard-keys-russian-light", "/dashboard?view=keys", 1440, 1000, "light", "ru"],
  ["dashboard-keys-russian-dark", "/dashboard?view=keys", 1440, 1000, "dark", "ru"],
  ["dashboard-keys-create-light", "/dashboard?view=keys", 1440, 1000, "light", "en", "key-create-open"],
  ["dashboard-keys-edit-light", "/dashboard?view=keys", 1440, 1000, "light", "en", "key-edit-open"],
  ["dashboard-keys-revoke-dark", "/dashboard?view=keys", 1440, 1000, "dark", "en", "key-revoke-open"],
  ["dashboard-topup-light", "/dashboard?view=credits", 1440, 1000, "light"],
  ["dashboard-topup-dark", "/dashboard?view=credits", 1440, 1000, "dark"],
  ["dashboard-topup-tablet-light", "/dashboard?view=credits", 768, 1024, "light"],
  ["dashboard-topup-mobile-light", "/dashboard?view=credits", 390, 844, "light"],
  ["dashboard-topup-mobile-dark", "/dashboard?view=credits", 390, 844, "dark"],
  ["dashboard-topup-mobile-russian", "/dashboard?view=credits", 390, 844, "light", "ru"],
  ["dashboard-usage-light", "/dashboard?view=usage", 1440, 1000, "light"],
  ["dashboard-usage-dark", "/dashboard?view=usage", 1440, 1000, "dark"],
  ["dashboard-usage-russian-light", "/dashboard?view=usage", 1440, 1000, "light", "ru"],
  ["dashboard-support-dark", "/dashboard?view=support", 1440, 1000, "dark"],
  ["dashboard-support-light", "/dashboard?view=support", 1440, 1000, "light"],
  ["dashboard-promos-light", "/dashboard?view=promos", 1440, 1000, "light"],
  ["dashboard-promos-dark", "/dashboard?view=promos", 1440, 1000, "dark"],
  ["dashboard-profile-light", "/dashboard?view=profile", 1440, 1000, "light"],
  ["dashboard-profile-dark", "/dashboard?view=profile", 1440, 1000, "dark"],
  ["dashboard-overview-mobile", "/dashboard", 390, 844, "light"],
  ["dashboard-overview-mobile-dark", "/dashboard", 390, 844, "dark"],
  ["dashboard-overview-mobile-russian-light", "/dashboard", 390, 844, "light", "ru"],
  ["dashboard-overview-mobile-russian", "/dashboard", 390, 844, "dark", "ru"],
  ["dashboard-keys-mobile-light", "/dashboard?view=keys", 390, 844, "light"],
  ["dashboard-keys-tablet-light", "/dashboard?view=keys", 820, 1000, "light"],
  ["dashboard-keys-mobile-dark", "/dashboard?view=keys", 390, 844, "dark"],
  ["dashboard-keys-mobile-russian-light", "/dashboard?view=keys", 390, 844, "light", "ru"],
  ["dashboard-keys-mobile-russian-dark", "/dashboard?view=keys", 390, 844, "dark", "ru"],
  ["dashboard-keys-create-mobile-dark", "/dashboard?view=keys", 390, 844, "dark", "en", "key-create-open"],
  ["dashboard-keys-edit-mobile-russian-dark", "/dashboard?view=keys", 390, 844, "dark", "ru", "key-edit-open"],
  ["dashboard-usage-mobile-light", "/dashboard?view=usage", 390, 844, "light"],
  ["dashboard-usage-mobile-dark", "/dashboard?view=usage", 390, 844, "dark"],
  ["dashboard-usage-mobile-russian-dark", "/dashboard?view=usage", 390, 844, "dark", "ru"],
  ["dashboard-support-mobile-light", "/dashboard?view=support", 390, 844, "light"],
  ["dashboard-support-mobile-dark", "/dashboard?view=support", 390, 844, "dark"],
  ["dashboard-promos-mobile-light", "/dashboard?view=promos", 390, 844, "light"],
  ["dashboard-promos-mobile-dark", "/dashboard?view=promos", 390, 844, "dark"],
  ["dashboard-profile-mobile-light", "/dashboard?view=profile", 390, 844, "light"],
  ["dashboard-profile-mobile-dark", "/dashboard?view=profile", 390, 844, "dark"],
];

const scopedCaptures = auditScope === "dashboard" ? dashboardCaptures :
  auditScope === "all" ? [...siteCaptures, ...dashboardCaptures] : siteCaptures;
const filteredCaptures = auditFilter.size > 0 ? scopedCaptures.filter(([name]) => auditFilter.has(name)) : scopedCaptures;
const auditStartAt = process.env.AUDIT_START_AT;
const startIndex = auditStartAt ? filteredCaptures.findIndex(([name]) => name === auditStartAt) : 0;
if (auditStartAt && startIndex < 0) throw new Error(`AUDIT_START_AT did not match a capture: ${auditStartAt}`);
const captures = filteredCaptures.slice(startIndex);
const shouldVerifyCredits = process.env.AUDIT_VERIFY_CREDITS === "1" ||
  (process.env.AUDIT_VERIFY_CREDITS !== "0" && captures.some(([name]) => name.startsWith("dashboard-topup-")));
const shouldVerifyKeys = process.env.AUDIT_VERIFY_KEYS === "1" ||
  (process.env.AUDIT_VERIFY_KEYS !== "0" && captures.some(([name]) => name.startsWith("dashboard-keys-")));
const shouldVerifyDocsTheme = process.env.AUDIT_VERIFY_DOCS_THEME === "1" ||
  (process.env.AUDIT_VERIFY_DOCS_THEME !== "0" && captures.some(([name]) => name.startsWith("docs-")));
const shouldVerifyPricing = process.env.AUDIT_VERIFY_PRICING === "1" ||
  (process.env.AUDIT_VERIFY_PRICING !== "0" && captures.some(([name]) => name.startsWith("pricing-cards-")));
const shouldVerifyHero = process.env.AUDIT_VERIFY_HERO === "1" ||
  (process.env.AUDIT_VERIFY_HERO !== "0" && captures.some(([name]) => name.startsWith("home-")));

if (captures.length === 0) throw new Error("No screenshots matched AUDIT_SCOPE/AUDIT_FILTER.");

const dashboardFixtureScript = `(() => {
  const originalFetch = window.fetch.bind(window);
  const json = (body, status = 200) => Promise.resolve(new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  }));
  const user = {
    id: "9d3b0b02-b864-4e77-b690-e3c252c44a9e",
    email: "dashboard.audit@apitoken.sale",
    displayName: "Dashboard Audit",
    emailVerified: true,
    passwordEnabled: false,
    engineAccountStatus: "active",
    customerType: "b2c",
    totpEnabled: true,
  };
  const account = {
    balanceNano: "240170000000",
    reservedNano: "0",
    spentNano: "262752000000",
    balanceUsd: "240.17",
    markupBasisPoints: 4000,
    status: "active",
    pricing: {
      customerType: "b2c",
      pricingMode: "progressive",
      monthStart: "2026-07-01T00:00:00.000Z",
      tier: "starter",
      discountPercent: 60,
      multiplierBp: 4000,
      spentNano: "0",
      retentionSpendNano: "0",
      nextTier: {
        tier: "builder",
        discountPercent: 65,
        spendThresholdNano: "100000000000",
        remainingNano: "100000000000",
        visibleOfficialUsageUsd: "600.43",
      },
    },
  };
  const keys = [{
    id: "3df4f03d-e5e8-4811-bcea-d32e9f6f20c0",
    label: "Production",
    keyMasked: "sk-pool-a5b5••••••••eeb",
    status: "active",
    spentNano: "14000000000",
    spentUsd: "14.00",
    reservedNano: "0",
    spendLimitNano: "15000000000",
    expiresAt: new Date(Date.now() + 30 * 86400000).toISOString(),
    lastUsedAt: new Date(Date.now() - 2 * 3600000).toISOString(),
    createdAt: "2026-07-15T08:30:00.000Z",
  }, {
    id: "2138f7aa-634c-4475-94d9-2cf3ded858ec",
    label: "CI deploy",
    keyMasked: "sk-pool-45e1••••••••bc8",
    status: "active",
    spentNano: "500000000",
    spentUsd: "0.50",
    reservedNano: "0",
    spendLimitNano: null,
    expiresAt: new Date(Date.now() + 3 * 86400000).toISOString(),
    lastUsedAt: new Date(Date.now() - 86400000).toISOString(),
    createdAt: "2026-07-17T11:00:00.000Z",
  }, {
    id: "57206bb3-4fdc-4be2-b3fd-87cd174c401b",
    label: null,
    keyMasked: "sk-pool-f367••••••••94ea",
    status: "active",
    spentNano: "500000000",
    spentUsd: "0.50",
    reservedNano: "0",
    spendLimitNano: "1000000000",
    expiresAt: null,
    lastUsedAt: null,
    createdAt: "2026-07-16T10:10:00.000Z",
  }, {
    id: "a1402825-0b99-42dc-b8ac-5381e1efb47b",
    label: "Legacy bot",
    keyMasked: "sk-pool-b221••••••••f09",
    status: "active",
    spentNano: "250000000",
    spentUsd: "0.25",
    reservedNano: "0",
    spendLimitNano: null,
    expiresAt: new Date(Date.now() + 60 * 86400000).toISOString(),
    lastUsedAt: new Date(Date.now() - 5 * 86400000).toISOString(),
    createdAt: "2026-07-09T09:00:00.000Z",
  }, {
    id: "9c56809f-2c35-49cb-932e-7569ddf0d2e8",
    label: "Staging",
    keyMasked: "sk-pool-741c••••••••19a",
    status: "disabled",
    spentNano: "2500000000",
    spentUsd: "2.50",
    reservedNano: "0",
    spendLimitNano: null,
    expiresAt: null,
    lastUsedAt: "2026-07-10T10:00:00.000Z",
    createdAt: "2026-07-08T14:20:00.000Z",
  }];
  const nowS = Math.floor(Date.now() / 1000), DAY = 86400;
  // реальный формат движка: amountUsd со знаком "$" и 6 знаками (раньше ломал график через Number())
  const chg = [
    [0, "1246000000", "claude-opus-4-8"], [0, "742000000", "claude-sonnet-5"], [0, "180000000", "claude-haiku-4-5-20251001"],
    [1, "918000000", "claude-opus-4-8"], [1, "410000000", "claude-sonnet-5"],
    [2, "655000000", "claude-sonnet-5"], [2, "300000000", "claude-opus-4-8"],
    [3, "1330000000", "claude-opus-4-8"], [3, "520000000", "claude-sonnet-5"], [3, "90000000", "claude-haiku-4-5-20251001"],
    [4, "540000000", "claude-sonnet-5"],
    [6, "805000000", "claude-opus-4-8"], [6, "260000000", "claude-haiku-4-5-20251001"],
    [8, "1050000000", "claude-opus-4-8"], [8, "300000000", "claude-sonnet-5"],
  ];
  const entries = chg.map((c, i) => ({ id: "c" + i, kind: "charge", amountNano: c[1], amountUsd: "$" + (Number(c[1]) / 1e9).toFixed(6), keyMasked: "sk-pool-a5b5••••••••eeb", reference: "req_0" + i, model: c[2], balanceAfterNano: null, timestamp: String(nowS - c[0] * DAY - i * 137) }));
  entries.push({ id: "t0", kind: "topup", amountNano: "12000000000", amountUsd: "$12.000000", discountPercent: 60, keyMasked: null, reference: "cryptomus_9f2c1a", balanceAfterNano: null, timestamp: String(nowS - 3 * DAY) });
  const todayUtc = Math.floor(nowS / DAY) * DAY;
  const usage = {
    window: "30d", sinceTs: nowS - 30 * DAY, untilTs: nowS, requests: 59, totalOfficialNano: "22984893050", totalChargedNano: "9193957220",
    buckets: {
      input: { tokens: 3781269, officialNano: "15124021000" },
      output: { tokens: 15168, officialNano: "228560000" },
      cacheRead: { tokens: 4866858, officialNano: "1840525800" },
      cacheWrite: { tokens: 741129, officialNano: "3041786250" },
      webSearch: { requests: 0, officialNano: "0" },
      unattributedLegacy: { officialNano: "2750000000" },
    },
    models: [
      { model: "claude-opus-4-8", requests: 27, inputTokens: 1890211, outputTokens: 5100, cacheReadTokens: 2256400, cacheWrite5mTokens: 282050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "15219567500", chargedNano: "6087827000" },
      { model: "claude-sonnet-5", requests: 27, inputTokens: 1890954, outputTokens: 5072, cacheReadTokens: 2256400, cacheWrite5mTokens: 282050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "7483549500", chargedNano: "2993419800" },
      { model: "claude-haiku-4-5-20251001", requests: 5, inputTokens: 104, outputTokens: 4996, cacheReadTokens: 354058, cacheWrite5mTokens: 177029, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "281776050", chargedNano: "112710420" },
    ],
    daily: [
      { dayTs: todayUtc - 3 * DAY, requests: 20, officialNano: "8000000000", chargedNano: "3200000000" },
      { dayTs: todayUtc - 2 * DAY, requests: 16, officialNano: "6000000000", chargedNano: "2400000000" },
      { dayTs: todayUtc - DAY, requests: 13, officialNano: "5000000000", chargedNano: "2000000000" },
      { dayTs: todayUtc, requests: 10, officialNano: "3984893050", chargedNano: "1593957220" },
    ],
    keys: [
      { keyMasked: "sk-pool-a5b5••••••••eeb", requests: 45, officialNano: "18000000000", chargedNano: "7200000000" },
      { keyMasked: "sk-pool-45e1••••••••bc8", requests: 14, officialNano: "4984893050", chargedNano: "1993957220" },
    ],
  };
  window.fetch = (input, init = {}) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const parsed = new URL(url, location.origin);
    if (!parsed.pathname.startsWith("/v1/")) return originalFetch(input, init);
    const path = parsed.pathname.slice("/v1".length);
    if (location.search.includes("audit-auth=1") && path === "/auth/me") return json({ user });
    if (location.pathname !== "/dashboard" && location.pathname !== "/ru/dashboard") return originalFetch(input, init);
    if (path === "/auth/me") {
      if ((init.method || "GET").toUpperCase() === "PATCH") {
        user.displayName = JSON.parse(String(init.body || "{}")).displayName || user.displayName;
      }
      return json({ user });
    }
    if (path === "/account") return json(account);
    if (path === "/api-keys") {
      if ((init.method || "GET").toUpperCase() === "POST") {
        window.__auditLastApiKeyCreate = JSON.parse(String(init.body || "{}"));
        return json({ key: "sk-pool-audit-secret", id: "audit-created" });
      }
      return json({ keys });
    }
    const policyMatch = path.match(new RegExp("^/api-keys/([^/]+)/policy$"));
    if (policyMatch && (init.method || "GET").toUpperCase() === "PATCH") {
      const body = JSON.parse(String(init.body || "{}"));
      window.__auditLastApiKeyPolicyUpdate = body;
      window.__auditApiKeyPolicyCalls = (window.__auditApiKeyPolicyCalls || 0) + 1;
      if (window.__auditFailNextApiKeyPolicy) {
        window.__auditFailNextApiKeyPolicy = false;
        return json({ message: "spend limit cannot be below billed and reserved usage" }, 409);
      }
      const key = keys.find((candidate) => candidate.id === decodeURIComponent(policyMatch[1]));
      if (!key) return json({ message: "API key not found" }, 404);
      if (body.spendLimitUsd === null) key.spendLimitNano = null;
      else {
        const [whole = "0", fraction = ""] = String(body.spendLimitUsd).split(".");
        key.spendLimitNano = (BigInt(whole) * 1000000000n + BigInt(fraction.padEnd(9, "0"))).toString();
      }
      key.expiresAt = body.expiresAt;
      return json(key);
    }
    const keyMatch = path.match(new RegExp("^/api-keys/([^/]+)$"));
    if (keyMatch && (init.method || "GET").toUpperCase() === "PATCH") {
      const body = JSON.parse(String(init.body || "{}"));
      window.__auditLastApiKeyRename = body;
      window.__auditApiKeyRenameCalls = (window.__auditApiKeyRenameCalls || 0) + 1;
      const key = keys.find((candidate) => candidate.id === decodeURIComponent(keyMatch[1]));
      if (!key) return json({ message: "API key not found" }, 404);
      key.label = body.label;
      return json(key);
    }
    if (path === "/account/ledger") return json({ entries });
    if (path === "/account/usage") return json(usage);
    if (path === "/auth/logout") return Promise.resolve(new Response(null, { status: 204 }));
    return json({ message: "Fixture route not found" }, 404);
  };
})();`;

async function findChrome() {
  const { access } = await import("node:fs/promises");
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error("Chrome/Chromium was not found. Set CHROME_PATH to its executable.");
}

async function waitForJson(url, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.json();
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw lastError ?? new Error(`Timed out waiting for ${url}`);
}

function createCdpClient(webSocketUrl) {
  const socket = new WebSocket(webSocketUrl);
  const pending = new Map();
  const events = new Map();
  let sequence = 0;

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id);
      clearTimeout(request.timeout);
      if (message.error) request.reject(new Error(message.error.message));
      else request.resolve(message.result);
      return;
    }
    const listeners = events.get(message.method) ?? [];
    listeners.forEach((listener) => listener(message.params));
  });

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });

  return {
    ready,
    send(method, params = {}) {
      const id = ++sequence;
      return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          pending.delete(id);
          reject(new Error(`CDP command timed out: ${method}`));
        }, 30_000);
        pending.set(id, { resolve, reject, timeout });
        socket.send(JSON.stringify({ id, method, params }));
      });
    },
    once(method) {
      return new Promise((resolve) => {
        const listener = (params) => {
          events.set(method, (events.get(method) ?? []).filter((entry) => entry !== listener));
          resolve(params);
        };
        events.set(method, [...(events.get(method) ?? []), listener]);
      });
    },
    close() { socket.close(); },
  };
}

async function capturePage(client, [name, route, width, height, theme, language = "en", state = "default"]) {
  await client.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 600,
    screenWidth: width,
    screenHeight: height,
  });
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('theme', ${JSON.stringify(theme)}); localStorage.setItem('lang', ${JSON.stringify(language)});`,
  });
  const captureUrl = new URL(route, baseUrl);
  // Force a real navigation even when consecutive captures use the same route.
  // Without this cache-buster Chrome can reuse the mounted English page after
  // localStorage is changed, producing a mislabeled language screenshot.
  captureUrl.searchParams.set("__audit", `${name}-${Date.now()}`);
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: captureUrl.href });
  await loaded;
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 500))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `(() => {
      if (document.documentElement.lang === ${JSON.stringify(language)}) return;
      const label = ${JSON.stringify(language.toUpperCase())};
      const control = [...document.querySelectorAll('.lang button, .lang a')]
        .find((button) => button.textContent?.trim() === label);
      control?.click();
    })()`,
  });
  try {
    await waitForCondition(
      client,
      `document.documentElement.lang === ${JSON.stringify(language)}`,
      `${name} language state`,
    );
  } catch (error) {
    const state = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({
        documentLanguage: document.documentElement.lang,
        storedLanguage: localStorage.getItem('lang'),
        controls: [...document.querySelectorAll('.lang button, .lang a')].map((button) => ({
          label: button.textContent?.trim(),
          active: button.classList.contains('active'),
        })),
      })`,
      returnByValue: true,
    });
    throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${state.result.value}`);
  }
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `(async () => {
      document.documentElement.dataset.theme = ${JSON.stringify(theme)};
      await new Promise((resolve) => setTimeout(resolve, 700));
      document.querySelector('.hero')?.classList.add('loaded');
      document.querySelectorAll('[data-reveal], [data-reveal-stagger], .reveal')
        .forEach((element) => element.classList.add('in'));
      document.querySelectorAll('nextjs-portal').forEach((element) => element.remove());
      document.documentElement.style.scrollBehavior = 'auto';
      await document.fonts.ready;
      await new Promise((resolve) => setTimeout(resolve, 850));
      // A language hydration can replace translated reveal nodes after the
      // first pass. Stabilize the final DOM immediately before capture.
      document.querySelector('.hero')?.classList.add('loaded');
      document.querySelectorAll('[data-reveal], [data-reveal-stagger], .reveal')
        .forEach((element) => element.classList.add('in'));
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    })()`,
  });
  if (state === "menu-open") {
    await client.send("Runtime.evaluate", { expression: `document.querySelector('.nav-burger')?.click()` });
    await waitForCondition(client, `document.querySelector('.nav-burger')?.getAttribute('aria-expanded') === 'true'`, `${name} open navigation`);
  }
  if (state === "key-create-open") {
    await clickSelector(client, ".keys-create-button");
    await waitForCondition(client, `Boolean(document.querySelector('.key-modal[role="dialog"]'))`, `${name} create-key dialog`);
  }
  if (state === "key-revoke-open") {
    await client.send("Runtime.evaluate", { expression: `document.querySelector('.key-row')?.scrollIntoView({ block: 'center' })` });
    await client.send("Runtime.evaluate", { awaitPromise: true, expression: `new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))` });
    await clickSelector(client, ".key-menu summary");
    await clickSelector(client, ".key-menu .danger");
    await waitForCondition(client, `Boolean(document.querySelector('.key-revoke-modal[role="alertdialog"]'))`, `${name} revoke-key dialog`);
  }
  if (state === "key-edit-open") {
    await waitForCondition(client, `Boolean(document.querySelector('.key-row .key-menu summary'))`, `${name} API key rows`);
    await client.send("Runtime.evaluate", { expression: `document.querySelector('.key-row')?.scrollIntoView({ block: 'center' })` });
    await client.send("Runtime.evaluate", { awaitPromise: true, expression: `new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))` });
    await clickSelector(client, ".key-row .key-menu summary");
    await clickSelector(client, '.key-row [data-key-action="edit"]');
    await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal[role="dialog"]'))`, `${name} edit-key dialog`);
  }
  const visualStateResult = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const h1 = document.querySelector('h1');
      const header = document.querySelector('header.nav');
      const footer = document.querySelector('footer');
      const modelPriceTable = document.querySelector('.model-detail .tier-table-wrap, .model-detail .model-price-table, main .tier-table-wrap');
      const modelPriceCards = document.querySelector('.model-pricing-mobile');
      const modelPriceRows = (modelPriceTable?.querySelectorAll('tbody tr').length ?? 0) + (modelPriceCards?.querySelectorAll('.model-pricing-card').length ?? 0);
      const notFoundCard = document.querySelector('main .auth-card');
      const notFoundRect = notFoundCard?.getBoundingClientRect();
      const feedback = document.querySelector('.auth-msg');
      const russianControl = [...document.querySelectorAll('.lang button, .lang a')]
        .find((control) => control.textContent?.trim() === 'RU');
      const promoEmpty = document.querySelector('.promo-history .empty-cell');
      const promoEmptyRect = promoEmpty?.getBoundingClientRect();
      const promoContainerRect = promoEmpty?.closest('.table-scroll')?.getBoundingClientRect();
      const overflowElements = [...document.querySelectorAll('body *')].flatMap((element) => {
        const rect = element.getBoundingClientRect();
        return rect.left < -1 || rect.right > innerWidth + 1
          ? [{ tag: element.tagName, className: element.className?.toString().slice(0, 120), left: Math.round(rect.left), right: Math.round(rect.right), width: Math.round(rect.width) }]
          : [];
      }).slice(0, 12);
      return JSON.stringify({
        href: location.href,
        pathname: location.pathname,
        language: document.documentElement.lang,
        overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        bodyWidth: document.body.scrollWidth,
        viewportWidth: document.documentElement.clientWidth,
        h1: h1?.textContent?.trim() ?? '',
        hasMain: Boolean(document.querySelector('main')),
        hasHeader: Boolean(header),
        hasFooter: Boolean(footer),
        modelPriceVisible: Boolean(
          (modelPriceTable && getComputedStyle(modelPriceTable).display !== 'none' && modelPriceTable.getBoundingClientRect().height > 0)
          || (modelPriceCards && getComputedStyle(modelPriceCards).display !== 'none' && modelPriceCards.getBoundingClientRect().height > 0)
        ),
        modelPriceRows,
        notFoundCentered: Boolean(notFoundRect && Math.abs((notFoundRect.left + notFoundRect.width / 2) - innerWidth / 2) < 3 && notFoundRect.top >= 40),
        feedbackClass: feedback?.className ?? '',
        feedbackRole: feedback?.getAttribute('role') ?? '',
        russianUnavailable: russianControl instanceof HTMLButtonElement && russianControl.disabled,
        overflowElements,
        promoEmptyFits: !promoEmpty || Boolean(promoEmptyRect && promoContainerRect && promoEmpty.scrollWidth <= promoEmpty.clientWidth + 1 && promoEmptyRect.left >= promoContainerRect.left - 1 && promoEmptyRect.right <= promoContainerRect.right + 1),
      });
    })()`,
    returnByValue: true,
  });
  const visualState = JSON.parse(visualStateResult.result.value);
  const expectedNotFound = name.startsWith("not-found-");
  if (visualState.overflow > 1 || visualState.bodyWidth > visualState.viewportWidth + 1) {
    throw new Error(`${name} has page-level horizontal overflow: ${JSON.stringify(visualState)}`);
  }
  if (!visualState.hasMain || (!expectedNotFound && /page not found/i.test(visualState.h1))) {
    throw new Error(`${name} rendered the wrong page state: ${JSON.stringify(visualState)}`);
  }
  if (expectedNotFound && (!/page not found/i.test(visualState.h1) || !visualState.notFoundCentered)) {
    throw new Error(`${name} 404 layout is not centered: ${JSON.stringify(visualState)}`);
  }
  if (name === "model-detail-mobile-dark" && (!visualState.modelPriceVisible || visualState.modelPriceRows === 0)) {
    throw new Error(`${name} hides model pricing: ${JSON.stringify(visualState)}`);
  }
  if (name.startsWith("oauth-callback-") && visualState.feedbackClass && (!visualState.feedbackClass.includes("err") || visualState.feedbackRole !== "alert")) {
    throw new Error(`${name} presents a callback failure as success: ${JSON.stringify(visualState)}`);
  }
  if (["about-desktop", "about-mobile-dark", "contacts-desktop", "contacts-mobile-dark", "changelog-desktop", "changelog-mobile-dark", "blog-desktop", "blog-mobile-dark", "status-desktop", "status-mobile-dark", "calculator-desktop", "calculator-mobile-dark", "calculator-mobile-language-disabled", "model-detail-desktop", "model-detail-mobile-dark"].includes(name) && !visualState.russianUnavailable) {
    throw new Error(`${name} offers a Russian route that does not exist: ${JSON.stringify(visualState)}`);
  }
  if (name.startsWith("dashboard-promos-mobile-") && !visualState.promoEmptyFits) {
    throw new Error(`${name} clips its empty promo history state: ${JSON.stringify(visualState)}`);
  }
  const { cssContentSize, contentSize } = await client.send("Page.getLayoutMetrics");
  // Chrome reports the legacy contentSize in physical pixels on Retina displays.
  // cssContentSize keeps the clip in CSS pixels and avoids a half-empty 2x canvas.
  const measuredSize = cssContentSize ?? contentSize;
  const pageHeight = Math.ceil(measuredSize.height);
  const pageWidth = Math.ceil(measuredSize.width);
  const modalState = state === "key-create-open" || state === "key-edit-open" || state === "key-revoke-open";
  const screenshot = await client.send("Page.captureScreenshot", modalState ? {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: false,
  } : {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: true,
    clip: { x: 0, y: 0, width: pageWidth, height: pageHeight, scale: 1 },
  });
  const filename = `${name}.png`;
  await writeFile(path.join(outputDirectory, filename), Buffer.from(screenshot.data, "base64"));
  return { name, route, finalPath: visualState.pathname, theme, language, width: modalState ? width : pageWidth, height: modalState ? height : pageHeight, file: filename };
}

async function waitForCondition(client, expression, description, timeoutMs = 8_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = await client.send("Runtime.evaluate", { expression, returnByValue: true });
    if (result.result.value === true) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out waiting for ${description}.`);
}

async function setViewport(client, width, height) {
  await client.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 600,
    screenWidth: width,
    screenHeight: height,
  });
}

async function clickSelector(client, selector) {
  const result = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      element?.scrollIntoView({ behavior: 'instant', block: 'center', inline: 'center' });
      const rect = element?.getBoundingClientRect();
      return rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })()`,
    returnByValue: true,
  });
  const rect = result.result.value;
  if (!rect) throw new Error(`Browser audit control was not found: ${selector}`);
  await client.send("Page.bringToFront");
  const x = rect.x + rect.width / 2;
  const y = rect.y + rect.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
}

async function verifyServerDocumentLanguages() {
  const cases = [
    ["/", "en"],
    ["/ru/docs", "ru"],
    ["/ko/docs/learn", "ko"],
    ["/zh/docs/learn", "zh-CN"],
  ];
  for (const [route, language] of cases) {
    const response = await fetch(new URL(route, baseUrl), { redirect: "error" });
    const html = await response.text();
    const renderedLanguage = html.match(/<html[^>]*\slang=["']([^"']+)["']/i)?.[1];
    if (!response.ok || renderedLanguage !== language) {
      throw new Error(`Server document language failed for ${route}: ${response.status}, lang=${renderedLanguage ?? "missing"}`);
    }
  }
  process.stdout.write("Verified server-rendered document languages for EN, RU, KO, and ZH routes\n");
}

async function verifyMobileNavigation(client) {
  await setViewport(client, 390, 844);
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/", baseUrl).href });
  await loaded;
  await waitForCondition(client, `Boolean(document.querySelector('.nav-burger'))`, "mobile navigation trigger");
  await clickSelector(client, ".nav-burger");
  await waitForCondition(client, `document.querySelector('.nav-burger')?.getAttribute('aria-expanded') === 'true'`, "open mobile navigation");
  const controls = await client.send("Runtime.evaluate", { expression: `document.querySelector('.nav-burger')?.getAttribute('aria-controls')`, returnByValue: true });
  if (controls.result.value !== "site-navigation") throw new Error("Mobile navigation trigger does not identify its controlled menu.");
  await clickSelector(client, '.nav-links a[href="#how"]');
  await waitForCondition(client, `location.hash === '#how' && document.querySelector('.nav-burger')?.getAttribute('aria-expanded') === 'false'`, "hash navigation to close the mobile menu");
  await clickSelector(client, ".nav-burger");
  await client.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape" });
  await client.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape" });
  await waitForCondition(client, `document.querySelector('.nav-burger')?.getAttribute('aria-expanded') === 'false' && document.activeElement === document.querySelector('.nav-burger')`, "Escape to close and restore mobile-menu focus");
  process.stdout.write("Verified mobile menu hash, Escape, focus, and ARIA behavior\n");
}

async function verifyLearnHubFiltering(client) {
  await setViewport(client, 390, 844);
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/docs/learn", baseUrl).href });
  await loaded;
  await waitForCondition(client, `document.querySelectorAll('.learn-card').length > 20 && Boolean(document.querySelector('.learn-search input'))`, "the searchable Learn hub");
  const initial = await client.send("Runtime.evaluate", { expression: `document.querySelectorAll('.learn-card').length`, returnByValue: true });
  await clickSelector(client, ".learn-search input");
  await client.send("Input.insertText", { text: "Cursor" });
  await waitForCondition(client, `document.querySelectorAll('.learn-card').length > 0 && document.querySelectorAll('.learn-card').length < ${Number(initial.result.value)}`, "Learn search results");
  await clickSelector(client, ".learn-filters button:nth-child(2)");
  await waitForCondition(client, `Boolean(document.querySelector('.learn-empty'))`, "combined Learn search and topic filters");
  await clickSelector(client, ".learn-empty button");
  await waitForCondition(client, `document.querySelectorAll('.learn-card').length === ${Number(initial.result.value)} && document.querySelector('.learn-search input')?.value === ''`, "cleared Learn filters");
  process.stdout.write("Verified Learn search, topics, result count, empty state, and clear action\n");
}

async function verifyHeroOfferLayout(client) {
  const cases = [
    { name: "desktop-light-en", width: 1440, height: 1000, theme: "light", language: "en", label: "Free" },
    { name: "desktop-dark-en", width: 1440, height: 1000, theme: "dark", language: "en", label: "Free" },
    { name: "desktop-light-ru", width: 1440, height: 1000, theme: "light", language: "ru", label: "Бонус" },
    { name: "desktop-dark-ru", width: 1440, height: 1000, theme: "dark", language: "ru", label: "Бонус" },
    { name: "mobile-light-en", width: 390, height: 844, theme: "light", language: "en", label: "Free" },
    { name: "mobile-dark-en", width: 390, height: 844, theme: "dark", language: "en", label: "Free" },
    { name: "mobile-light-ru", width: 390, height: 844, theme: "light", language: "ru", label: "Бонус" },
    { name: "mobile-dark-ru", width: 390, height: 844, theme: "dark", language: "ru", label: "Бонус" },
  ];

  for (const layoutCase of cases) {
    await setViewport(client, layoutCase.width, layoutCase.height);
    await client.send("Runtime.evaluate", {
      expression: `localStorage.setItem('theme', ${JSON.stringify(layoutCase.theme)}); localStorage.setItem('lang', ${JSON.stringify(layoutCase.language)});`,
    });
    const url = new URL("/", baseUrl);
    url.searchParams.set("__auditHero", layoutCase.name);
    const loaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: url.href });
    await loaded;
    await waitForCondition(
      client,
      `Boolean(document.querySelector('.hero-offer .offer-value-table')) && document.querySelectorAll('.hero-offer .ot').length === 3`,
      `${layoutCase.name} hero offer`,
    );
    await client.send("Runtime.evaluate", {
      expression: `(() => {
        if (document.documentElement.lang === ${JSON.stringify(layoutCase.language)}) return;
        const target = [...document.querySelectorAll('.lang button, .lang a')]
          .find((button) => button.textContent?.trim() === ${JSON.stringify(layoutCase.language.toUpperCase())});
        target?.click();
      })()`,
    });
    await waitForCondition(
      client,
      `document.documentElement.lang === ${JSON.stringify(layoutCase.language)} && document.querySelector('.offer-free-head .off-tag')?.textContent?.trim() === ${JSON.stringify(layoutCase.label)}`,
      `${layoutCase.name} localized hero offer`,
    );
    await client.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `document.fonts.ready.then(() => new Promise((resolve) => setTimeout(resolve, 350)))`,
    });

    const result = await client.send("Runtime.evaluate", {
      expression: `(() => {
        const rect = (target) => target?.getBoundingClientRect();
        const card = rect(document.querySelector('.hero-offer'));
        const kicker = rect(document.querySelector('.offer-kicker'));
        const noCard = rect(document.querySelector('.offer-no-card'));
        const mainValue = document.querySelector('.offer-free-head .ofa');
        const label = document.querySelector('.offer-free-head .off-tag');
        const freeBlock = rect(document.querySelector('.offer-free'));
        const rateHead = rect(document.querySelector('.offer-rate-head'));
        const table = rect(document.querySelector('.offer-value-table'));
        const headCells = [...document.querySelectorAll('.offer-table-head>span')].map(rect);
        const rows = [...document.querySelectorAll('.offer-tiers>.ot')];
        const rowRects = rows.map(rect);
        const rowCells = rows.map((row) => [...row.children].map(rect));
        const rowTextFits = rows.every((row) => {
          const rowRect = rect(row);
          return [...row.querySelectorAll('b,i,span')].every((node) => {
            const nodeRect = rect(node);
            return nodeRect.left >= rowRect.left - 1 && nodeRect.right <= rowRect.right + 1;
          });
        });
        const columnCentersAligned = headCells.length === 3 && rowCells.every((cells) => cells.length === 3 && cells.every((cell, index) =>
          Math.abs((cell.left + cell.width / 2) - (headCells[index].left + headCells[index].width / 2)) < 3
        ));
        const rowHeights = rowRects.map((row) => row.height);
        const mainStyle = getComputedStyle(mainValue);
        const labelStyle = getComputedStyle(label);
        return JSON.stringify({
          language: document.documentElement.lang,
          theme: document.documentElement.dataset.theme || 'light',
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          label: label?.textContent?.trim(),
          cardFits: Boolean(card && card.left >= -1 && card.right <= innerWidth + 1),
          compactCard: Boolean(card && card.height >= 390 && card.height <= 610),
          metaAligned: Boolean(kicker && noCard && Math.abs((kicker.top + kicker.height / 2) - (noCard.top + noCard.height / 2)) < 8),
          hierarchy: Number.parseFloat(mainStyle.fontSize) >= 52 && Number.parseFloat(labelStyle.fontSize) <= 12,
          verticalRhythm: Boolean(freeBlock && rateHead && table && freeBlock.bottom < rateHead.top && rateHead.bottom < table.top),
          rowsEqual: rowHeights.length === 3 && Math.max(...rowHeights) - Math.min(...rowHeights) < 2,
          columnCentersAligned,
          rowTextFits,
          values: rows.map((row) => [...row.querySelectorAll('b')].map((node) => node.textContent?.trim())),
          discounts: rows.map((row) => row.querySelector('i')?.textContent?.trim()),
        });
      })()`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    const expectedValues = [["$10", "$25"], ["$100", "$286"], ["$1,000", "$5,000"]];
    const expectedDiscounts = ["−60%", "−65%", "−80%"];
    if (state.language !== layoutCase.language || state.theme !== layoutCase.theme || state.overflow > 1 || state.label !== layoutCase.label || !state.cardFits || !state.compactCard || !state.metaAligned || !state.hierarchy || !state.verticalRhythm || !state.rowsEqual || !state.columnCentersAligned || !state.rowTextFits || JSON.stringify(state.values) !== JSON.stringify(expectedValues) || JSON.stringify(state.discounts) !== JSON.stringify(expectedDiscounts)) {
      throw new Error(`Hero offer ${layoutCase.name} layout failed: ${JSON.stringify(state)}`);
    }
  }
  process.stdout.write("Verified hero offer hierarchy, spacing, value columns, translations, themes, and responsive layout\n");
}

async function verifyPricingCardsLayout(client) {
  const cases = [
    { name: "desktop-light-en", width: 1440, height: 1000, theme: "light", language: "en", rate: "Custom rate" },
    { name: "desktop-dark-en", width: 1440, height: 1000, theme: "dark", language: "en", rate: "Custom rate" },
    { name: "desktop-light-ru", width: 1440, height: 1000, theme: "light", language: "ru", rate: "Особый тариф" },
    { name: "desktop-dark-ru", width: 1440, height: 1000, theme: "dark", language: "ru", rate: "Особый тариф" },
    { name: "mobile-light-en", width: 390, height: 844, theme: "light", language: "en", rate: "Custom rate" },
    { name: "mobile-dark-en", width: 390, height: 844, theme: "dark", language: "en", rate: "Custom rate" },
    { name: "mobile-light-ru", width: 390, height: 844, theme: "light", language: "ru", rate: "Особый тариф" },
    { name: "mobile-dark-ru", width: 390, height: 844, theme: "dark", language: "ru", rate: "Особый тариф" },
  ];

  for (const layoutCase of cases) {
    await setViewport(client, layoutCase.width, layoutCase.height);
    await client.send("Runtime.evaluate", {
      expression: `localStorage.setItem('theme', ${JSON.stringify(layoutCase.theme)}); localStorage.setItem('lang', ${JSON.stringify(layoutCase.language)});`,
    });
    const url = new URL("/plans", baseUrl);
    url.searchParams.set("__auditPricing", layoutCase.name);
    const loaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: url.href });
    await loaded;
    await waitForCondition(
      client,
      `Boolean(document.querySelector('.pricing-intro .topup-live')) && Boolean(document.querySelector('.business-preview-head strong')) && Boolean(document.querySelector('.business-terms'))`,
      `${layoutCase.name} pricing cards`,
    );
    await client.send("Runtime.evaluate", {
      expression: `(() => {
        if (document.documentElement.lang === ${JSON.stringify(layoutCase.language)}) return;
        const target = [...document.querySelectorAll('.lang button, .lang a')]
          .find((button) => button.textContent?.trim() === ${JSON.stringify(layoutCase.language.toUpperCase())});
        target?.click();
      })()`,
    });
    await waitForCondition(
      client,
      `document.documentElement.lang === ${JSON.stringify(layoutCase.language)} && document.querySelector('.business-preview-head strong')?.textContent?.trim() === ${JSON.stringify(layoutCase.rate)}`,
      `${layoutCase.name} localized pricing cards`,
    );
    await client.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `document.fonts.ready.then(() => new Promise((resolve) => setTimeout(resolve, 350)))`,
    });

    const result = await client.send("Runtime.evaluate", {
      expression: `(() => {
        const rect = (selector) => document.querySelector(selector)?.getBoundingClientRect();
        const topup = rect('.topup-card');
        const business = rect('.business-card');
        const topupPanel = rect('.topup-live');
        const businessPanel = rect('.business-preview');
        const topupValue = rect('.topup-preview input');
        const businessValue = rect('.business-preview-head strong');
        const topupDescription = rect('.topup-card>p');
        const businessDescription = rect('.business-card>p');
        const topupCta = rect('.topup-card .btn');
        const businessCta = rect('.business-card .business-status');
        const access = rect('.business-access');
        const terms = [...document.querySelectorAll('.business-terms>div')].map((element) => element.getBoundingClientRect());
        const topupStyle = getComputedStyle(document.querySelector('.topup-card'));
        const businessStyle = getComputedStyle(document.querySelector('.business-card'));
        const businessPanelStyle = getComputedStyle(document.querySelector('.business-preview'));
        const rateStyle = getComputedStyle(document.querySelector('.business-preview-head strong'));
        const stacked = innerWidth <= 900;
        return JSON.stringify({
          language: document.documentElement.lang,
          theme: document.documentElement.dataset.theme || 'light',
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          cardWidthsAligned: Boolean(topup && business && Math.abs(topup.width - business.width) < 2),
          desktopCardsAligned: stacked || Boolean(topup && business && Math.abs(topup.top - business.top) < 2 && Math.abs(topup.height - business.height) < 2),
          panelEdgesAligned: stacked || Boolean(topupPanel && businessPanel && Math.abs(topupPanel.top - businessPanel.top) < 2 && Math.abs(topupPanel.bottom - businessPanel.bottom) < 2),
          mainValuesAligned: stacked || Boolean(topupValue && businessValue && Math.abs((topupValue.top + topupValue.height / 2) - (businessValue.top + businessValue.height / 2)) < 2),
          descriptionsAligned: stacked || Boolean(topupDescription && businessDescription && Math.abs(topupDescription.top - businessDescription.top) < 2),
          compactBusinessPanel: Boolean(businessPanel && businessPanel.height <= 210 && Number.parseFloat(rateStyle.fontSize) <= 34),
          matchingCardSurface: topupStyle.backgroundColor === businessStyle.backgroundColor && topupStyle.borderColor === businessStyle.borderColor,
          distinctInnerSurface: businessPanelStyle.backgroundColor !== businessStyle.backgroundColor && businessPanelStyle.borderRadius !== businessStyle.borderRadius,
          ctasMatch: Boolean(topupCta && businessCta && Math.abs(topupCta.width - businessCta.width) < 2 && Math.abs(topupCta.height - businessCta.height) < 2),
          desktopCtasAligned: stacked || Boolean(topupCta && businessCta && Math.abs(topupCta.bottom - businessCta.bottom) < 2),
          termsFit: terms.length === 2 && terms.every((term) => term.right <= innerWidth + 1) && Math.abs(terms[0].top - terms[1].top) < 2,
          accessFits: Boolean(access && businessPanel && access.right <= businessPanel.right && access.left >= businessPanel.left),
        });
      })()`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    const expectedTheme = layoutCase.theme === "dark" ? "dark" : "light";
    if (state.language !== layoutCase.language || state.theme !== expectedTheme || state.overflow > 1 || !state.cardWidthsAligned || !state.desktopCardsAligned || !state.panelEdgesAligned || !state.mainValuesAligned || !state.descriptionsAligned || !state.compactBusinessPanel || !state.matchingCardSurface || !state.distinctInnerSurface || !state.ctasMatch || !state.desktopCtasAligned || !state.termsFit || !state.accessFits) {
      throw new Error(`Pricing cards ${layoutCase.name} layout failed: ${JSON.stringify(state)}`);
    }
  }
  process.stdout.write("Verified pricing card balance, compact B2B hierarchy, translations, themes, and responsive layout\n");
}

async function verifyCreditsLayout(client) {
  const cases = [
    { name: "desktop", width: 1440, height: 1000, statRows: 1, statusRows: 1, converterRow: true, mobileHistory: false },
    { name: "tablet", width: 768, height: 1024, statRows: 1, statusRows: 3, converterRow: true, mobileHistory: false },
    { name: "mobile", width: 390, height: 844, statRows: 3, statusRows: 3, converterRow: false, mobileHistory: true },
  ];

  for (const layoutCase of cases) {
    await setViewport(client, layoutCase.width, layoutCase.height);
    await client.send("Runtime.evaluate", {
      expression: `localStorage.setItem('theme', 'light'); localStorage.setItem('lang', 'en');`,
    });
    const url = new URL("/dashboard?view=credits", baseUrl);
    url.searchParams.set("__auditCredits", layoutCase.name);
    const loaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: url.href });
    await loaded;
    await waitForCondition(
      client,
      `Boolean(document.querySelector('.credits-stack .topup-convert')) && document.querySelectorAll('.pricing-status-item').length === 3 && Boolean(document.querySelector('.topup-history-table tbody tr'))`,
      `${layoutCase.name} Credits layout`,
    );
    await client.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `new Promise((resolve) => setTimeout(resolve, 500))`,
    });

    const result = await client.send("Runtime.evaluate", {
      expression: `(() => {
        const rects = (selector) => [...document.querySelectorAll(selector)].map((element) => element.getBoundingClientRect());
        const rowCount = (items) => new Set(items.map((rect) => Math.round(rect.top))).size;
        const stats = rects('.credits-stack .tc-stats .ovstat');
        const statuses = rects('.pricing-milestone-status .pricing-status-item');
        const input = document.querySelector('.tc-input')?.getBoundingClientRect();
        const receive = document.querySelector('.tc-receive')?.getBoundingClientRect();
        const rail = ['.credits-stack .tc-stats', '.credits-stack .topup-convert', '.credits-stack .pricing-banner', '.credits-history']
          .map((selector) => document.querySelector(selector)?.getBoundingClientRect())
          .filter(Boolean);
        const history = document.querySelector('.credits-history .table-scroll');
        const historyTable = document.querySelector('.topup-history-table');
        const historyCells = [...document.querySelectorAll('.topup-history-table td:not(.empty-cell)')];
        return JSON.stringify({
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          statRows: rowCount(stats),
          statusRows: rowCount(statuses),
          converterRow: Boolean(input && receive && Math.abs(input.top - receive.top) < 2),
          aligned: rail.length === 4 && Math.max(...rail.map((rect) => rect.left)) - Math.min(...rail.map((rect) => rect.left)) < 2 && Math.max(...rail.map((rect) => rect.right)) - Math.min(...rail.map((rect) => rect.right)) < 2,
          historyFits: Boolean(history && history.scrollWidth <= history.clientWidth + 1),
          mobileHistory: Boolean(historyTable && historyCells.length === 5 && getComputedStyle(historyTable).display === 'block' && historyCells.every((cell) => cell.dataset.label && !['none', '""'].includes(getComputedStyle(cell, '::before').content))),
          receiveText: document.querySelector('.tc-recv-value')?.textContent?.trim(),
        });
      })()`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    if (state.overflow > 1 || state.statRows !== layoutCase.statRows || state.statusRows !== layoutCase.statusRows || state.converterRow !== layoutCase.converterRow) {
      throw new Error(`Credits ${layoutCase.name} responsive layout failed: ${JSON.stringify(state)}`);
    }
    if (layoutCase.name === "desktop" && !state.aligned) {
      throw new Error(`Credits desktop rail is not aligned: ${JSON.stringify(state)}`);
    }
    if (!state.historyFits || state.mobileHistory !== layoutCase.mobileHistory) {
      throw new Error(`Credits ${layoutCase.name} history layout failed: ${JSON.stringify(state)}`);
    }

    if (layoutCase.name === "desktop") {
      await clickSelector(client, '[data-topup-preset="500"]');
      await waitForCondition(
        client,
        `document.querySelector('.tc-field input')?.value === '500' && document.querySelector('.tc-preset.on b')?.textContent?.trim() === '$500'`,
        "the Credits preset to update the converter",
      );
      const updated = await client.send("Runtime.evaluate", {
        expression: `document.querySelector('.tc-recv-value')?.textContent?.trim()`,
        returnByValue: true,
      });
      if (!updated.result.value || updated.result.value === state.receiveText) {
        throw new Error(`The Credits receive value did not update: ${state.receiveText} -> ${updated.result.value}`);
      }
    }
  }
  process.stdout.write("Verified Credits alignment, responsive stacking, history layout, and preset interaction\n");
}

async function verifyApiKeysLayout(client) {
  const cases = [
    { name: "desktop-light-en", width: 1440, height: 1000, theme: "light", language: "en", label: "Filter API keys", disabled: "Revoked" },
    { name: "desktop-dark-en", width: 1440, height: 1000, theme: "dark", language: "en", label: "Filter API keys", disabled: "Revoked" },
    { name: "desktop-light-ru", width: 1440, height: 1000, theme: "light", language: "ru", label: "Фильтр API-ключей", disabled: "Отозван" },
    { name: "desktop-dark-ru", width: 1440, height: 1000, theme: "dark", language: "ru", label: "Фильтр API-ключей", disabled: "Отозван" },
    { name: "tablet-light-en", width: 820, height: 1000, theme: "light", language: "en", label: "Filter API keys", disabled: "Revoked" },
    { name: "mobile-light-en", width: 390, height: 844, theme: "light", language: "en", label: "Filter API keys", disabled: "Revoked" },
    { name: "mobile-dark-en", width: 390, height: 844, theme: "dark", language: "en", label: "Filter API keys", disabled: "Revoked" },
    { name: "mobile-light-ru", width: 390, height: 844, theme: "light", language: "ru", label: "Фильтр API-ключей", disabled: "Отозван" },
    { name: "mobile-dark-ru", width: 390, height: 844, theme: "dark", language: "ru", label: "Фильтр API-ключей", disabled: "Отозван" },
  ];

  for (const layoutCase of cases) {
    await setViewport(client, layoutCase.width, layoutCase.height);
    await client.send("Runtime.evaluate", {
      expression: `localStorage.setItem('theme', ${JSON.stringify(layoutCase.theme)}); localStorage.setItem('lang', ${JSON.stringify(layoutCase.language)});`,
    });
    const url = new URL("/dashboard?view=keys", baseUrl);
    url.searchParams.set("__auditKeys", layoutCase.name);
    const loaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: url.href });
    await loaded;
    try {
      await waitForCondition(
        client,
        `Boolean(document.querySelector('.keys-toolbar')) && document.querySelectorAll('.keys-filter-tab').length === 5 && document.querySelectorAll('.keys-health-card').length === 0 && Boolean(document.querySelector('.lang button, .lang a'))`,
        `${layoutCase.name} API key manager shell`,
      );
    } catch (error) {
      const shellState = await client.send("Runtime.evaluate", {
        expression: `JSON.stringify({ href: location.href, body: document.body.innerText.slice(0,500) })`,
        returnByValue: true,
      });
      throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${shellState.result.value}`);
    }
    await client.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `new Promise((resolve) => setTimeout(resolve, 500))`,
    });
    await client.send("Runtime.evaluate", {
      expression: `(() => {
        if (document.documentElement.lang === ${JSON.stringify(layoutCase.language)}) return;
        const target = [...document.querySelectorAll('.lang button, .lang a')]
          .find((button) => button.textContent?.trim() === ${JSON.stringify(layoutCase.language.toUpperCase())});
        target?.click();
      })()`,
    });
    try {
      await waitForCondition(
        client,
        `document.documentElement.lang === ${JSON.stringify(layoutCase.language)} && Boolean(document.querySelector('.key-row'))`,
        `${layoutCase.name} API key manager`,
      );
    } catch (error) {
      const diagnostic = await client.send("Runtime.evaluate", {
        expression: `JSON.stringify({
          documentLanguage: document.documentElement.lang,
          storedLanguage: localStorage.getItem('lang'),
          href: location.href,
          title: document.title,
          bodyText: document.body.innerText.slice(0, 500),
          loading: Boolean(document.querySelector('.dashboard-loading')),
          guard: Boolean(document.querySelector('.guard')),
          keyRows: document.querySelectorAll('.key-row').length,
          controls: [...document.querySelectorAll('.lang button, .lang a')].map((button) => ({ label: button.textContent?.trim(), active: button.classList.contains('active') })),
        })`,
        returnByValue: true,
      });
      throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${diagnostic.result.value}`);
    }
    await client.send("Runtime.evaluate", { awaitPromise: true, expression: `document.fonts.ready.then(() => new Promise((resolve) => setTimeout(resolve, 250)))` });

    const result = await client.send("Runtime.evaluate", {
      expression: `(() => {
        const rect = (selector) => document.querySelector(selector)?.getBoundingClientRect();
        const heading = rect('.keys-heading-row');
        const dock = rect('.agent-connect-dock');
        const manager = rect('.keys-manager-head');
        const createButton = rect('.keys-create-button');
        const toolbar = rect('.keys-toolbar');
        const tabs = rect('.keys-filter-tabs');
        const keys = rect('.key-table-wrap');
        const key = rect('.key-row');
        const toolbarStyle = getComputedStyle(document.querySelector('.keys-toolbar'));
        const keyStyle = getComputedStyle(document.querySelector(innerWidth <= 900 ? '.key-row' : '.key-table-wrap'));
        const tabRects = [...document.querySelectorAll('.keys-filter-tab')].map((element) => element.getBoundingClientRect());
        return JSON.stringify({
          language: document.documentElement.lang,
          theme: document.documentElement.dataset.theme || 'light',
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          aligned: Boolean(heading && dock && manager && toolbar && keys &&
            Math.max(heading.left, dock.left, manager.left, toolbar.left, keys.left) - Math.min(heading.left, dock.left, manager.left, toolbar.left, keys.left) < 2 &&
            Math.max(dock.right, manager.right, toolbar.right, keys.right) - Math.min(dock.right, manager.right, toolbar.right, keys.right) < 2),
          separated: Boolean(toolbar && key && key.top - toolbar.bottom >= 12),
          distinctSurface: toolbarStyle.backgroundColor !== keyStyle.backgroundColor && toolbarStyle.borderRadius !== keyStyle.borderRadius,
          createButtonCount: document.querySelectorAll('.keys-create-button').length,
          createInManager: Boolean(document.querySelector('.keys-manager-head .keys-create-button')),
          createInHero: Boolean(document.querySelector('.keys-heading-row .keys-create-button')),
          createNearList: Boolean(createButton && manager && createButton.top >= manager.top - 1 && createButton.bottom <= manager.bottom + 1),
          createFullWidthOnMobile: innerWidth > 620 || Boolean(createButton && manager && Math.abs(createButton.width - manager.width) < 2),
          controlsFit: Boolean(tabs && tabs.right <= innerWidth + 1 && document.querySelector('.keys-filter-tabs').scrollWidth <= document.querySelector('.keys-filter-tabs').clientWidth + 1),
          tabRows: new Set(tabRects.map((entry) => Math.round(entry.top))).size,
          equalTabHeights: Math.max(...tabRects.map((entry) => entry.height)) - Math.min(...tabRects.map((entry) => entry.height)) < 2,
          label: document.querySelector('.keys-filter-tabs')?.getAttribute('aria-label'),
          counts: [...document.querySelectorAll('.keys-filter-tab b')].map((element) => element.textContent?.trim()),
          activeFilter: document.querySelector('.keys-filter-tab[aria-pressed="true"]')?.dataset.keyFilter,
          healthCards: document.querySelectorAll('.keys-health-card').length,
          policyStates: ['near-limit','expires-soon','limit','expired'].map((state) => Boolean(document.querySelector('.key-row-' + state))),
        });
      })()`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    const expectedTheme = layoutCase.theme === "dark" ? "dark" : "light";
    const expectedTabRows = layoutCase.width > 620 ? 1 : 3;
    if (state.language !== layoutCase.language || state.theme !== expectedTheme || state.overflow > 1 || !state.aligned || !state.separated || !state.distinctSurface || state.createButtonCount !== 1 || !state.createInManager || state.createInHero || !state.createNearList || !state.createFullWidthOnMobile || !state.controlsFit || state.tabRows !== expectedTabRows || !state.equalTabHeights || state.label !== layoutCase.label || state.counts.join(",") !== "4,2,4,1,5" || state.activeFilter !== "current" || state.healthCards !== 0 || state.policyStates.some((present) => !present)) {
      throw new Error(`API keys ${layoutCase.name} layout failed: ${JSON.stringify(state)}`);
    }

    await clickSelector(client, '[data-key-filter="working"]');
    await waitForCondition(
      client,
      `document.querySelector('[data-key-filter="working"]')?.getAttribute('aria-pressed') === 'true' && document.querySelectorAll('.key-row').length === 2 && !document.querySelector('.key-row-expired,.key-row-limit')`,
      `${layoutCase.name} working-key filter`,
    );
    await clickSelector(client, '[data-key-filter="attention"]');
    await waitForCondition(
      client,
      `document.querySelector('[data-key-filter="attention"]')?.getAttribute('aria-pressed') === 'true' && document.querySelectorAll('.key-row').length === 4`,
      `${layoutCase.name} attention-key filter`,
    );
    await clickSelector(client, '[data-key-filter="disabled"]');
    await waitForCondition(
      client,
      `document.querySelector('[data-key-filter="disabled"]')?.getAttribute('aria-pressed') === 'true' && document.querySelectorAll('.key-row').length === 1 && document.querySelector('.key-row .key-status')?.textContent?.trim() === ${JSON.stringify(layoutCase.disabled)}`,
      `${layoutCase.name} disabled-key filter`,
    );
    await clickSelector(client, '[data-key-filter="all"]');
    await waitForCondition(client, `document.querySelectorAll('.key-row').length === 5`, `${layoutCase.name} all-key filter`);
    await client.send("Runtime.evaluate", { expression: `(() => { const input=document.querySelector('.keys-search input'); const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set; set.call(input,'CI'); input.dispatchEvent(new Event('input',{bubbles:true})); })()` });
    await waitForCondition(client, `document.querySelectorAll('.key-row').length === 1 && document.querySelector('.key-name-cell')?.textContent?.includes('CI')`, `${layoutCase.name} key search`);
    await clickSelector(client, ".keys-create-button");
    await waitForCondition(client, `Boolean(document.querySelector('.key-modal[role="dialog"]'))`, `${layoutCase.name} create dialog`);
    const dialogResult = await client.send("Runtime.evaluate", {
      expression: `(() => { const dialog=document.querySelector('.key-modal'); const r=dialog.getBoundingClientRect(); return JSON.stringify({fields:dialog.querySelectorAll('.key-field').length,left:r.left,right:r.right,top:r.top,bottom:r.bottom,scrollWidth:dialog.scrollWidth,clientWidth:dialog.clientWidth}); })()`,
      returnByValue: true,
    });
    const dialog = JSON.parse(dialogResult.result.value);
    if (dialog.fields < 4 || dialog.left < -1 || dialog.right > layoutCase.width + 1 || dialog.top < -1 || dialog.bottom > layoutCase.height + 1 || dialog.scrollWidth > dialog.clientWidth + 1) {
      throw new Error(`API keys ${layoutCase.name} create dialog failed: ${JSON.stringify(dialog)}`);
    }
    if (layoutCase.name === "desktop-light-en") {
      await client.send("Runtime.evaluate", { expression: `(() => {
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const fill = (selector,value) => { const input=document.querySelector(selector); set.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true})); };
        fill('.key-money-field input','25.50'); fill('.key-field input[type="date"]','2099-01-01'); fill('.key-field .tfa-code','123456');
      })()` });
      await clickSelector(client, ".key-modal-actions .btn-primary");
      try {
        await waitForCondition(client, `Boolean(document.querySelector('.secret-card')) && !document.querySelector('.key-modal')`, `${layoutCase.name} create submission`);
      } catch (error) {
        const diagnostic = await client.send("Runtime.evaluate", {
          expression: `JSON.stringify({
            message: document.querySelector('.key-modal .banner-error')?.textContent?.trim(),
            values: [...document.querySelectorAll('.key-modal input')].map((input) => ({ type: input.type, value: input.value })),
            payload: window.__auditLastApiKeyCreate,
            modalOpen: Boolean(document.querySelector('.key-modal')),
            secretVisible: Boolean(document.querySelector('.secret-card')),
            dockClass: document.querySelector('.agent-connect-dock')?.className,
            dockText: document.querySelector('.agent-connect-dock')?.textContent?.slice(0, 300),
          })`,
          returnByValue: true,
        });
        throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${diagnostic.result.value}`);
      }
      const created = await client.send("Runtime.evaluate", { expression: `JSON.stringify(window.__auditLastApiKeyCreate)`, returnByValue: true });
      const payload = JSON.parse(created.result.value);
      const expectedExpiration = new Date("2099-01-01T23:59:59.999").toISOString();
      if (payload.spendLimitUsd !== "25.50" || payload.expiresAt !== expectedExpiration || payload.totpCode !== "123456") {
        throw new Error(`API keys create payload failed: ${JSON.stringify(payload)}`);
      }
    } else {
      await clickSelector(client, ".key-modal-close");
    }

    await client.send("Runtime.evaluate", { expression: `document.querySelector('.key-row')?.scrollIntoView({ block: 'center' })` });
    await client.send("Runtime.evaluate", { awaitPromise: true, expression: `new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))` });
    await clickSelector(client, ".key-menu summary");
    await waitForCondition(client, `Boolean(document.querySelector('.key-menu[open]'))`, `${layoutCase.name} API key menu opens`);
    await clickSelector(client, ".keys-search input");
    await waitForCondition(client, `!document.querySelector('.key-menu[open]')`, `${layoutCase.name} API key menu outside-click dismissal`);
    await clickSelector(client, ".key-menu summary");
    const editMenuResult = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({
        edit: document.querySelector('.key-row .key-edit-action')?.textContent?.trim(),
        docs: Boolean(document.querySelector('.key-row .key-menu-pop a')),
        extraEditButtons: document.querySelectorAll('.key-row .key-menu-pop button:not(.danger)').length,
      })`,
      returnByValue: true,
    });
    const editMenuItems = JSON.parse(editMenuResult.result.value);
    const expectedEditLabel = layoutCase.language === "ru" ? "Изменить" : "Edit";
    if (editMenuItems.edit !== expectedEditLabel || !editMenuItems.docs || editMenuItems.extraEditButtons !== 0) {
      throw new Error(`API keys ${layoutCase.name} must expose one edit action: ${JSON.stringify(editMenuItems)}`);
    }
    await client.send("Runtime.evaluate", { expression: `document.querySelector('[data-key-action="edit"]')?.click()` });
    await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal[role="dialog"]'))`, `${layoutCase.name} edit dialog`);
    const policyDialogResult = await client.send("Runtime.evaluate", {
      expression: `(() => { const dialog=document.querySelector('.key-edit-modal'); const r=dialog.getBoundingClientRect(); return JSON.stringify({fields:dialog.querySelectorAll('.key-field').length,left:r.left,right:r.right,top:r.top,bottom:r.bottom,scrollWidth:dialog.scrollWidth,clientWidth:dialog.clientWidth,focusInside:dialog.contains(document.activeElement),label:dialog.querySelector('.key-field input:not([type])')?.value,limit:dialog.querySelector('.key-money-field input')?.value,expiry:dialog.querySelector('input[type="date"]')?.value}); })()`,
      returnByValue: true,
    });
    const policyDialog = JSON.parse(policyDialogResult.result.value);
    if (policyDialog.fields !== 3 || policyDialog.left < -1 || policyDialog.right > layoutCase.width + 1 || policyDialog.top < -1 || policyDialog.bottom > layoutCase.height + 1 || policyDialog.scrollWidth > policyDialog.clientWidth + 1 || !policyDialog.focusInside || !policyDialog.label || policyDialog.limit !== "" || !policyDialog.expiry) {
      throw new Error(`API keys ${layoutCase.name} edit dialog failed: ${JSON.stringify(policyDialog)}`);
    }
    if (layoutCase.name === "desktop-light-en") {
      await client.send("Runtime.evaluate", { expression: `(() => {
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const fill = (selector,value) => { const input=document.querySelector(selector); set.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true})); };
        fill('.key-edit-modal .key-money-field input','0.25');
      })()` });
      await clickSelector(client, ".key-edit-modal .key-modal-actions .btn-primary");
      await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal .banner-error[role="alert"]')) && !window.__auditApiKeyPolicyCalls && !window.__auditApiKeyRenameCalls`, `${layoutCase.name} local policy floor`);
      await client.send("Runtime.evaluate", { expression: `(() => {
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const fill = (selector,value) => { const input=document.querySelector(selector); set.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true})); };
        fill('.key-edit-modal .key-money-field input','20.000000001');
      })()` });
      await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal .tfa-code'))`, `${layoutCase.name} policy verification field`);
      await client.send("Runtime.evaluate", { expression: `(() => {
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const fill = (selector,value) => { const input=document.querySelector(selector); set.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true})); };
        fill('.key-edit-modal .key-field input:not([type])','CI Production'); fill('.key-edit-modal input[type="date"]','2099-02-01'); fill('.key-edit-modal .tfa-code','654321');
      })()` });
      await clickSelector(client, ".key-edit-modal .key-modal-actions .btn-primary");
      await waitForCondition(client, `!document.querySelector('.key-edit-modal') && window.__auditApiKeyPolicyCalls === 1 && window.__auditApiKeyRenameCalls === 1`, `${layoutCase.name} edit submission`);
      const updated = await client.send("Runtime.evaluate", { expression: `JSON.stringify(window.__auditLastApiKeyPolicyUpdate)`, returnByValue: true });
      const updatedPayload = JSON.parse(updated.result.value);
      const renamed = await client.send("Runtime.evaluate", { expression: `JSON.stringify(window.__auditLastApiKeyRename)`, returnByValue: true });
      const renamedPayload = JSON.parse(renamed.result.value);
      const expectedPolicyExpiration = new Date("2099-02-01T23:59:59.999").toISOString();
      if (updatedPayload.spendLimitUsd !== "20.000000001" || updatedPayload.expiresAt !== expectedPolicyExpiration || updatedPayload.totpCode !== "654321" || renamedPayload.label !== "CI Production") {
        throw new Error(`API keys edit payload failed: ${JSON.stringify({ updatedPayload, renamedPayload })}`);
      }

      try {
        await waitForCondition(client, `Boolean(document.querySelector('.key-menu summary'))`, `${layoutCase.name} refreshed API key rows`);
      } catch (error) {
        const refreshState = await client.send("Runtime.evaluate", {
          expression: `JSON.stringify({ path: location.pathname + location.search, body: document.body.innerText.slice(0, 1000), error: document.querySelector('.banner-error')?.textContent })`,
          returnByValue: true,
        });
        throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${refreshState.result.value}`);
      }
      await clickSelector(client, ".key-menu summary");
      await client.send("Runtime.evaluate", { expression: `document.querySelector('[data-key-action="edit"]')?.click()` });
      await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal'))`, `${layoutCase.name} reopen edit dialog`);
      await client.send("Runtime.evaluate", { expression: `(() => {
        window.__auditFailNextApiKeyPolicy = true;
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const fill = (selector,value) => { const input=document.querySelector(selector); set.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true})); };
        fill('.key-edit-modal .key-money-field input','21');
      })()` });
      await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal .tfa-code'))`, `${layoutCase.name} conflict verification field`);
      await client.send("Runtime.evaluate", { expression: `(() => {
        const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;
        const input=document.querySelector('.key-edit-modal .tfa-code'); set.call(input,'123456'); input.dispatchEvent(new Event('input',{bubbles:true})); input.dispatchEvent(new Event('change',{bubbles:true}));
      })()` });
      await clickSelector(client, ".key-edit-modal .key-modal-actions .btn-primary");
      await waitForCondition(client, `Boolean(document.querySelector('.key-edit-modal .banner-error[role="alert"]')) && window.__auditApiKeyPolicyCalls === 2 && window.__auditApiKeyRenameCalls === 1`, `${layoutCase.name} policy conflict visibility`);
    }
    await client.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape" });
    await client.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape" });
    await waitForCondition(client, `!document.querySelector('.key-edit-modal') && document.activeElement?.matches('.key-edit-action')`, `${layoutCase.name} edit focus restoration`);

    if (layoutCase.name === "desktop-light-en") {
      await clickSelector(client, ".key-menu summary");
      await client.send("Runtime.evaluate", { expression: `document.querySelector('.key-menu .danger')?.click()` });
      await waitForCondition(client, `Boolean(document.querySelector('.key-revoke-modal'))`, `${layoutCase.name} revoke confirmation`);
      await clickSelector(client, ".key-revoke-modal .btn-danger");
      await waitForCondition(client, `Boolean(document.querySelector('.key-revoke-modal .banner-error[role="alert"]'))`, `${layoutCase.name} revoke error visibility`);
      await client.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape" });
      await client.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape" });
      await waitForCondition(client, `!document.querySelector('.key-revoke-modal') && document.activeElement?.matches('.key-menu summary')`, `${layoutCase.name} revoke focus restoration`);
    }
  }
  process.stdout.write("Verified API key table/cards, mutable policy workflow, search, filters, dialogs, translations, themes, and responsive layout\n");
}

async function verifyDocsTheme(client) {
  await setViewport(client, 1440, 1000);
  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('theme', 'light'); localStorage.setItem('lang', 'en');`,
  });
  const url = new URL("/docs", baseUrl);
  url.searchParams.set("__auditDocsTheme", "1");
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: url.href });
  await loaded;
  await waitForCondition(
    client,
    `document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme' && !document.documentElement.hasAttribute('data-theme')`,
    "the light docs theme",
  );

  const lightResult = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const style = (selector) => getComputedStyle(document.querySelector(selector));
      return JSON.stringify({
        site: style('.docs-site').backgroundColor,
        header: style('.docs-header').backgroundColor,
        code: style('.docs-code-card pre').backgroundColor,
        durations: ['.docs-site', '.docs-header', '.docs-sidebar', '.docs-endpoint', '.docs-notice', '.docs-code-card pre', '.docs-auth-flow', '.docs-checklist', '.docs-footer'].map((selector) => style(selector).transitionDuration),
      });
    })()`,
    returnByValue: true,
  });
  const light = JSON.parse(lightResult.result.value);
  if (light.durations.some((duration) => Number.parseFloat(duration) < 0.3)) {
    throw new Error(`Docs theme surfaces do not use the shared transition timing: ${JSON.stringify(light)}`);
  }

  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to light theme'`,
    "the dark docs theme after a real toggle click",
  );
  await client.send("Runtime.evaluate", { awaitPromise: true, expression: `new Promise((resolve) => setTimeout(resolve, 450))` });
  const darkResult = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const style = (selector) => getComputedStyle(document.querySelector(selector));
      return JSON.stringify({
        site: style('.docs-site').backgroundColor,
        header: style('.docs-header').backgroundColor,
        code: style('.docs-code-card pre').backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  const dark = JSON.parse(darkResult.result.value);
  if (dark.site === light.site || dark.header === light.header || dark.code === light.code) {
    throw new Error(`Docs theme-sensitive surfaces did not change together: ${JSON.stringify({ light, dark })}`);
  }

  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `!document.documentElement.hasAttribute('data-theme') && localStorage.getItem('theme') === 'light' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme'`,
    "the light docs theme after switching back",
  );
  await clickSelector(client, ".theme-tgl");
  await waitForCondition(client, `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark'`, "the persisted dark docs theme");

  const reloaded = client.once("Page.loadEventFired");
  await client.send("Page.reload");
  await reloaded;
  await waitForCondition(
    client,
    `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to light theme'`,
    "the dark docs theme after reload",
  );

  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
  const reducedResult = await client.send("Runtime.evaluate", {
    expression: `JSON.stringify(['.docs-site', '.docs-header', '.docs-sidebar', '.docs-endpoint', '.docs-notice', '.docs-code-card pre', '.docs-auth-flow', '.docs-checklist', '.docs-footer'].map((selector) => getComputedStyle(document.querySelector(selector)).transitionDuration))`,
    returnByValue: true,
  });
  const reducedDurations = JSON.parse(reducedResult.result.value);
  if (reducedDurations.some((duration) => Number.parseFloat(duration) > 0.002)) {
    throw new Error(`Reduced-motion docs transitions are too long: ${JSON.stringify(reducedDurations)}`);
  }
  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `!document.documentElement.hasAttribute('data-theme') && localStorage.getItem('theme') === 'light' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme'`,
    "the reduced-motion docs theme toggle",
  );
  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  process.stdout.write("Verified real docs theme toggles, persistence, shared transitions, and reduced motion\n");
}

async function verifyDashboardRouting(client) {
  await client.send("Runtime.evaluate", { expression: `localStorage.setItem('lang', 'en');` });
  for (const removedView of ["refer", "orders"]) {
    const removedLoaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: new URL(`/dashboard?view=${removedView}`, baseUrl).href });
    await removedLoaded;
    await waitForCondition(
      client,
      `document.querySelector('[data-dashboard-section="overview"]')?.getAttribute('aria-current') === 'page' && Boolean(document.querySelector('.overview-core'))`,
      `the removed ${removedView} route to fall back to Overview`,
    );
  }
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/dashboard?view=credits", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `Boolean(document.querySelector('[data-dashboard-section="overview"]')) && document.querySelector('.p-h1')?.textContent?.trim() === 'Top up balance'`,
    "the reloaded top-up view",
  );
  await client.send("Runtime.evaluate", {
    expression: `document.querySelector('[data-dashboard-section="overview"]')?.click()`,
  });
  try {
    await waitForCondition(
      client,
      `location.pathname === '/dashboard' && location.search === '' && document.querySelector('.app-title')?.textContent?.trim() === 'Overview' && Boolean(document.querySelector('.overview-core'))`,
      "direct navigation from a reloaded subview to Overview",
    );
  } catch (error) {
    const state = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({ href: location.href, heading: document.querySelector('.app-title')?.textContent?.trim(), active: document.querySelector('[data-dashboard-section][aria-current="page"]')?.dataset.dashboardSection })`,
      returnByValue: true,
    });
    throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${state.result.value}`);
  }
  await client.send("Runtime.evaluate", {
    expression: `document.querySelector('[data-dashboard-section="keys"]')?.click()`,
  });
  await waitForCondition(
    client,
    `location.search === '?view=keys' && document.querySelector('.p-h1')?.textContent?.trim() === 'API keys'`,
    "dashboard navigation to API keys",
  );
  await client.send("Runtime.evaluate", { expression: "history.back()" });
  await waitForCondition(
    client,
    `location.pathname === '/dashboard' && location.search === '' && document.querySelector('.app-title')?.textContent?.trim() === 'Overview' && Boolean(document.querySelector('.overview-core'))`,
    "Back navigation to Overview",
  );
  await client.send("Runtime.evaluate", { expression: "history.forward()" });
  await waitForCondition(
    client,
    `location.search === '?view=keys' && document.querySelector('.p-h1')?.textContent?.trim() === 'API keys'`,
    "Forward navigation to API keys",
  );
  process.stdout.write("Verified removed-view fallbacks, reload, direct Overview, and Back/Forward dashboard routing\n");
}

async function verifyProfileBehavior(client) {
  await client.send("Runtime.evaluate", { expression: `localStorage.setItem('lang', 'en');` });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/dashboard?view=profile", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `Boolean(document.querySelector('#profile-display-name')) && Boolean(document.querySelector('.uid-copy-button'))`,
    "the editable profile form",
  );
  await client.send("Browser.grantPermissions", {
    origin: new URL(baseUrl).origin,
    permissions: ["clipboardReadWrite"],
  });
  const before = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('.profile-id-row .set-in');
      const rect = input?.getBoundingClientRect();
      const style = input ? getComputedStyle(input) : null;
      return JSON.stringify({
        value: input?.value,
        disabled: input?.disabled,
        readOnly: input?.readOnly,
        className: input?.className,
        rect: rect && { width: rect.width, height: rect.height },
        border: style?.border,
        background: style?.backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  const copyRect = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const rect = document.querySelector('.uid-copy-button')?.getBoundingClientRect();
      return rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })()`,
    returnByValue: true,
  });
  await client.send("Page.bringToFront");
  const copyX = copyRect.result.value.x + copyRect.result.value.width / 2;
  const copyY = copyRect.result.value.y + copyRect.result.value.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x: copyX, y: copyY, button: "left", clickCount: 1 });
  await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: copyX, y: copyY, button: "left", clickCount: 1 });
  await waitForCondition(
    client,
    `document.querySelector('.uid-copy-button')?.textContent?.trim() === 'Copied'`,
    "independent user-ID copy feedback",
  );
  const after = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('.profile-id-row .set-in');
      const rect = input?.getBoundingClientRect();
      const style = input ? getComputedStyle(input) : null;
      return JSON.stringify({
        value: input?.value,
        disabled: input?.disabled,
        readOnly: input?.readOnly,
        className: input?.className,
        rect: rect && { width: rect.width, height: rect.height },
        border: style?.border,
        background: style?.backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  const state = JSON.parse(before.result.value);
  const afterState = JSON.parse(after.result.value);
  const fieldChanged = state.value !== afterState.value
    || state.disabled !== afterState.disabled
    || state.readOnly !== afterState.readOnly
    || state.className !== afterState.className
    || state.border !== afterState.border
    || state.background !== afterState.background
    || Math.abs((state.rect?.width ?? 0) - (afterState.rect?.width ?? 0)) > 0.5
    || Math.abs((state.rect?.height ?? 0) - (afterState.rect?.height ?? 0)) > 0.5;
  if (fieldChanged) {
    throw new Error(`Copy feedback changed the user-ID field: ${before.result.value} -> ${after.result.value}`);
  }
  if (!state.disabled || !state.readOnly || !state.value) {
    throw new Error(`User ID is not immutable: ${before.result.value}`);
  }
  await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('#profile-display-name');
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(input, 'Updated Dashboard');
      input?.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('.prof-save button')?.click();
    })()`,
  });
  await waitForCondition(
    client,
    `document.querySelector('.side-uinfo b')?.textContent?.trim() === 'Updated Dashboard' && Boolean(document.querySelector('.profile-save-success'))`,
    "the saved display name to update the authenticated profile shell",
  );
  process.stdout.write("Verified editable display name and immutable, independently copied user ID\n");
}

async function verifyPersistentSiteRouting(client) {
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('lang', 'en'); localStorage.setItem('theme', 'dark');`,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `document.documentElement.lang === 'en' && document.documentElement.dataset.theme === 'dark' && Boolean(document.querySelector('header.nav')) && Boolean(document.querySelector('footer')) && Boolean(document.querySelector('.bg-decor'))`,
    "the public site shell",
  );
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 800))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `window.__siteAuditSentinel = 'persistent-site-shell';
      window.__siteAuditHeader = document.querySelector('header.nav');
      window.__siteAuditFooter = document.querySelector('footer');
      window.__siteAuditBackground = document.querySelector('.bg-decor');
      window.__siteAuditAuthChecks = 0;
      window.__siteAuditOriginalFetch = window.fetch;
      window.fetch = (...args) => {
        const input = args[0];
        const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        if (url.includes('/auth/me')) window.__siteAuditAuthChecks += 1;
        return window.__siteAuditOriginalFetch(...args);
      };`,
  });

  const transitions = [
    [`header.nav a[href="/models"]`, "/models"],
    [`header.nav a[href="/integrations"]`, "/integrations"],
    [`.steps a[href="/int-claude-code"]`, "/int-claude-code"],
    [`.auth-back[href="/integrations"]`, "/integrations"],
    [`header.nav a[href="/models"]`, "/models"],
    [`footer a[href="/privacy"]`, "/privacy"],
    [`.compliance-nav a[href="/terms"]`, "/terms"],
    [`.compliance-nav a[href="/support"]`, "/support"],
    [`.compliance-nav a[href="/plans"]`, "/plans"],
    [`header.nav a.brand[href="/"]`, "/"],
  ];

  for (const [selector, pathname] of transitions) {
    const clicked = await client.send("Runtime.evaluate", {
      expression: `(() => { const link = document.querySelector(${JSON.stringify(selector)}); link?.click(); return Boolean(link); })()`,
      returnByValue: true,
    });
    if (!clicked.result.value) throw new Error(`Navigation audit link was not found: ${selector}`);
    await waitForCondition(client, `location.pathname === ${JSON.stringify(pathname)}`, `client navigation to ${pathname}`);
    const result = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({
        sentinel: window.__siteAuditSentinel === 'persistent-site-shell',
        sameHeader: window.__siteAuditHeader === document.querySelector('header.nav'),
        sameFooter: window.__siteAuditFooter === document.querySelector('footer'),
        sameBackground: window.__siteAuditBackground === document.querySelector('.bg-decor'),
        authChecks: window.__siteAuditAuthChecks,
        language: document.documentElement.lang,
        theme: document.documentElement.dataset.theme,
      })`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    // SupportContent performs one intentional identity lookup to personalize its
    // Telegram deep link; the persistent header itself must not remount/refetch.
    if (!state.sentinel || !state.sameHeader || !state.sameFooter || !state.sameBackground || state.authChecks > 1 || state.language !== "en" || state.theme !== "dark") {
      throw new Error(`Public shell changed while navigating to ${pathname}: ${JSON.stringify(state)}`);
    }
  }
  process.stdout.write("Verified persistent public shell across landing, marketing, integration, and compliance routes\n");
}

async function verifyComplianceRouting(client) {
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('lang', 'ru'); localStorage.setItem('theme', 'dark');`,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/ru/privacy", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `document.documentElement.lang === 'ru' && document.documentElement.dataset.theme === 'dark' && document.querySelector('h1')?.textContent?.trim() === 'Политика конфиденциальности'`,
    "the Russian dark-mode compliance state",
  );
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 800))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `window.__complianceHeader = document.querySelector('header.nav');
      window.__complianceAuthText = document.querySelector('.nav-actions')?.textContent;
      window.__complianceAuthChecks = 0;
      window.__complianceOriginalFetch = window.fetch;
      window.fetch = (...args) => {
        const input = args[0];
        const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        if (url.includes('/auth/me')) window.__complianceAuthChecks += 1;
        return window.__complianceOriginalFetch(...args);
      };
      document.querySelector('.compliance-nav a[href="/ru/terms"]')?.click();`,
  });
  await waitForCondition(
    client,
    `location.pathname === '/ru/terms' && document.querySelector('h1')?.textContent?.trim() === 'Пользовательское соглашение'`,
    "client navigation to the User Agreement",
  );
  await client.send("Runtime.evaluate", { expression: `document.querySelector('.compliance-nav a[href="/ru/support"]')?.click()` });
  await waitForCondition(
    client,
    `location.pathname === '/ru/support' && document.querySelector('h1')?.textContent?.trim() === 'apiToken Support'`,
    "client navigation to Support",
  );
  await client.send("Runtime.evaluate", { expression: `document.querySelector('.compliance-nav a[href="/ru/plans"]')?.click()` });
  await waitForCondition(
    client,
    `location.pathname === '/ru/plans' && document.querySelector('h1')?.textContent?.trim() === 'Тарифы и цены'`,
    "client navigation to Pricing",
  );
  const result = await client.send("Runtime.evaluate", {
    expression: `JSON.stringify({
      sameHeader: window.__complianceHeader === document.querySelector('header.nav'),
      authChecks: window.__complianceAuthChecks,
      sameAuthText: window.__complianceAuthText === document.querySelector('.nav-actions')?.textContent,
      language: document.documentElement.lang,
      storedLanguage: localStorage.getItem('lang'),
      theme: document.documentElement.dataset.theme,
      storedTheme: localStorage.getItem('theme'),
    })`,
    returnByValue: true,
  });
  const state = JSON.parse(result.result.value);
  if (!state.sameHeader || state.authChecks !== 1 || !state.sameAuthText || state.language !== "ru" || state.storedLanguage !== "ru" || state.theme !== "dark" || state.storedTheme !== "dark") {
    throw new Error(`Compliance shell was not preserved: ${JSON.stringify(state)}`);
  }
  process.stdout.write("Verified persistent compliance shell, language, theme, and authentication menu state\n");
}

const chrome = await findChrome();
const port = 9222 + Math.floor(Math.random() * 500);
await mkdir(outputDirectory, { recursive: true });
await verifyServerDocumentLanguages();

const browser = spawn(chrome, [
  "--headless=new",
  "--disable-gpu",
  "--hide-scrollbars",
  "--no-first-run",
  "--no-default-browser-check",
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${path.join(outputDirectory, ".chrome-profile")}`,
  "about:blank",
], { stdio: "ignore" });

try {
  await waitForJson(`http://127.0.0.1:${port}/json/version`);
  const targetResponse = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" });
  const target = await targetResponse.json();
  const client = createCdpClient(target.webSocketDebuggerUrl);
  await client.ready;
  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("Page.addScriptToEvaluateOnNewDocument", { source: dashboardFixtureScript });
  const warmupLoaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: baseUrl });
  await warmupLoaded;

  const manifest = [];
  for (const capture of captures) {
    manifest.push(await capturePage(client, capture));
    process.stdout.write(`Captured ${capture[0]}\n`);
  }
  if (process.env.AUDIT_VERIFY_ROUTING === "1") await verifyDashboardRouting(client);
  if (process.env.AUDIT_VERIFY_PROFILE === "1") await verifyProfileBehavior(client);
  if (shouldVerifyHero) await verifyHeroOfferLayout(client);
  if (shouldVerifyPricing) await verifyPricingCardsLayout(client);
  if (shouldVerifyKeys) await verifyApiKeysLayout(client);
  if (shouldVerifyCredits) await verifyCreditsLayout(client);
  if (shouldVerifyDocsTheme) await verifyDocsTheme(client);
  if (captures.some(([name]) => name.startsWith("header-mobile"))) await verifyMobileNavigation(client);
  if (captures.some(([name]) => name.startsWith("learn-index-"))) await verifyLearnHubFiltering(client);
  if (process.env.AUDIT_VERIFY_SITE_ROUTING === "1") await verifyPersistentSiteRouting(client);
  if (process.env.AUDIT_VERIFY_COMPLIANCE === "1") await verifyComplianceRouting(client);
  await writeFile(path.join(outputDirectory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  client.close();
  process.stdout.write(`Screenshots: ${outputDirectory}\n`);
} finally {
  browser.kill("SIGTERM");
}

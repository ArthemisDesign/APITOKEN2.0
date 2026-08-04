// Превью-слой web/v2: на Vercel-превью (VERCEL_ENV=preview) дашборд работает «уже внутри» —
// request() из api.ts отдаёт эти фикстуры вместо сети. Бэкенд пускает единственный browser-origin
// (прод-домен), поэтому живой логин с превью невозможен by design; данные ниже — копия
// детерминированных фикстур визуального аудита (scripts/capture-site.mjs), но с состоянием:
// мутации (ключи, профиль, промо, чекаут) меняют его в памяти вкладки, чтобы UI-потоки были живыми.
// В прод-сборке флаг NEXT_PUBLIC_PREVIEW_FIXTURES пуст и этот модуль не загружается вообще.
import {
  ApiError,
  type AccountView,
  type ApiKeyView,
  type AuthUser,
  type CheckoutView,
  type LedgerEntry,
  type ProviderStatus,
  type TotpSetup,
  type UsageView,
} from "./api";

const DAY_MS = 86_400_000;
const NANO = 1_000_000_000n;

const user: AuthUser = {
  id: "9d3b0b02-b864-4e77-b690-e3c252c44a9e",
  email: "v2.preview@apitoken.sale",
  displayName: "V2 Preview",
  emailVerified: true,
  passwordEnabled: true,
  engineAccountStatus: "active",
  customerType: "b2c",
  totpEnabled: false,
};

let balanceNano = 240_170_000_000n;

const keys: ApiKeyView[] = [
  {
    id: "3df4f03d-e5e8-4811-bcea-d32e9f6f20c0",
    label: "Production",
    keyMasked: "sk-pool-a5b5••••••••eeb",
    status: "active",
    spentNano: "14000000000",
    spentUsd: "14.00",
    reservedNano: "0",
    spendLimitNano: "15000000000",
    expiresAt: new Date(Date.now() + 30 * DAY_MS).toISOString(),
    lastUsedAt: new Date(Date.now() - 2 * 3_600_000).toISOString(),
    createdAt: "2026-07-15T08:30:00.000Z",
  },
  {
    id: "2138f7aa-634c-4475-94d9-2cf3ded858ec",
    label: "CI deploy",
    keyMasked: "sk-pool-45e1••••••••bc8",
    status: "active",
    spentNano: "500000000",
    spentUsd: "0.50",
    reservedNano: "0",
    spendLimitNano: null,
    expiresAt: new Date(Date.now() + 3 * DAY_MS).toISOString(),
    lastUsedAt: new Date(Date.now() - DAY_MS).toISOString(),
    createdAt: "2026-07-17T11:00:00.000Z",
  },
  {
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
  },
  {
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
  },
];

function account(): AccountView {
  return {
    balanceNano: balanceNano.toString(),
    reservedNano: "0",
    spentNano: "262752000000",
    balanceUsd: (Number(balanceNano) / 1e9).toFixed(2),
    markupBasisPoints: 5000,
    status: "active",
    funding: {
      accountClass: "b2c",
      fundingEnforcement: "strict",
      reconciliationState: "verified",
      bucketCount: "2",
      balances: {
        paidNano: (balanceNano - 4n * NANO).toString(),
        bonusNano: (4n * NANO).toString(),
        otherNano: "0",
        unattributedNano: "0",
      },
      reserved: { paidNano: "0", bonusNano: "0", otherNano: "0", unattributedNano: "0" },
      spent: { paidNano: "250000000000", bonusNano: "12752000000", otherNano: "0", unattributedNano: "0" },
    },
    pricing: {
      customerType: "b2c",
      pricingMode: "progressive",
      discountPercent: 60,
      multiplierBp: 4000,
    },
    pricingPolicies: [{
      accountClass: "b2c",
      productId: "main",
      policyEnforcement: "strict",
      fundingEnforcement: "strict",
      reconciliationState: "verified",
      syncState: "confirmed",
      inSync: true,
      lastAcknowledgedAt: new Date().toISOString(),
      desired: policyVersion(),
      applied: policyVersion(),
    }],
  };
}

function policyVersion() {
  const trackRule = (ruleId: string) => ({
    ruleId,
    scope: "provider" as const,
    pricingMode: "track" as const,
    ruleOrigin: "managed" as const,
    discountBps: null,
    payableMultiplierBp: 4000,
    trackEligible: true,
    retentionEligible: true,
    commissionEligible: true,
  });
  const staticRule = (ruleId: string) => ({
    ruleId,
    scope: "provider" as const,
    pricingMode: "discount" as const,
    ruleOrigin: "managed" as const,
    discountBps: 5000,
    payableMultiplierBp: 5000,
    trackEligible: false,
    retentionEligible: false,
    commissionEligible: false,
  });
  return {
    effectiveVersion: "3",
    policyVersion: "2",
    catalogGeneration: "1",
    switchGeneration: "1",
    providers: [{
      providerId: "anthropic",
      available: true,
      models: ["claude-opus-4-8", "claude-sonnet-5", "claude-haiku-4-5-20251001"].map((modelId) => ({
        modelId, available: true, unavailableReasons: [], rule: trackRule("anthropic-track"),
      })),
    }, {
      providerId: "openai",
      available: true,
      models: ["gpt-5.6-sol", "gpt-5.6-luna"].map((modelId) => ({
        modelId, available: true, unavailableReasons: [], rule: staticRule("openai-static"),
      })),
    }],
  };
}

const nowS = Math.floor(Date.now() / 1000);
const DAY_S = 86_400;
// Реальный формат движка: amountUsd со знаком "$" и 6 знаками, timestamp — секунды строкой.
const charges: Array<[number, string, string]> = [
  [0, "1246000000", "claude-opus-4-8"], [0, "742000000", "claude-sonnet-5"], [0, "180000000", "claude-haiku-4-5-20251001"], [0, "1390000000", "gpt-5.6-sol"],
  [1, "918000000", "claude-opus-4-8"], [1, "410000000", "claude-sonnet-5"],
  [2, "655000000", "claude-sonnet-5"], [2, "300000000", "claude-opus-4-8"], [2, "88000000", "gpt-5.6-luna"],
  [3, "1330000000", "claude-opus-4-8"], [3, "520000000", "claude-sonnet-5"], [3, "90000000", "claude-haiku-4-5-20251001"],
  [4, "540000000", "claude-sonnet-5"],
  [6, "805000000", "claude-opus-4-8"], [6, "260000000", "claude-haiku-4-5-20251001"],
  [8, "1050000000", "claude-opus-4-8"], [8, "300000000", "claude-sonnet-5"],
];
const ledger: LedgerEntry[] = charges.map(([daysAgo, amountNano, model], i) => ({
  id: `c${i}`,
  kind: "charge",
  amountNano,
  amountUsd: `$${(Number(amountNano) / 1e9).toFixed(6)}`,
  keyMasked: "sk-pool-a5b5••••••••eeb",
  reference: `req_0${i}`,
  model,
  requestId: `request-preview-${i}`,
  provider: model.startsWith("gpt-") ? "openai" : "anthropic",
  officialNano: (BigInt(amountNano) * 2n).toString(),
  attribution: null,
  fundingAllocations: [],
  balanceAfterNano: null,
  timestamp: String(nowS - daysAgo * DAY_S - i * 137),
}));
ledger.push({
  id: "t0",
  kind: "topup",
  amountNano: "12000000000",
  amountUsd: "$12.000000",
  keyMasked: null,
  reference: "platega_9f2c1a",
  requestId: null,
  provider: null,
  officialNano: null,
  attribution: null,
  fundingAllocations: [],
  balanceAfterNano: null,
  timestamp: String(nowS - 3 * DAY_S),
});

function usage(window: string): UsageView {
  const todayUtc = Math.floor(nowS / DAY_S) * DAY_S;
  return {
    window,
    sinceTs: nowS - 30 * DAY_S,
    untilTs: nowS,
    requests: 71,
    totalOfficialNano: "26679893050",
    totalChargedNano: "10671957220",
    buckets: {
      input: { tokens: 4_301_269, officialNano: "16574021000" },
      output: { tokens: 95_168, officialNano: "2148560000" },
      cacheRead: { tokens: 5_016_858, officialNano: "1915525800" },
      cacheWrite: { tokens: 781_129, officialNano: "3291786250" },
      webSearch: { requests: 0, officialNano: "0" },
      unattributedLegacy: { officialNano: "2750000000" },
    },
    models: [
      { model: "claude-opus-4-8", provider: "anthropic", requests: 27, inputTokens: 1_890_211, outputTokens: 5_100, cacheReadTokens: 2_256_400, cacheWrite5mTokens: 282_050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "15219567500", chargedNano: "6087827000" },
      { model: "claude-sonnet-5", provider: "anthropic", requests: 27, inputTokens: 1_890_954, outputTokens: 5_072, cacheReadTokens: 2_256_400, cacheWrite5mTokens: 282_050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "7483549500", chargedNano: "2993419800" },
      { model: "gpt-5.6-sol", provider: "openai", requests: 8, inputTokens: 420_000, outputTokens: 60_000, cacheReadTokens: 150_000, cacheWrite5mTokens: 0, cacheWrite1hTokens: 40_000, webSearchRequests: 0, officialNano: "3475000000", chargedNano: "1390000000" },
      { model: "claude-haiku-4-5-20251001", provider: "anthropic", requests: 5, inputTokens: 104, outputTokens: 4_996, cacheReadTokens: 354_058, cacheWrite5mTokens: 177_029, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "281776050", chargedNano: "112710420" },
      { model: "gpt-5.6-luna", provider: "openai", requests: 4, inputTokens: 100_000, outputTokens: 20_000, cacheReadTokens: 0, cacheWrite5mTokens: 0, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "220000000", chargedNano: "88000000" },
    ],
    daily: [
      { dayTs: todayUtc - 3 * DAY_S, requests: 20, officialNano: "8000000000", chargedNano: "3200000000" },
      { dayTs: todayUtc - 2 * DAY_S, requests: 16, officialNano: "6000000000", chargedNano: "2400000000" },
      { dayTs: todayUtc - DAY_S, requests: 13, officialNano: "5000000000", chargedNano: "2000000000" },
      { dayTs: todayUtc, requests: 22, officialNano: "7679893050", chargedNano: "3071957220" },
    ],
    dailyProviders: [
      { dayTs: todayUtc - 3 * DAY_S, provider: "anthropic", requests: 16, officialNano: "7000000000", chargedNano: "2800000000" },
      { dayTs: todayUtc - 3 * DAY_S, provider: "openai", requests: 4, officialNano: "1000000000", chargedNano: "400000000" },
      { dayTs: todayUtc - 2 * DAY_S, provider: "anthropic", requests: 13, officialNano: "5000000000", chargedNano: "2000000000" },
      { dayTs: todayUtc - 2 * DAY_S, provider: "openai", requests: 3, officialNano: "1000000000", chargedNano: "400000000" },
      { dayTs: todayUtc - DAY_S, provider: "anthropic", requests: 10, officialNano: "4000000000", chargedNano: "1600000000" },
      { dayTs: todayUtc - DAY_S, provider: "openai", requests: 3, officialNano: "1000000000", chargedNano: "400000000" },
      { dayTs: todayUtc, provider: "anthropic", requests: 20, officialNano: "6984893050", chargedNano: "2793957220" },
      { dayTs: todayUtc, provider: "openai", requests: 2, officialNano: "695000000", chargedNano: "278000000" },
    ],
    keys: [
      { keyMasked: "sk-pool-a5b5••••••••eeb", requests: 45, officialNano: "18000000000", chargedNano: "7200000000" },
      { keyMasked: "sk-pool-f367••••••••94ea", requests: 26, officialNano: "8679893050", chargedNano: "3471957220" },
    ],
  };
}

const providers: ProviderStatus = {
  email: { password: true, verificationRequired: false },
  google: { configured: true, enabled: true },
  github: { configured: true, enabled: true, emailScope: "user:email" },
};

const checkouts = new Map<string, CheckoutView & { polls: number }>();

function usdToNano(value: string): bigint {
  const [whole = "0", fraction = ""] = String(value).split(".");
  return BigInt(whole) * NANO + BigInt(fraction.padEnd(9, "0").slice(0, 9) || "0");
}

function randomHex(length: number): string {
  return Array.from({ length }, () => Math.floor(Math.random() * 16).toString(16)).join("");
}

const TOTP_SETUP: TotpSetup = {
  otpauthUri: "otpauth://totp/apitoken.sale:v2.preview@apitoken.sale?secret=JBSWY3DPEHPK3PXP&issuer=apitoken.sale",
  secret: "JBSWY3DPEHPK3PXP",
  qrDataUrl:
    "data:image/svg+xml," +
    encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120"><rect width="120" height="120" fill="#fff"/><text x="60" y="64" font-size="11" text-anchor="middle" fill="#111">preview QR</text></svg>',
    ),
};

function body(init: RequestInit): Record<string, unknown> {
  try {
    return JSON.parse(String(init.body ?? "{}")) as Record<string, unknown>;
  } catch {
    return {};
  }
}

// Небольшая пауза, чтобы состояния загрузки в UI оставались наблюдаемыми.
const delay = () => new Promise((resolve) => setTimeout(resolve, 120));

export async function previewRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  await delay();
  const method = (init.method ?? "GET").toUpperCase();
  const url = new URL(path, "https://preview.local");
  const route = `${method} ${url.pathname}`;

  switch (route) {
    case "GET /auth/providers": return providers as T;
    case "POST /auth/business-invite/preview": return { valid: false } as T;
    case "GET /auth/me": return { user } as T;
    case "PATCH /auth/me": {
      const next = body(init).displayName;
      if (typeof next === "string" && next.trim()) user.displayName = next.trim();
      return { user } as T;
    }
    case "POST /auth/register": return { user, verificationRequired: false } as T;
    case "POST /auth/login": return { user } as T;
    case "POST /auth/email/verify": return { user } as T;
    case "POST /auth/email/resend":
    case "POST /auth/password/forgot": return { accepted: true } as T;
    case "POST /auth/password/reset":
    case "POST /auth/logout": return undefined as T;
    case "POST /promo/redeem": {
      balanceNano += 10n * NANO;
      return { credited_usd: "10.00", credited_nano: "10000000000", balance_nano: balanceNano.toString() } as T;
    }
    case "GET /account": return account() as T;
    case "GET /account/ledger": return { entries: ledger } as T;
    case "GET /account/usage": return usage(url.searchParams.get("window") ?? "30d") as T;
    case "GET /api-keys": return { keys } as T;
    case "POST /api-keys": {
      const input = body(init);
      const suffix = randomHex(3);
      const created: ApiKeyView = {
        id: randomHex(32),
        label: typeof input.label === "string" && input.label ? input.label : null,
        keyMasked: `sk-pool-${randomHex(4)}••••••••${suffix}`,
        status: "active",
        spentNano: "0",
        spentUsd: "0.00",
        reservedNano: "0",
        spendLimitNano: typeof input.spendLimitUsd === "string" ? usdToNano(input.spendLimitUsd).toString() : null,
        expiresAt: typeof input.expiresAt === "string" ? input.expiresAt : null,
        lastUsedAt: null,
        createdAt: new Date().toISOString(),
        key: `sk-pool-preview-${randomHex(24)}${suffix}`,
      };
      keys.unshift({ ...created, key: undefined });
      return created as T;
    }
    case "POST /security/totp/setup": return TOTP_SETUP as T;
    case "POST /security/totp/enable": user.totpEnabled = true; return undefined as T;
    case "POST /security/totp/disable": user.totpEnabled = false; return undefined as T;
    case "POST /checkouts": {
      const amountUsd = String(body(init).amountUsd ?? "0");
      const checkout: CheckoutView & { polls: number } = {
        id: randomHex(16),
        provider: "platega",
        amountUsd,
        status: "pending",
        checkoutUrl: "https://apitoken.sale/docs#pricing",
        expiresAt: new Date(Date.now() + 30 * 60_000).toISOString(),
        polls: 0,
      };
      checkouts.set(checkout.id, checkout);
      return checkout as T;
    }
  }

  const keyPolicy = url.pathname.match(/^\/api-keys\/([^/]+)\/policy$/);
  if (keyPolicy && method === "PATCH") {
    const key = keys.find((candidate) => candidate.id === decodeURIComponent(keyPolicy[1]));
    if (!key) throw new ApiError("API key not found", 404);
    const input = body(init);
    key.spendLimitNano = typeof input.spendLimitUsd === "string" ? usdToNano(input.spendLimitUsd).toString() : null;
    key.expiresAt = typeof input.expiresAt === "string" ? input.expiresAt : null;
    return key as T;
  }
  const keyById = url.pathname.match(/^\/api-keys\/([^/]+)$/);
  if (keyById) {
    const id = decodeURIComponent(keyById[1]);
    const index = keys.findIndex((candidate) => candidate.id === id);
    if (index === -1) throw new ApiError("API key not found", 404);
    if (method === "PATCH") {
      const label = body(init).label;
      keys[index].label = typeof label === "string" && label ? label : null;
      return keys[index] as T;
    }
    if (method === "DELETE") {
      keys.splice(index, 1);
      return undefined as T;
    }
  }
  const checkoutById = url.pathname.match(/^\/checkouts\/([^/]+)$/);
  if (checkoutById && method === "GET") {
    const checkout = checkouts.get(decodeURIComponent(checkoutById[1]));
    if (!checkout) throw new ApiError("Checkout not found", 404);
    checkout.polls += 1;
    // Второй опрос статуса «оплачивает» чекаут и зачисляет баланс — поток пополнения живой.
    if (checkout.status === "pending" && checkout.polls >= 2) {
      checkout.status = "paid";
      balanceNano += usdToNano(checkout.amountUsd);
      ledger.unshift({
        id: `t-${checkout.id}`,
        kind: "topup",
        amountNano: usdToNano(checkout.amountUsd).toString(),
        amountUsd: `$${Number(checkout.amountUsd).toFixed(6)}`,
        keyMasked: null,
        reference: `platega_${checkout.id.slice(0, 6)}`,
        requestId: null,
        provider: null,
        officialNano: null,
        attribution: null,
        fundingAllocations: [],
        balanceAfterNano: balanceNano.toString(),
        timestamp: String(Math.floor(Date.now() / 1000)),
      });
    }
    return checkout as T;
  }

  throw new ApiError(`Preview fixture route not found: ${route}`, 404);
}
